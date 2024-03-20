mod delegators;
mod extensions;
mod methods;

#[macro_use]
extern crate rocket;

use std::io::Write;

use color_eyre::Result;

use near_jsonrpc_client::JsonRpcClient;
use rocket::http::Status;
use rocket::serde::json::Json;
use rocket::State;

use serde::{Deserialize, Serialize};
use shuttle_persist::PersistInstance;
use std::collections::BTreeMap;

use std::sync::Arc;
use tokio::sync::{mpsc::Sender, RwLock};

#[derive(Debug, Deserialize, Serialize)]
struct WebhookData {
    payload: Payload,
}

#[derive(Debug, Deserialize, Serialize)]
struct Payload {
    #[serde(rename = "Actions")]
    actions: Actions,
}

#[derive(Debug, Deserialize, Serialize)]
struct Actions {
    receipt_id: Option<String>,
    block_hash: Option<String>,
}

#[derive(Clone)]
struct AppState {
    validators_to_process: Arc<RwLock<BTreeMap<String, u64>>>,
    validators_state: Arc<RwLock<delegators::ValidatorsWithTimestamp>>,
    delegators_state: Arc<RwLock<delegators::DelegatorsWithTimestamp>>,
    tx: Sender<()>,
}

#[get("/get-staking-pools")]
async fn get_all(state: &State<AppState>) -> Json<delegators::DelegatorsWithTimestamp> {
    info!("GET request received");

    Json(state.delegators_state.read().await.clone())
}

#[get("/get-staking-pools/<account_id>")]
async fn get_by_account_id(
    account_id: &str,
    state: &State<AppState>,
) -> Result<(Status, Json<delegators::DelegatorWithTimestamp>), Status> {
    info!("GET by account id request received");

    let locked_delegators_state = state.delegators_state.read().await;

    locked_delegators_state
        .delegator_staking_pools
        .get(account_id)
        .map_or_else(
            || Err(Status::new(503)),
            |delegators| {
                Ok((
                    Status::Ok,
                    Json(delegators::DelegatorWithTimestamp {
                        timestamp: locked_delegators_state.timestamp,
                        delegator_staking_pools: delegators.clone(),
                    }),
                ))
            },
        )
}

#[post("/update-staking-pools", data = "<data>")]
async fn update(data: Json<WebhookData>, state: &State<AppState>) -> Status {
    info!("POST request received");

    let Some(receipt_id) = data.payload.actions.receipt_id.clone() else {
        return Status::InternalServerError;
    };
    let Some(block_hash) = data.payload.actions.block_hash.clone() else {
        return Status::InternalServerError;
    };
    let Ok(block_hash) = block_hash.parse::<near_primitives::hash::CryptoHash>() else {
        return Status::InternalServerError;
    };

    let beta_json_rpc_client = JsonRpcClient::connect("https://beta.rpc.mainnet.near.org");

    let block_reference = near_primitives::types::BlockReference::BlockId(
        near_primitives::types::BlockId::Hash(block_hash),
    );
    let Ok(block_id) = methods::get_block_id(&beta_json_rpc_client, block_reference).await else {
        return Status::InternalServerError;
    };

    if let Ok(receiver_id) = methods::get_receiver_id(&beta_json_rpc_client, receipt_id).await {
        state
            .validators_to_process
            .write()
            .await
            .entry(receiver_id)
            .and_modify(|prev_block_id| {
                if *prev_block_id < block_id {
                    *prev_block_id = block_id;
                }
            })
            .or_insert(block_id);

        tokio::time::sleep(std::time::Duration::from_secs(1)).await;

        if state.tx.send(()).await.is_err() {
            error!("Failed to send message to the worker");
        }
    }

    Status::Ok
}

