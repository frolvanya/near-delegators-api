use crate::methods;

use color_eyre::{eyre::Context, Result};

use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

pub const STAKE_DELEGATORS_FILENAME: &str = "stake_delegators.json";

#[derive(Debug, serde::Serialize, serde::Deserialize, Default, Clone)]
#[serde(crate = "rocket::serde")]
pub struct DelegatorsWithTimestamp {
    pub timestamp: i64,
    pub stake_delegators: std::collections::BTreeMap<String, String>,
}

async fn with_json_file_cache() -> Result<tokio::fs::File> {
    let path = format!(
        "{}/{STAKE_DELEGATORS_FILENAME}",
        std::env::var("HOME").unwrap_or_default()
    );

    tokio::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open(path)
        .await
        .context("Failed to open file")
}

pub async fn read_delegators_from_file(
    file: &mut tokio::fs::File,
) -> Result<DelegatorsWithTimestamp, color_eyre::eyre::Error> {
    let mut content = String::new();
    file.read_to_string(&mut content)
        .await
        .context("Failed to read from file")?;

    let data = serde_json::from_str(&content).map_or_else(
        |_| {
            info!("File is empty");
            DelegatorsWithTimestamp::default()
        },
        |data| data,
    );

    Ok(data)
}

pub async fn get_delegators_from_cache() -> Result<DelegatorsWithTimestamp> {
    let mut file = with_json_file_cache().await?;

    read_delegators_from_file(&mut file).await
}

pub async fn update_stake_delegators_cache() -> Result<DelegatorsWithTimestamp> {
    let mut file = with_json_file_cache().await?;

    let existing_delegators = read_delegators_from_file(&mut file).await?;

    let new_delegators = methods::get_all_delegators().await?;

    let timestamp = chrono::Utc::now().timestamp();
    let updated_data = DelegatorsWithTimestamp {
        timestamp,
        stake_delegators: new_delegators.clone(),
    };
    let updated_data_json = serde_json::to_string_pretty(&updated_data)?;

    if timestamp - existing_delegators.timestamp < 1800
        && new_delegators == existing_delegators.stake_delegators
    {
        info!("Stake delegators in file are up-to-date");
        return Ok(existing_delegators);
    }

    file.seek(std::io::SeekFrom::Start(0))
        .await
        .context("Failed to seek to the beginning of the file")?;

    file.set_len(0)
        .await
        .context("Failed to truncate the file")?;

    file.write_all(updated_data_json.as_bytes())
        .await
        .context("Failed to write to file")?;

    info!("Updated stake delegators file");

    Ok(updated_data)
}
