use crate::extensions::{self, CallResultExt, RpcQueryResponseExt};

use color_eyre::{eyre::Context, Result};

use near_jsonrpc_client::JsonRpcClient;

use borsh::BorshDeserialize;

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
use tokio::sync::Mutex;

pub async fn get_receiver_id(
    receipt_id: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let json_rpc_client = JsonRpcClient::connect("https://rpc.mainnet.near.org");

    info!("Fetching receipt");
    let receipt_response = json_rpc_client
        .call(
            near_jsonrpc_client::methods::EXPERIMENTAL_receipt::RpcReceiptRequest {
                receipt_reference: near_jsonrpc_primitives::types::receipts::ReceiptReference {
                    receipt_id: receipt_id.parse()?,
                },
            },
        )
        .await
        .context("Failed to fetch receipt")?;

    Ok(receipt_response.receiver_id.to_string())
}

pub async fn get_all_validators() -> Result<BTreeSet<String>> {
    let json_rpc_client = JsonRpcClient::connect("https://beta.rpc.mainnet.near.org");

    info!("Fetching all validators");
    let query_view_method_response = json_rpc_client
        .call(near_jsonrpc_client::methods::query::RpcQueryRequest {
            block_reference: near_primitives::types::Finality::Final.into(),
            request: near_primitives::views::QueryRequest::ViewState {
                account_id: "poolv1.near".parse()?,
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

pub async fn get_delegators_by_validator_account_id(
    validator_account_id: String,
) -> Result<BTreeSet<String>> {
    let json_rpc_client = JsonRpcClient::connect("https://archival-rpc.mainnet.near.org");

    let delegators_response = json_rpc_client
        .call(near_jsonrpc_client::methods::query::RpcQueryRequest {
            block_reference: near_primitives::types::BlockReference::latest(),
            request: near_primitives::views::QueryRequest::CallFunction {
                account_id: validator_account_id.parse()?,
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
            .parse_result_from_json::<BTreeSet<extensions::Delegator>>()
            .map(|delegators| {
                delegators
                    .into_iter()
                    .map(|delegator| delegator.account_id.to_string())
                    .collect()
            })
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

pub async fn get_all_delegators() -> Result<BTreeMap<String, BTreeSet<String>>> {
    info!("Fetching all delegators");

    let validators = get_all_validators().await?;
    let delegators = Arc::new(Mutex::new(BTreeMap::<String, BTreeSet<String>>::new()));

    info!("Fetching delegators for {} validators", validators.len());

    let mut handles = Vec::new();
    for validator_account_id in validators {
        let delegators = delegators.clone();

        let handle = tokio::spawn(async move {
            let validator_delegators =
                get_delegators_by_validator_account_id(validator_account_id.clone()).await?;
            for delegator in validator_delegators {
                let mut locked_delegators = delegators.lock().await;
                locked_delegators
                    .entry(delegator.to_string())
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
    Ok(locked_delegators.clone())
}
