use crate::methods;

use color_eyre::{eyre::Context, Result};

use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

pub const STAKE_DELEGATORS_FILENAME: &str = "stake_delegators.json";

#[derive(Debug, serde::Serialize, serde::Deserialize, Default)]
#[serde(crate = "rocket::serde")]
pub struct DelegatorsWithTimestamp {
    timestamp: i64,
    stake_delegators: std::collections::BTreeMap<String, String>,
}

async fn open_file() -> Result<tokio::fs::File> {
    tokio::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open(STAKE_DELEGATORS_FILENAME)
        .await
        .context("Failed to open the file")
}

pub async fn read(
    file: &mut tokio::fs::File,
) -> Result<DelegatorsWithTimestamp, color_eyre::eyre::Error> {
    let mut existing_content = String::new();
    file.read_to_string(&mut existing_content)
        .await
        .context("Failed to read from file")?;

    let existing_data = serde_json::from_str(&existing_content).map_or_else(
        |_| {
            info!("File is empty");
            DelegatorsWithTimestamp::default()
        },
        |data| data,
    );

    Ok(existing_data)
}

pub async fn get() -> Result<DelegatorsWithTimestamp> {
    let mut file = open_file().await?;

    let existing_data = read(&mut file).await?;

    Ok(existing_data)
}

pub async fn update() -> Result<()> {
    let mut file = open_file().await?;

    let existing_data = read(&mut file).await?;

    let delegators = methods::get_all_delegators().await?;

    let timestamp = chrono::Utc::now().timestamp();
    let current_data = serde_json::to_string_pretty(&DelegatorsWithTimestamp {
        timestamp,
        stake_delegators: delegators.clone(),
    })?;

    if timestamp - existing_data.timestamp < 3600 && delegators == existing_data.stake_delegators {
        info!("Stake delegators in file are up-to-date");
        return Ok(());
    }

    file.seek(std::io::SeekFrom::Start(0))
        .await
        .context("Failed to seek to the beginning of the file")?;

    file.set_len(0)
        .await
        .context("Failed to truncate the file")?;

    file.write_all(current_data.as_bytes())
        .await
        .context("Failed to write to file")?;

    info!("Updated stake delegators file");

    Ok(())
}
