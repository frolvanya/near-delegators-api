use crate::methods;

use color_eyre::{eyre::Context, Result};
use std::collections::{BTreeMap, BTreeSet};

use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

pub const DELEGATORS_FILENAME: &str = "delegators.json";

#[derive(Debug, Clone)]
pub struct ValidatorsWithTimestamp {
    pub timestamp: i64,
    pub validators: BTreeMap<String, BTreeSet<String>>,
}

impl From<&DelegatorsWithTimestamp> for ValidatorsWithTimestamp {
    fn from(delegators: &DelegatorsWithTimestamp) -> Self {
        let mut validators_map = BTreeMap::<String, BTreeSet<String>>::new();

        for (delegator, validators) in &delegators.delegators {
            for validator in validators {
                validators_map
                    .entry(validator.to_string())
                    .or_default()
                    .insert(delegator.clone());
            }
        }

        Self {
            timestamp: delegators.timestamp,
            validators: validators_map,
        }
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Default, Clone)]
#[serde(crate = "rocket::serde")]
pub struct DelegatorsWithTimestamp {
    pub timestamp: i64,
    pub delegators: BTreeMap<String, BTreeSet<String>>,
}

impl From<&ValidatorsWithTimestamp> for DelegatorsWithTimestamp {
    fn from(validators: &ValidatorsWithTimestamp) -> Self {
        let mut delegators_map = BTreeMap::<String, BTreeSet<String>>::new();

        for (validator, delegators) in &validators.validators {
            for delegator in delegators {
                delegators_map
                    .entry(delegator.to_string())
                    .or_default()
                    .insert(validator.clone());
            }
        }

        Self {
            timestamp: validators.timestamp,
            delegators: delegators_map,
        }
    }
}

pub async fn with_json_file_cache() -> Result<tokio::fs::File> {
    let path = format!(
        "{}/{DELEGATORS_FILENAME}",
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
) -> Result<DelegatorsWithTimestamp> {
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

pub async fn update_delegators_cache(
    mut delegators_with_timestamp: DelegatorsWithTimestamp,
    mut validators_with_timestamp: ValidatorsWithTimestamp,
    receipt_id: Option<&str>,
) -> Result<(DelegatorsWithTimestamp, ValidatorsWithTimestamp)> {
    let timestamp = chrono::Utc::now().timestamp();
    let (updated_delegators_with_timestamp, updated_validators_with_timestamp) =
        if let Some(receipt_id) = receipt_id {
            info!("Updating delegators for validator: {}", receipt_id);

            let Ok(receiver_id) = methods::get_receiver_id(receipt_id).await else {
                color_eyre::eyre::bail!("Failed to get receiver_id for receipt_id: {receipt_id}");
            };

            validators_with_timestamp.timestamp = timestamp;
            validators_with_timestamp.validators.insert(
                receiver_id.clone(),
                methods::get_delegators_by_validator_account_id(receiver_id).await?,
            );

            (
                DelegatorsWithTimestamp::from(&validators_with_timestamp),
                validators_with_timestamp,
            )
        } else {
            info!("Updating all delegators");

            let updated_delegators = methods::get_all_delegators().await?;
            if timestamp - delegators_with_timestamp.timestamp < 1800
                && delegators_with_timestamp.delegators == updated_delegators
            {
                info!("Delegators in file are up-to-date");
                return Ok((delegators_with_timestamp, validators_with_timestamp));
            }

            delegators_with_timestamp.timestamp = timestamp;
            delegators_with_timestamp.delegators = updated_delegators;

            (
                delegators_with_timestamp.clone(),
                ValidatorsWithTimestamp::from(&delegators_with_timestamp),
            )
        };

    let updated_delegators_json = serde_json::to_string_pretty(&updated_delegators_with_timestamp)?;

    let mut file = with_json_file_cache().await?;
    file.seek(std::io::SeekFrom::Start(0))
        .await
        .context("Failed to seek to the beginning of the file")?;

    file.set_len(0)
        .await
        .context("Failed to truncate the file")?;

    file.write_all(updated_delegators_json.as_bytes())
        .await
        .context("Failed to write to file")?;

    info!("Updated delegators file");

    Ok((
        updated_delegators_with_timestamp,
        updated_validators_with_timestamp,
    ))
}
