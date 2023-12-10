use std::{collections::BTreeMap, sync::Arc};

use color_eyre::{eyre::Context, Result};

use near_jsonrpc_client::JsonRpcClient;
use near_token::NearToken;
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

#[derive(serde::Deserialize, Debug)]
struct Delegator {
    account_id: near_primitives::types::AccountId,
    unstaked_balance: String,
    staked_balance: String,
}

async fn get_validators_stake(
    json_rpc_client: &JsonRpcClient,
) -> color_eyre::eyre::Result<
    std::collections::HashMap<near_primitives::types::AccountId, near_primitives::types::Balance>,
> {
    let epoch_validator_info = json_rpc_client
        .call(
            &near_jsonrpc_client::methods::validators::RpcValidatorRequest {
                epoch_reference: near_primitives::types::EpochReference::Latest,
            },
        )
        .await?;

    Ok(epoch_validator_info
        .current_proposals
        .into_iter()
        .map(|validator_stake_view| {
            let validator_stake = validator_stake_view.into_validator_stake();
            validator_stake.account_and_stake()
        })
        .chain(epoch_validator_info.current_validators.into_iter().map(
            |current_epoch_validator_info| {
                (
                    current_epoch_validator_info.account_id,
                    current_epoch_validator_info.stake,
                )
            },
        ))
        .chain(
            epoch_validator_info
                .next_validators
                .into_iter()
                .map(|next_epoch_validator_info| {
                    (
                        next_epoch_validator_info.account_id,
                        next_epoch_validator_info.stake,
                    )
                }),
        )
        .collect())
}

async fn get_delegators(
    json_rpc_client: &JsonRpcClient,
    validator_account_id: near_primitives::types::AccountId,
) -> Result<Vec<Delegator>> {
    let delegators_response = json_rpc_client
        .call(near_jsonrpc_client::methods::query::RpcQueryRequest {
            block_reference: near_primitives::types::BlockReference::latest(),
            request: near_primitives::views::QueryRequest::CallFunction {
                account_id: validator_account_id,
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

// I was trying to use .for_each_concurrent() but it was not working for me

// fn main() -> Result<()> {
//     let json_rpc_client = JsonRpcClient::connect("https://rpc.mainnet.near.org");
//     // let json_rpc_client = JsonRpcClient::connect("https://archival-rpc.mainnet.near.org");

//     let runtime = tokio::runtime::Builder::new_multi_thread()
//         .enable_all()
//         .build()?;
//     let concurrency = 10;

//     let validators = runtime.block_on(get_validators_stake(&json_rpc_client))?;

//     let delegators_staked_balance = Mutex::new(BTreeMap::new());

//     runtime.block_on(
//         futures::stream::iter(validators.into_keys())
//             .map(|validator_account_id| async {
//                 let delegators = get_delegators(&json_rpc_client, validator_account_id).await?;
//                 for delegator in delegators {
//                     let mut locked_balance = delegators_staked_balance.lock().await;
//                     let entry = locked_balance
//                         .entry(delegator.account_id)
//                         .or_insert_with(|| NearToken::from_yoctonear(0));

//                     *entry = entry.saturating_add(
//                         delegator
//                             .staked_balance
//                             .saturating_add(delegator.unstaked_balance),
//                     );
//                 }
//                 Ok::<_, color_eyre::eyre::Report>(())
//             })
//             .buffer_unordered(concurrency)
//             .collect::<Vec<_>>(),
//     );

//     println!("{:#?}", delegators_staked_balance);

//     // println!("{:#?}", delegated_stake);

//     Ok(())
// }

#[tokio::main]
async fn main() -> Result<()> {
    let json_rpc_client = JsonRpcClient::connect("https://rpc.mainnet.near.org");

    let validators = get_validators_stake(&json_rpc_client).await?;
    let mut handles = Vec::new();
    let delegators_staked_balance = Arc::new(Mutex::new(BTreeMap::new()));

    for validator_account_id in validators.into_keys() {
        let json_rpc_client = json_rpc_client.clone();
        let delegators_staked_balance = delegators_staked_balance.clone();

        let handle = tokio::spawn(async move {
            let delegators = get_delegators(&json_rpc_client, validator_account_id).await?;
            for delegator in delegators {
                let staked_balance = NearToken::from_yoctonear(
                    delegator
                        .staked_balance
                        .parse::<u128>()
                        .wrap_err("Failed to parse staked balance")?,
                );
                let unstaked_balance = NearToken::from_yoctonear(
                    delegator
                        .unstaked_balance
                        .parse::<u128>()
                        .wrap_err("Failed to parse unstaked balance")?,
                );

                let mut locked_balance = delegators_staked_balance.lock().await;
                locked_balance
                    .entry(delegator.account_id)
                    .and_modify(|balance: &mut NearToken| {
                        *balance =
                            balance.saturating_add(staked_balance.saturating_add(unstaked_balance));
                    })
                    .or_insert_with(|| staked_balance.saturating_add(unstaked_balance));
            }
            Ok::<_, color_eyre::eyre::Report>(())
        });

        handles.push(handle);
    }

    futures::future::try_join_all(handles).await?;

    println!("{delegators_staked_balance:#?}");

    Ok(())
}
