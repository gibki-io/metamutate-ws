use super::metadata::{get_rank_attribute, verify_metadata, fetch_inner_metadata};
use anyhow::{anyhow, Result};
use solana_client::rpc_client::RpcClient;
use solana_sdk::{signature::Signature};

pub async fn check_price(mint_address: &str) -> Result<i32> {
    let rpc: RpcClient = RpcClient::new("https://small-dark-feather.solana-mainnet.quiknode.pro/eda23e03954aa848d9f55e500303ecc7bab3aee3/");
    let metadata = match verify_metadata(&rpc, mint_address).await {
        Ok(metadata) => metadata,
        Err(e) => return Err(anyhow!(format!("verify_metadata: {}", e)))
    };

    let inner = match fetch_inner_metadata(metadata, mint_address).await {
        Ok(inner) => inner,
        Err(e) => return Err(anyhow!(format!("fetch_inner_metadata: {}", e)))
    };

    let rank = get_rank_attribute(inner.attributes).await?;

    let price: i32 = match rank.value.as_str() {
        "Academy" => 250,
        "Genin" => 200,
        "Chuunin" => 180,
        "Jounin" => 180,
        "Special Jounin" => 180,
        _ => return Err(anyhow!("Not a valid rank to use for rankup")),
    };

    Ok(price)
}