#[shuttle_runtime::main]
async fn rocket(
    #[shuttle_persist::Persist] persist: PersistInstance,
) -> shuttle_rocket::ShuttleRocket {
    pretty_env_logger::formatted_timed_builder()
        .format(|buf, record| {
            writeln!(
                buf,
                "{} [{}] - {}",
                chrono::Local::now().format("%d-%b-%Y %H:%M:%S"),
                record.level(),
                record.args()
            )
        })
        .filter(None, log::LevelFilter::Info)
        .target(pretty_env_logger::env_logger::fmt::Target::Stdout)
        .init();

    let (tx, mut rx) = tokio::sync::mpsc::channel(100);

    let mut initial_delegators_state = delegators::DelegatorsWithTimestamp::default();
    let mut initial_validators_state = delegators::ValidatorsWithTimestamp::default();

    if let Ok(delegators_state) =
        persist.load::<delegators::DelegatorsWithTimestamp>("delegators_state")
    {
        initial_delegators_state = delegators_state.clone();
        initial_validators_state = delegators::ValidatorsWithTimestamp::from(&delegators_state);
    }

    let app_state = AppState {
        validators_to_process: Arc::new(RwLock::new(BTreeMap::new())),
        delegators_state: Arc::new(RwLock::new(initial_delegators_state)),
        validators_state: Arc::new(RwLock::new(initial_validators_state)),
        tx,
    };
    let app_state_clone = app_state.clone();

    let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));

    tokio::spawn(async move {
        let beta_json_rpc_client = JsonRpcClient::connect("https://beta.rpc.mainnet.near.org");

        loop {
            interval.tick().await;

            if chrono::Utc::now().timestamp()
                - app_state_clone.delegators_state.read().await.timestamp
                > 1800
            {
                let block_reference = near_primitives::types::Finality::Final.into();

                let Ok(block_id) =
                    methods::get_block_id(&beta_json_rpc_client, block_reference).await
                else {
                    error!("Failed to get block id");
                    continue;
                };

                let Ok(validators_to_update) =
                    methods::get_all_validators(&beta_json_rpc_client).await
                else {
                    error!("Failed to get all validators");
                    continue;
                };

                let mut validators_to_process = app_state_clone.validators_to_process.write().await;

                for validator in validators_to_update {
                    validators_to_process
                        .entry(validator)
                        .and_modify(|prev_block_id| {
                            if *prev_block_id < block_id {
                                *prev_block_id = block_id;
                            }
                        })
                        .or_insert(block_id);
                }

                drop(validators_to_process);

                if app_state_clone.tx.send(()).await.is_err() {
                    error!("Failed to send message to the worker");
                }
            }
        }
    });

    let app_state_clone = app_state.clone();
    tokio::spawn(async move {
        let json_rpc_client = JsonRpcClient::connect("https://rpc.mainnet.near.org");

        while rx.recv().await.is_some() {
            let mut validators_to_process = BTreeMap::new();
            std::mem::swap(
                &mut *app_state_clone.validators_to_process.write().await,
                &mut validators_to_process,
            );

            let mut handles = Vec::new();

            for (account_id, block_id) in validators_to_process {
                let app_state_clone = app_state_clone.clone();
                let beta_json_rpc_client = json_rpc_client.clone();
                handles.push(tokio::spawn(async move {
                    if let Err(e) = delegators::update_delegators_by_validator_account_id(
                        &beta_json_rpc_client,
                        &app_state_clone.delegators_state,
                        &app_state_clone.validators_state,
                        account_id.clone(),
                        block_id,
                    )
                    .await
                    {
                        error!("Error updating delegators: {}", e);
                    }
                }));
            }

            futures::future::join_all(handles).await;

            if let Err(e) = persist.save(
                "delegators_state",
                &*app_state_clone.delegators_state.read().await,
            ) {
                error!("Error saving delegators state: {}", e);
            }
        }
    });

    let rocket = rocket::build()
        .mount("/", routes![get_all, get_by_account_id, update])
        .manage(app_state);

    Ok(rocket.into())
}
