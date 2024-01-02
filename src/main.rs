mod extensions;
mod methods;
mod stake_delegators;

#[macro_use]
extern crate rocket;

use std::io::Write;

use rocket::http::Status;
use rocket::serde::json::Json;
use rocket::State;

use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Clone)]
struct AppState {
    delegators_state: Arc<Mutex<stake_delegators::DelegatorsWithTimestamp>>,
}

#[get("/get-stake-delegators")]
async fn get_all(state: &State<AppState>) -> Json<stake_delegators::DelegatorsWithTimestamp> {
    info!("GET request received");

    Json(state.delegators_state.lock().await.clone())
}

#[get("/get-stake-delegators/<account_id>")]
async fn get_by_account_id(
    account_id: &str,
    state: &State<AppState>,
) -> Json<stake_delegators::DelegatorsWithTimestamp> {
    info!("GET by account id request received");

    let locked_state = state.delegators_state.lock().await;
    Json(stake_delegators::DelegatorsWithTimestamp {
        timestamp: locked_state.timestamp,
        stake_delegators: locked_state
            .stake_delegators
            .iter()
            .filter(|(k, _)| **k == account_id)
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect::<std::collections::BTreeMap<String, String>>(),
    })
}

#[post("/update-stake-delegators")]
async fn update(state: &State<AppState>) -> Status {
    info!("POST request received");

    match stake_delegators::update_stake_delegators_cache().await {
        Ok(updated_state) => {
            let mut locked_state = state.delegators_state.lock().await;
            *locked_state = updated_state;
        }
        Err(e) => {
            error!("Error processing POST request: {}", e);
        }
    }

    Status::Ok
}

#[tokio::main]
#[allow(clippy::no_effect_underscore_binding)]
async fn main() {
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

    let initial_delegators_state = stake_delegators::get_delegators_from_cache()
        .await
        .unwrap_or_default();
    let app_state = AppState {
        delegators_state: Arc::new(Mutex::new(initial_delegators_state)),
    };
    let app_state_clone = app_state.clone();

    let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));

    tokio::spawn(async move {
        loop {
            interval.tick().await;
            match stake_delegators::get_delegators_from_cache().await {
                Ok(data) => {
                    if chrono::Utc::now().timestamp() - data.timestamp > 1800 {
                        match stake_delegators::update_stake_delegators_cache().await {
                            Ok(updated_state) => {
                                let mut locked_state =
                                    app_state_clone.delegators_state.lock().await;
                                *locked_state = updated_state;
                            }
                            Err(e) => {
                                error!("Error updating stake delegators: {}", e);
                            }
                        }
                    }
                }
                Err(e) => error!("Error updating stake delegators: {}", e),
            }
        }
    });

    let _ = rocket::build()
        .mount("/", routes![get_all, get_by_account_id, update])
        .manage(app_state)
        .launch()
        .await;
}
