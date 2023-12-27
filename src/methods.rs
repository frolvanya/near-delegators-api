use crate::extensions::{self, CallResultExt, RpcQueryResponseExt};

use color_eyre::{eyre::Context, Result};

use near_jsonrpc_client::JsonRpcClient;
use near_primitives::types::AccountId;

use borsh::BorshDeserialize;

// use futures::{StreamExt, TryStreamExt};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
use tokio::sync::Mutex;

pub async fn get_validators() -> Result<BTreeSet<AccountId>> {
    let json_rpc_client = JsonRpcClient::connect("https://beta.rpc.mainnet.near.org");

    info!("Fetching all validators");
    let query_view_method_response = json_rpc_client
        .call(near_jsonrpc_client::methods::query::RpcQueryRequest {
            block_reference: near_primitives::types::Finality::Final.into(),
            request: near_primitives::views::QueryRequest::ViewState {
                account_id: "poolv1.near".parse::<AccountId>()?,
                prefix: near_primitives::types::StoreKey::from(Vec::new()),
                include_proof: false,
            },
        })
        .await
        .context("Failed to fetch query ViewState for <poolv1.near> on network <beta-rpc>")?;
    if let near_jsonrpc_primitives::types::query::QueryResponseKind::ViewState(result) =
        query_view_method_response.kind
    {
        info!("Parsing validators");

        Ok(result
            .values
            .iter()
            .filter_map(|item| {
                if &item.key[..2] == b"se" {
                    String::try_from_slice(&item.value)
                        .ok()
                        .and_then(|result| result.parse().ok())
                } else {
                    None
                }
            })
            .collect())
    } else {
        error!("Failed to parse validators");

        Err(color_eyre::Report::msg("Error call result".to_string()))
    }
}

async fn get_validator_delegators(
    json_rpc_client: &JsonRpcClient,
    validator_account_id: &AccountId,
) -> Result<BTreeSet<extensions::Delegator>> {
    let delegators_response = json_rpc_client
        .call(near_jsonrpc_client::methods::query::RpcQueryRequest {
            block_reference: near_primitives::types::BlockReference::latest(),
            request: near_primitives::views::QueryRequest::CallFunction {
                account_id: validator_account_id.clone(),
                method_name: "get_accounts".to_string(),
                args: near_primitives::types::FunctionArgs::from(serde_json::to_vec(
                    &serde_json::json!({
                        "from_index": 0,
                        "limit": std::u64::MAX,
                    }),
                )?),
            },
        })
        .await;

    match delegators_response {
        Ok(response) => Ok(response
            .call_result()?
            .parse_result_from_json()
            .context("Failed to parse delegators")?),
        Err(near_jsonrpc_client::errors::JsonRpcError::ServerError(
            near_jsonrpc_client::errors::JsonRpcServerError::HandlerError(
                near_jsonrpc_client::methods::query::RpcQueryError::NoContractCode { .. }
                | near_jsonrpc_client::methods::query::RpcQueryError::ContractExecutionError {
                    ..
                },
            ),
        )) => Ok(BTreeSet::new()),
        Err(err) => Err(err.into()),
    }
}

pub async fn get_all_delegators() -> Result<BTreeMap<String, String>> {
    info!("Fetching all delegators");

    let json_rpc_client = JsonRpcClient::connect("https://archival-rpc.mainnet.near.org");

    let validators = get_validators().await?;
    let delegators = Arc::new(Mutex::new(BTreeMap::<AccountId, BTreeSet<AccountId>>::new()));

    info!("Fetching delegators for {} validators", validators.len());

    let mut handles = Vec::new();
    for validator_account_id in validators {
        let json_rpc_client = json_rpc_client.clone();
        let delegators = delegators.clone();

        let handle = tokio::spawn(async move {
            let validator_delegators =
                get_validator_delegators(&json_rpc_client, &validator_account_id).await?;
            for delegator in validator_delegators {
                let mut locked_delegators = delegators.lock().await;
                locked_delegators
                    .entry(delegator.account_id.clone())
                    .or_default()
                    .insert(validator_account_id.clone());
            }
            Ok::<_, color_eyre::eyre::Report>(())
        });

        handles.push(handle);
    }

    info!("Waiting for all delegators to be fetched");
    futures::future::try_join_all(handles).await?;

    let locked_delegators = delegators.lock().await;

    Ok(locked_delegators
        .iter()
        .map(|(k, v)| {
            (
                k.to_string(),
                v.iter()
                    .map(std::string::ToString::to_string)
                    .collect::<Vec<String>>()
                    .join(","),
            )
        })
        .collect::<BTreeMap<String, String>>())
}
