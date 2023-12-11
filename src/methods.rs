use crate::extensions;

use color_eyre::{eyre::Context, Result};

use near_jsonrpc_client::JsonRpcClient;
use near_primitives::types::AccountId;

use std::{
    collections::{BTreeMap, HashSet},
    sync::Arc,
};
use tokio::sync::Mutex;

async fn get_validators(json_rpc_client: &JsonRpcClient) -> Result<Vec<AccountId>> {
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

async fn get_validator_delegators(
    json_rpc_client: &JsonRpcClient,
    validator_account_id: &AccountId,
) -> Result<Vec<extensions::Delegator>> {
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
        Ok(response) => Ok(extensions::CallResultExt::parse_result_from_json::<
            Vec<extensions::Delegator>,
        >(&extensions::RpcQueryResponseExt::call_result(&response)?)
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

pub async fn get_all_delegators() -> Result<BTreeMap<String, String>> {
    let json_rpc_client = JsonRpcClient::connect("https://rpc.mainnet.near.org");

    let mut checked_validators = HashSet::new();
    let validators = get_validators(&json_rpc_client).await?;
    let delegators = Arc::new(Mutex::new(BTreeMap::<AccountId, Vec<AccountId>>::new()));

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
                get_validator_delegators(&json_rpc_client, &validator_account_id).await?;
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

    // match serde_json::to_string_pretty(&*delegators.lock().await) {
    //     Ok(json_string) => println!("{json_string}"),
    //     Err(err) => color_eyre::eyre::bail!("Failed to serialize delegators: {}", err),
    // }

    // Ok(())
}
