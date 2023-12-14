mod extensions;
mod git;
mod methods;

#[macro_use]
extern crate rocket;

use std::io::Write;

use near_jsonrpc_client::JsonRpcClient;
use rocket::http::Status;

#[post("/")]
async fn webhook() -> Status {
    let json_rpc_client = JsonRpcClient::connect("https://rpc.mainnet.near.org");

    if let Ok(delegators) = methods::get_all_delegators(&json_rpc_client).await {
        let file_result = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(git::STAKE_DELEGATORS_FILENAME);

        let Ok(mut file) = file_result else { return Status::NotFound };

        file.set_len(0).unwrap();

        match serde_json::to_string_pretty(&delegators) {
            Ok(json) => {
                if file.write_all(json.as_bytes()).is_err() {
                    return Status::NotFound;
                }
            }
            Err(_) => {
                return Status::NotFound;
            }
        }

        if git::push().is_err() {
            return Status::NotFound;
        }
    }

    Status::Ok
}

#[launch]
fn rocket() -> _ {
    rocket::build().mount("/", routes![webhook])
}
