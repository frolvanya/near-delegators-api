use crate::extensions::{self, CallResultExt, RpcQueryResponseExt};

use color_eyre::{eyre::Context, Result};

use near_jsonrpc_client::JsonRpcClient;

use borsh::BorshDeserialize;

use futures::{stream::StreamExt, TryStreamExt};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
use tokio::sync::RwLock;

pub const LIMIT: usize = 500;

pub async fn get_receiver_id(
    beta_json_rpc_client: &JsonRpcClient,
    receipt_id: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    info!("Fetching receipt");

    let receipt_response = beta_json_rpc_client
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

pub async fn get_all_validators(beta_json_rpc_client: &JsonRpcClient) -> Result<BTreeSet<String>> {
    info!("Fetching all validators");

    let query_view_method_response = beta_json_rpc_client
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

async fn get_number_of_delegators(
    beta_json_rpc_client: &JsonRpcClient,
    block_reference: near_primitives::types::BlockReference,
    validator_account_id: String,
) -> Result<usize> {
    let delegators_response = beta_json_rpc_client
        .call(near_jsonrpc_client::methods::query::RpcQueryRequest {
            block_reference,
            request: near_primitives::views::QueryRequest::CallFunction {
                account_id: validator_account_id.parse()?,
                method_name: "get_number_of_accounts".to_string(),
                args: near_primitives::types::FunctionArgs::from(serde_json::to_vec(
                    &serde_json::json!(null),
                )?),
            },
        })
        .await;

    match delegators_response {
        Ok(response) => response
            .call_result()?
            .parse_result_from_json::<usize>()
            .context("Failed to parse delegators"),
        Err(near_jsonrpc_client::errors::JsonRpcError::ServerError(
            near_jsonrpc_client::errors::JsonRpcServerError::HandlerError(
                near_jsonrpc_client::methods::query::RpcQueryError::NoContractCode { .. }
                | near_jsonrpc_client::methods::query::RpcQueryError::ContractExecutionError {
                    ..
                },
            ),
        )) => Ok(0),
        Err(err) => Err(err.into()),
    }
}

pub async fn get_delegators_by_validator_account_id(
    beta_json_rpc_client: &JsonRpcClient,
    validator_account_id: String,
    block_reference: near_primitives::types::BlockReference,
) -> Result<BTreeSet<String>> {
    let number_of_delegators = get_number_of_delegators(
        beta_json_rpc_client,
        block_reference.clone(),
        validator_account_id.clone(),
    )
    .await?;

    let delegators = futures::stream::iter((0..number_of_delegators).step_by(LIMIT)).map(|from| {
        let block_reference = block_reference.clone();
        let validator_account_id = validator_account_id.clone();

        async move {
            let delegators_response = beta_json_rpc_client
                .call(near_jsonrpc_client::methods::query::RpcQueryRequest {
                    block_reference: block_reference.clone(),
                    request: near_primitives::views::QueryRequest::CallFunction {
                        account_id: validator_account_id.parse()?,
                        method_name: "get_accounts".to_string(),
                        args: near_primitives::types::FunctionArgs::from(serde_json::to_vec(
                            &serde_json::json!({
                                "from_index": from,
                                "limit": LIMIT,
                            }),
                        )?),
                    },
                })
                .await;

            match delegators_response {
                Ok(response) => response
                    .call_result()?
                    .parse_result_from_json::<BTreeSet<extensions::Delegator>>()
                    .map(|delegators| {
                        delegators
                            .into_iter()
                            .map(|delegator| delegator.account_id.to_string())
                            .collect::<BTreeSet<_>>()
                    })
                    .context("Failed to parse delegators"),
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
    })
        .buffer_unordered(50)
        .try_collect::<BTreeSet<_>>()
        .await?;

    Ok(delegators.into_iter().flatten().collect())
}

pub async fn get_all_delegators(
    beta_json_rpc_client: &JsonRpcClient,
) -> Result<BTreeMap<String, BTreeSet<String>>> {
    info!("Fetching all delegators");

    let validators = get_all_validators(beta_json_rpc_client).await?;
    let delegators = Arc::new(RwLock::new(BTreeMap::<String, BTreeSet<String>>::new()));

    info!("Fetching delegators for {} validators", validators.len());

    let mut handles = Vec::new();
    let block_reference = near_primitives::types::BlockReference::latest();

    for validator in validators {
        let delegators = delegators.clone();
        let beta_json_rpc_client = beta_json_rpc_client.clone();
        let block_reference = block_reference.clone();

        let handle = tokio::spawn(async move {
            let validator_delegators = get_delegators_by_validator_account_id(
                &beta_json_rpc_client,
                validator.clone(),
                block_reference,
            )
            .await?;

            let mut locked_delegators = delegators.write().await;
            for delegator in validator_delegators {
                locked_delegators
                    .entry(delegator.to_string())
                    .or_default()
                    .insert(validator.clone());
            }
            drop(locked_delegators);

            Ok::<_, color_eyre::eyre::Report>(())
        });

        handles.push(handle);
    }

    info!("Waiting for all delegators to be fetched");

    futures::future::try_join_all(handles).await?;

    let locked_delegators = delegators.read().await;
    Ok(locked_delegators.clone())
}
