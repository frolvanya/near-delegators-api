use crate::methods;

use color_eyre::{eyre::Context, Result};
use near_jsonrpc_client::JsonRpcClient;
use std::collections::{BTreeMap, BTreeSet};

use std::sync::Arc;
use tokio::sync::RwLock;

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
    // let path = format!(
    //     "{}/{DELEGATORS_FILENAME}",
    //     std::env::var("HOME").unwrap_or_default()
    // );
    let path = format!("/mnt/{DELEGATORS_FILENAME}");

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
    delegators_with_timestamp: &Arc<RwLock<DelegatorsWithTimestamp>>,
) -> Result<()> {
    info!("Updating delegators file");
    let updated_delegators_json =
        serde_json::to_string_pretty(&delegators_with_timestamp.read().await.clone())?;
    info!("Updated delegators JSON");

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

    Ok(())
}

pub async fn update_all_delegators(
    delegators_with_timestamp: &Arc<RwLock<DelegatorsWithTimestamp>>,
    validators_with_timestamp: &Arc<RwLock<ValidatorsWithTimestamp>>,
) -> Result<()> {
    info!("Updating all delegators");

    let updated_delegators =
        methods::get_all_delegators(&JsonRpcClient::connect("https://beta.rpc.mainnet.near.org"))
            .await
            .context("Failed to get all delegators")?;

    info!("Fetched all delegators");

    let timestamp = chrono::Utc::now().timestamp();
    let mut updated_delegators_with_timestamp = delegators_with_timestamp.write().await;

    info!("Checking if delegators in file are up-to-date");
    if timestamp - updated_delegators_with_timestamp.timestamp < 1800
        && updated_delegators_with_timestamp.delegators == updated_delegators
    {
        info!("Delegators in file are up-to-date");
        return Ok(());
    }

    info!("Delegators in file are not up-to-date");
    updated_delegators_with_timestamp.timestamp = timestamp;
    updated_delegators_with_timestamp.delegators = updated_delegators;

    *validators_with_timestamp.write().await =
        ValidatorsWithTimestamp::from(&updated_delegators_with_timestamp.clone());
    drop(updated_delegators_with_timestamp);

    info!("Updated all delegators");

    update_delegators_cache(delegators_with_timestamp).await?;

    Ok(())
}

pub async fn update_delegators_by_validator_account_id(
    delegators_with_timestamp: &Arc<RwLock<DelegatorsWithTimestamp>>,
    validators_with_timestamp: &Arc<RwLock<ValidatorsWithTimestamp>>,
    validator_account_id: String,
) -> Result<()> {
    info!(
        "Updating delegators for validator: {}",
        validator_account_id
    );

    let validator_delegators = methods::get_delegators_by_validator_account_id(
        &JsonRpcClient::connect("https://beta.rpc.mainnet.near.org"),
        validator_account_id.clone(),
    )
    .await
    .context("Failed to get delegators by validator account id")?;

    let mut validators_with_timestamp = validators_with_timestamp.write().await;

    let timestamp = chrono::Utc::now().timestamp();
    validators_with_timestamp.timestamp = timestamp;
    validators_with_timestamp
        .validators
        .insert(validator_account_id.clone(), validator_delegators);

    let updated_delegators_with_timestamp =
        DelegatorsWithTimestamp::from(&validators_with_timestamp.clone());
    drop(validators_with_timestamp);

    *delegators_with_timestamp.write().await = updated_delegators_with_timestamp.clone();

    info!("Updated delegators for validator: {}", validator_account_id);

    update_delegators_cache(delegators_with_timestamp).await?;

    Ok(())
}
