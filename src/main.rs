mod extensions;
mod methods;

#[macro_use]
extern crate rocket;

use std::io::Write;

use rocket::http::Status;

#[post("/")]
async fn webhook() -> Status {
    if let Ok(delegators) = methods::get_all_delegators().await {
        let file_result = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open("delegators.json");

        let Ok(mut file) = file_result else { return Status::NotFound };

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
    }

    Status::Ok
}

#[launch]
fn rocket() -> _ {
    rocket::build().mount("/", routes![webhook])
}
