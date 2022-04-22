use super::metadata::{get_rank_attribute, verify_metadata, fetch_inner_metadata};
use rbatis::rbatis::Rbatis;
use anyhow::{anyhow, Result};
use solana_client::rpc_client::RpcClient;
use solana_sdk::signature::Signature;

pub async fn check_price(mint_address: &str, _db: &Rbatis) -> Result<i64> {
    let rpc: RpcClient = RpcClient::new("https://solport.genesysgo.net/");
    let metadata = match verify_metadata(&rpc, mint_address) {
        Ok(metadata) => metadata,
        Err(_e) => return Err(anyhow!("Failed to fetch metadata"))
    };

    let inner = match fetch_inner_metadata(metadata, mint_address).await {
        Ok(inner) => inner,
        Err(_e) => return Err(anyhow!("Failed to fetch uri metadata"))
    };

    let rank = get_rank_attribute(inner.attributes)?;

    let price: i64 = match rank.value.as_str() {
        "Academy" => 50,
        "Genin" => 100,
        "Chuunin" => 150,
        "Jounin" => 200,
        "Special Jonin" => 300,
        _ => return Err(anyhow!("Not a valid rank to use for rankup")),
    };

    Ok(price)
}

pub async fn confirm_transaction(signature: &Signature, _db: &Rbatis) -> Result<()> {
    let rpc = RpcClient::new("https://solport.genesysgo.net/");

    let confirmed = rpc.confirm_transaction(signature)?;

    loop {
        if confirmed {
            break
        }
    }
    Ok(())
}