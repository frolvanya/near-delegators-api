use crate::methods;

use color_eyre::{eyre::Context, Result};
use near_jsonrpc_client::JsonRpcClient;
use std::collections::{BTreeMap, BTreeSet};

use std::sync::Arc;
use tokio::sync::Mutex;

use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

pub const DELEGATORS_FILENAME: &str = "delegators.json";

#[derive(Debug, Clone, Default)]
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

pub async fn get_delegators_from_cache() -> Result<DelegatorsWithTimestamp> {
    let mut content = String::new();

    let mut file = with_json_file_cache().await?;
    file.read_to_string(&mut content)
        .await
        .context("Failed to read from file")?;

    Ok(serde_json::from_str(&content).map_or_else(
        |_| {
            info!("File is empty");
            DelegatorsWithTimestamp::default()
        },
        |data| data,
    ))
}

pub async fn update_delegators_cache(
    delegators_with_timestamp: &Arc<Mutex<DelegatorsWithTimestamp>>,
    validators_with_timestamp: &Arc<Mutex<ValidatorsWithTimestamp>>,
    receipt_id: Option<&str>,
) -> Result<(DelegatorsWithTimestamp, ValidatorsWithTimestamp)> {
    let beta_json_rpc_client = JsonRpcClient::connect("https://beta.rpc.mainnet.near.org");

    let timestamp = chrono::Utc::now().timestamp();

    let (mut updated_delegators_with_timestamp, mut updated_validators_with_timestamp) =
        (None, None);

    if let Some(receipt_id) = receipt_id {
        for _ in 0..20 {
            if let Ok(receiver_id) =
                methods::get_receiver_id(&beta_json_rpc_client, receipt_id).await
            {
                info!("Updating delegators for validator: {}", receiver_id);

                let validator_delegators = methods::get_delegators_by_validator_account_id(
                    &beta_json_rpc_client,
                    receiver_id.clone(),
                )
                .await?;

                info!("Updated delegators for validator: {}", receiver_id);

                let mut validators_with_timestamp = validators_with_timestamp.lock().await;
                validators_with_timestamp.timestamp = timestamp;
                validators_with_timestamp
                    .validators
                    .insert(receiver_id.clone(), validator_delegators);

                (
                    updated_delegators_with_timestamp,
                    updated_validators_with_timestamp,
                ) = (
                    Some(DelegatorsWithTimestamp::from(
                        &validators_with_timestamp.clone(),
                    )),
                    Some(validators_with_timestamp.clone()),
                );

                break;
            }

            warn!("Failed to get receiver_id for receipt_id: {receipt_id}. Retrying...");
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    }

    if updated_delegators_with_timestamp.is_none() && updated_validators_with_timestamp.is_none() {
        info!("Updating all delegators");

        let mut delegators_with_timestamp = delegators_with_timestamp.lock().await;
        let updated_delegators = methods::get_all_delegators(&beta_json_rpc_client)
            .await
            .context("Failed to get all delegators")?;

        if timestamp - delegators_with_timestamp.timestamp < 1800
            && delegators_with_timestamp.delegators == updated_delegators
        {
            info!("Delegators in file are up-to-date");
            return Ok((
                delegators_with_timestamp.clone(),
                validators_with_timestamp.lock().await.clone(),
            ));
        }

        delegators_with_timestamp.timestamp = timestamp;
        delegators_with_timestamp.delegators = updated_delegators;

        (
            updated_delegators_with_timestamp,
            updated_validators_with_timestamp,
        ) = (
            Some(delegators_with_timestamp.clone()),
            Some(ValidatorsWithTimestamp::from(
                &delegators_with_timestamp.clone(),
            )),
        );
    }

    let Some(updated_delegators_with_timestamp) = updated_delegators_with_timestamp else {
        color_eyre::eyre::bail!("Failed to update delegators");
    };
    let Some(updated_validators_with_timestamp) = updated_validators_with_timestamp else {
        color_eyre::eyre::bail!("Failed to update validators");
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
