mod delegators;
mod extensions;
mod methods;

#[macro_use]
extern crate rocket;

use std::io::Write;
use tokio::io::AsyncSeekExt;

use rocket::http::Status;
use rocket::serde::json::Json;
use rocket::State;

use color_eyre::{eyre::Context, Result};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Clone)]
struct AppState {
    validators_state: Arc<Mutex<delegators::ValidatorsWithTimestamp>>,
    delegators_state: Arc<Mutex<delegators::DelegatorsWithTimestamp>>,
}

#[get("/get-delegators")]
async fn get_all(state: &State<AppState>) -> Json<delegators::DelegatorsWithTimestamp> {
    info!("GET request received");

    Json(state.delegators_state.lock().await.clone())
}

#[get("/get-delegators/<account_id>")]
async fn get_by_account_id(
    account_id: &str,
    state: &State<AppState>,
) -> (Status, Json<delegators::DelegatorsWithTimestamp>) {
    info!("GET by account id request received");

    let locked_delegators_state = state.delegators_state.lock().await;

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

    let mut locked_delegators_state = state.delegators_state.lock().await;
    let mut locked_validators_state = state.validators_state.lock().await;

    match delegators::update_delegators_cache(
        locked_delegators_state.clone(),
        locked_validators_state.clone(),
        receipt_id,
    )
    .await
    {
        Ok((updated_delegators, updated_validators)) => {
            *locked_delegators_state = updated_delegators;
            *locked_validators_state = updated_validators;

            Status::Ok
        }
        Err(e) => {
            error!("Error processing POST request: {}", e);
            Status::InternalServerError
        }
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
                chrono::Local::now().format("%H:%M:%S"),
                record.level(),
                record.args()
            )
        })
        .filter(None, log::LevelFilter::Info)
        .init();

    let mut file = delegators::with_json_file_cache().await?;

    file.seek(std::io::SeekFrom::Start(0))
        .await
        .context("Failed to seek to the beginning of the file")?;

    file.set_len(0)
        .await
        .context("Failed to truncate the file")?;

    let initial_delegators_state = delegators::get_delegators_from_cache()
        .await
        .unwrap_or_default();
    let initial_validators_state =
        delegators::ValidatorsWithTimestamp::from(&initial_delegators_state);
    let app_state = AppState {
        delegators_state: Arc::new(Mutex::new(initial_delegators_state)),
        validators_state: Arc::new(Mutex::new(initial_validators_state)),
    };
    let app_state_clone = app_state.clone();

    let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));

    tokio::spawn(async move {
        loop {
            interval.tick().await;

            match delegators::get_delegators_from_cache().await {
                Ok(data) => {
                    if chrono::Utc::now().timestamp() - data.timestamp > 1800 {
                        let mut locked_delegators_state =
                            app_state_clone.delegators_state.lock().await;
                        let mut locked_validators_state =
                            app_state_clone.validators_state.lock().await;

                        match delegators::update_delegators_cache(
                            locked_delegators_state.clone(),
                            locked_validators_state.clone(),
                            None,
                        )
                        .await
                        {
                            Ok((updated_delegators, updated_validators)) => {
                                *locked_delegators_state = updated_delegators;
                                *locked_validators_state = updated_validators;
                            }
                            Err(e) => {
                                error!("Error updating delegators: {}", e);
                            }
                        }
                    }
                }
                Err(e) => error!("Error updating delegators: {}", e),
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
