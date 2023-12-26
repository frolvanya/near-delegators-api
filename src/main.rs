mod extensions;
mod methods;
mod stake_delegators;

#[macro_use]
extern crate rocket;

use std::io::Write;

use rocket::http::Status;
use rocket::serde::json::Json;

#[get("/get-stake-delegators")]
async fn get_handler() -> Json<stake_delegators::DelegatorsWithTimestamp> {
    info!("GET request received");

    match stake_delegators::get().await {
        Ok(data) => Json(data),
        Err(e) => {
            error!("Error processing GET request: {}", e);
            Json(stake_delegators::DelegatorsWithTimestamp::default())
        }
    }
}

#[post("/update-stake-delegators")]
async fn post_handler() -> Status {
    info!("POST request received");

    if let Err(e) = stake_delegators::update().await {
        error!("Error processing POST request: {}", e);
        return Status::InternalServerError;
    }

    Status::Ok
}

#[launch]
fn rocket() -> _ {
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

    rocket::build().mount("/", routes![get_handler, post_handler])
}
