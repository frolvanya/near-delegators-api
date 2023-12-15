mod extensions;
mod git;
mod methods;

#[macro_use]
extern crate rocket;

use std::io::Write;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

use color_eyre::eyre::{Context, Result};
use rocket::http::Status;

async fn update_stake_delegators() -> Result<()> {
    let mut file = tokio::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open(git::STAKE_DELEGATORS_FILENAME)
        .await
        .context("Failed to open the file")?;

    let mut existing_delegators = String::new();
    file.read_to_string(&mut existing_delegators)
        .await
        .context("Failed to read from file")?;

    let delegators = methods::get_all_delegators().await?;

    let current_json = serde_json::to_string_pretty(&delegators)?;

    if current_json == existing_delegators {
        info!("Delegators in file are up-to-date");
        return Ok(());
    }

    file.seek(std::io::SeekFrom::Start(0))
        .await
        .context("Failed to seek to the beginning of the file")?;

    file.set_len(0)
        .await
        .context("Failed to truncate the file")?;

    file.write_all(current_json.as_bytes())
        .await
        .context("Failed to write to file")?;

    info!("Delegators updated in file");

    git::push()?;

    Ok(())
}

#[post("/")]
async fn webhook() -> Status {
    info!("Webhook received");

    if let Err(e) = update_stake_delegators().await {
        error!("Error processing webhook: {}", e);
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

    rocket::build().mount("/", routes![webhook])
}
