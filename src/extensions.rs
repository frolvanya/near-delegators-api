use color_eyre::{eyre::Context, Result};

#[easy_ext::ext(RpcQueryResponseExt)]
pub impl near_jsonrpc_primitives::types::query::RpcQueryResponse {
    fn call_result(&self) -> Result<near_primitives::views::CallResult> {
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
pub impl near_primitives::views::CallResult {
    fn parse_result_from_json<T>(&self) -> Result<T, color_eyre::eyre::Error>
    where
        T: for<'de> serde::Deserialize<'de>,
    {
        serde_json::from_slice(&self.result).context("Failed to parse view-function call result")
    }
}

#[derive(serde::Deserialize, Debug, Ord, PartialOrd, Eq, PartialEq)]
pub struct Delegator {
    pub account_id: near_primitives::types::AccountId,
}
