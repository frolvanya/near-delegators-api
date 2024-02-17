mod delegators;
mod extensions;
mod methods;

#[macro_use]
extern crate rocket;

use std::io::Write;

use near_jsonrpc_client::JsonRpcClient;
use rocket::http::Status;
use rocket::serde::json::Json;
use rocket::State;

use color_eyre::Result;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

use std::sync::Arc;
use tokio::sync::{mpsc::Sender, RwLock};

#[derive(Clone)]
struct AppState {
    accounts_to_process: Arc<RwLock<BTreeSet<String>>>,
    validators_state: Arc<RwLock<delegators::ValidatorsWithTimestamp>>,
    delegators_state: Arc<RwLock<delegators::DelegatorsWithTimestamp>>,
    tx: Sender<()>,
}

#[get("/get-delegators")]
async fn get_all(state: &State<AppState>) -> Json<delegators::DelegatorsWithTimestamp> {
    info!("GET request received");

    Json(state.delegators_state.read().await.clone())
}

#[get("/get-delegators/<account_id>")]
async fn get_by_account_id(
    account_id: &str,
    state: &State<AppState>,
) -> (Status, Json<delegators::DelegatorsWithTimestamp>) {
    info!("GET by account id request received");

    let locked_delegators_state = state.delegators_state.read().await;

    locked_delegators_state
        .delegators
        .get(account_id)
        .map_or_else(
            || {
                (
                    Status::InternalServerError,
                    Json(delegators::DelegatorsWithTimestamp::default()),
                )
            },
            |delegators| {
                let mut delegators_map = BTreeMap::<String, BTreeSet<String>>::new();
                delegators_map.insert(account_id.to_string(), delegators.clone());

                (
                    Status::Ok,
                    Json(delegators::DelegatorsWithTimestamp {
                        timestamp: locked_delegators_state.timestamp,
                        delegators: delegators_map,
                    }),
                )
            },
        )
}

#[post("/update-delegators", data = "<data>")]
async fn update(data: Json<Value>, state: &State<AppState>) -> Status {
    info!("POST request received");

    let receipt_id = data["payload"]["Actions"]["receipt_id"].as_str();

    if let Some(receipt_id) = receipt_id {
        let beta_json_rpc_client = JsonRpcClient::connect("https://beta.rpc.mainnet.near.org");

        for _ in 0..20 {
            if let Ok(receiver_id) =
                methods::get_receiver_id(&beta_json_rpc_client, receipt_id).await
            {
                state.accounts_to_process.write().await.insert(receiver_id);

                tokio::time::sleep(std::time::Duration::from_secs(1)).await;

                if state.tx.send(()).await.is_err() {
                    error!("Failed to send message to the worker");
                }

                break;
            }

            warn!("Failed to get receiver_id for receipt_id: {receipt_id}. Retrying...");
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }

        Status::Ok
    } else {
        Status::InternalServerError
    }
}

#[tokio::main]
#[allow(clippy::no_effect_underscore_binding)]
async fn main() -> Result<()> {
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
        .init();

    let (tx, mut rx) = tokio::sync::mpsc::channel(100);

    let initial_delegators_state = delegators::get_delegators_from_cache()
        .await
        .unwrap_or_default();
    let initial_validators_state =
        delegators::ValidatorsWithTimestamp::from(&initial_delegators_state);

    let app_state = AppState {
        accounts_to_process: Arc::new(RwLock::new(BTreeSet::new())),
        delegators_state: Arc::new(RwLock::new(initial_delegators_state)),
        validators_state: Arc::new(RwLock::new(initial_validators_state)),
        tx,
    };
    let app_state_clone = app_state.clone();

    let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));

    tokio::spawn(async move {
        loop {
            interval.tick().await;

            match delegators::get_delegators_from_cache().await {
                Ok(data) => {
                    if chrono::Utc::now().timestamp() - data.timestamp > 1800 {
                        info!(
                            "Before: {:?}",
                            app_state_clone
                                .delegators_state
                                .read()
                                .await
                                .clone()
                                .timestamp
                        );

                        if let Err(e) = delegators::update_all_delegators(
                            &app_state_clone.delegators_state,
                            &app_state_clone.validators_state,
                        )
                        .await
                        {
                            error!("Error updating delegators: {}", e);
                        }

                        info!(
                            "After: {:?}",
                            app_state_clone
                                .delegators_state
                                .read()
                                .await
                                .clone()
                                .timestamp
                        );
                    }
                }
                Err(e) => error!("Error updating delegators: {}", e),
            }
        }
    });

    let app_state_clone = app_state.clone();
    tokio::spawn(async move {
        while rx.recv().await.is_some() {
            let mut accounts_to_process = app_state_clone.accounts_to_process.write().await;

            let mut account_ids = Vec::new();
            for _ in 0..5 {
                if let Some(account_id) = accounts_to_process.pop_first() {
                    account_ids.push(account_id);
                } else {
                    break;
                }
            }

            for account_id in account_ids {
                if let Err(e) = delegators::update_delegators_by_validator_account_id(
                    &app_state_clone.delegators_state,
                    &app_state_clone.validators_state,
                    account_id,
                )
                .await
                {
                    error!("Error updating delegators: {}", e);
                }
            }
        }
    });

    let _ = rocket::build()
        .mount("/", routes![get_all, get_by_account_id, update])
        .manage(app_state)
        .launch()
        .await;

    Ok(())
}
