mod extensions;
mod git;
mod methods;

#[macro_use]
extern crate rocket;

use std::io::{Read, Seek, Write};

use near_jsonrpc_client::JsonRpcClient;
use rocket::http::Status;

#[post("/")]
async fn webhook() -> Status {
    info!("Webhook received");

    let json_rpc_client = JsonRpcClient::connect("https://rpc.mainnet.near.org");

    if let Ok(mut file) = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open(git::STAKE_DELEGATORS_FILENAME)
    {
        let mut existing_delegators = String::new();
        if file.read_to_string(&mut existing_delegators).is_err() {
            error!("Failed to read from file");
            return Status::NotFound;
        }

        if let Ok(delegators) = methods::get_all_delegators(&json_rpc_client).await {
            if let Ok(current_json) = serde_json::to_string_pretty(&delegators) {
                if current_json == existing_delegators {
                    info!("Delegators in file are up-to-date");
                    return Status::Ok;
                }

                if file.seek(std::io::SeekFrom::Start(0)).is_err() {
                    error!("Failed to seek to the beginning of the file");
                    return Status::NotFound;
                }

                if file.set_len(0).is_err() {
                    error!("Failed to truncate the file");
                    return Status::NotFound;
                }

                if file.write_all(current_json.as_bytes()).is_err() {
                    error!("Failed to write to file");
                    return Status::NotFound;
                }

                info!("Delegators updated in file");

                if git::push().is_err() {
                    error!("Failed to push to Git");
                    return Status::NotFound;
                }
            } else {
                error!("Failed to serialize current delegators");
                return Status::NotFound;
            }
        } else {
            error!("Failed to get current delegators from JSON-RPC client");
            return Status::NotFound;
        }
    } else {
        error!("Failed to open the file");
        return Status::NotFound;
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

    rocket::build().mount("/", routes![webhook])
}
