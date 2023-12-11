use color_eyre::{eyre::Context, Result};

use near_jsonrpc_client::JsonRpcClient;

use std::{
    collections::{BTreeMap, HashSet},
    sync::Arc,
};
use tokio::sync::Mutex;

#[easy_ext::ext(RpcQueryResponseExt)]
impl near_jsonrpc_primitives::types::query::RpcQueryResponse {
    fn call_result(&self) -> color_eyre::eyre::Result<near_primitives::views::CallResult> {
        if let near_jsonrpc_primitives::types::query::QueryResponseKind::CallResult(result) =
            &self.kind
        {
            Ok(result.clone())
        } else {
            color_eyre::eyre::bail!(
                "Internal error: Received unexpected query kind in response to a view-function query call",
            );
        }
    }
}

#[easy_ext::ext(CallResultExt)]
impl near_primitives::views::CallResult {
    fn parse_result_from_json<T>(&self) -> Result<T, color_eyre::eyre::Error>
    where
        T: for<'de> serde::Deserialize<'de>,
    {
        serde_json::from_slice(&self.result).wrap_err_with(|| {
            format!(
                "Failed to parse view-function call return value: {}",
                String::from_utf8_lossy(&self.result)
            )
        })
    }
}

#[derive(serde::Deserialize, Debug, PartialOrd, PartialEq)]
struct Delegator {
    account_id: near_primitives::types::AccountId,
}

async fn get_validators(
    json_rpc_client: &JsonRpcClient,
) -> color_eyre::eyre::Result<Vec<near_primitives::types::AccountId>> {
    let epoch_validator_info = json_rpc_client
        .call(
            &near_jsonrpc_client::methods::validators::RpcValidatorRequest {
                epoch_reference: near_primitives::types::EpochReference::Latest,
            },
        )
        .await?;

    Ok(epoch_validator_info
        .current_validators
        .into_iter()
        .map(|validator| validator.account_id)
        .chain(
            epoch_validator_info
                .next_validators
                .into_iter()
                .map(|validator| validator.account_id),
        )
        .chain(
            epoch_validator_info.current_fishermen.into_iter().map(
                near_primitives::views::validator_stake_view::ValidatorStakeView::take_account_id,
            ),
        )
        .chain(
            epoch_validator_info.next_fishermen.into_iter().map(
                near_primitives::views::validator_stake_view::ValidatorStakeView::take_account_id,
            ),
        )
        .chain(
            epoch_validator_info.current_proposals.into_iter().map(
                near_primitives::views::validator_stake_view::ValidatorStakeView::take_account_id,
            ),
        )
        .chain(
            epoch_validator_info
                .prev_epoch_kickout
                .into_iter()
                .map(|validator| validator.account_id),
        )
        .collect())
}

async fn get_delegators(
    json_rpc_client: &JsonRpcClient,
    validator_account_id: &near_primitives::types::AccountId,
) -> Result<Vec<Delegator>> {
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
            .parse_result_from_json::<Vec<Delegator>>()
            .wrap_err("Failed to parse return value of view function call for Vec<Delegator>.")?),
        Err(near_jsonrpc_client::errors::JsonRpcError::ServerError(
            near_jsonrpc_client::errors::JsonRpcServerError::HandlerError(
                near_jsonrpc_client::methods::query::RpcQueryError::NoContractCode { .. }
                | near_jsonrpc_client::methods::query::RpcQueryError::ContractExecutionError {
                    ..
                },
            ),
        )) => Ok(Vec::new()),
        Err(err) => Err(err.into()),
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let json_rpc_client = JsonRpcClient::connect("https://archival-rpc.mainnet.near.org");

    let mut checked_validators = HashSet::new();
    let validators = get_validators(&json_rpc_client).await?;
    let delegators = Arc::new(Mutex::new(BTreeMap::<
        near_primitives::types::AccountId,
        Vec<near_primitives::types::AccountId>,
    >::new()));

    let mut handles = Vec::new();
    for validator_account_id in validators {
        if checked_validators.contains(&validator_account_id) {
            continue;
        }
        checked_validators.insert(validator_account_id.clone());

        let json_rpc_client = json_rpc_client.clone();
        let delegators = delegators.clone();

        let handle = tokio::spawn(async move {
            let validator_delegators =
                get_delegators(&json_rpc_client, &validator_account_id).await?;
            for delegator in validator_delegators {
                let mut locked_delegators = delegators.lock().await;
                locked_delegators
                    .entry(delegator.account_id)
                    .or_default()
                    .push(validator_account_id.clone());
            }
            Ok::<_, color_eyre::eyre::Report>(())
        });

        handles.push(handle);
    }

    futures::future::try_join_all(handles).await?;

    match serde_json::to_string_pretty(&*delegators.lock().await) {
        Ok(json_string) => println!("{json_string}"),
        Err(err) => color_eyre::eyre::bail!("Failed to serialize delegators: {}", err),
    }

    Ok(())
}
