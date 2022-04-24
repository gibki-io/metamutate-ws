use crate::{
    models::{
        metadata::{MetadataAttribute, MetadataInner},
    },
};
use anyhow::{anyhow, Result as AnyResult};
use mpl_token_metadata::state::Metadata;
use rand::Rng;
use reqwest::blocking::multipart;
use serde_json::{json, value::to_value, Value};
use solana_client::rpc_client::RpcClient;
use solana_sdk::signature::Keypair;
use std::fs::File;
use metaboss::update_metadata::update_uri;
use tokio::task;

const VERIFIED_CREATOR: &str = "Bf2jdfoFrqVS2n6eDtzzmb8cbue7B1ibcZF4QCvruqav";

pub async fn handle_update(mint_account: &'_ str) -> AnyResult<bool>{
    let rpc = RpcClient::new_with_commitment(
        "https://solport.genesysgo.net/", 
        solana_sdk::commitment_config::CommitmentConfig::confirmed()
    );

    let rpc2 = RpcClient::new_with_commitment(
        "https://solport.genesysgo.net/", 
        solana_sdk::commitment_config::CommitmentConfig::confirmed()
    );

    let mint_verify = mint_account.to_owned().clone();
    let mint_upload = mint_account.to_owned().clone();

    let metadata = verify_metadata(&rpc, &mint_verify).await?;
    let mut inner = fetch_inner_metadata(metadata, mint_account).await?;
    let rankup_result = rank_up(inner.attributes).await?;
    let (new_attributes, successful) = match rankup_result {
        (attributes, successful) => (attributes, successful)
    };

    inner.attributes = new_attributes;

    save_metadata(inner, mint_account).await?;

    // Upload Metadata to IPFS
    let ipfs = upload_to_ipfs(mint_account).await?;
    println!("{}", ipfs);

    let _status = match ipfs["ok"].as_bool() {
        Some(ok) => {
            if !ok {
                return Err(anyhow::anyhow!("IPFS upload unsuccessful"));
            }
        },
        None => {
            return Err(anyhow::anyhow!("IPFS upload status missing"))
        }
    };

    let cid = match ipfs["value"]["cid"].as_str() {
        Some(cid) => cid,
        None => {
            return Err(anyhow::anyhow!("IPFS cid is missing"));
        }
    };

    // Upload Metadata to Metaplex
    let raw_keys = tokio::fs::read("./keys/kamakura.json").await?;
    let b58_keys = String::from_utf8_lossy(&raw_keys);
    let keys = bs58::decode(b58_keys.as_ref()).into_vec()?;

    let keypair = match Keypair::from_bytes(&keys) {
        Ok(keypair) => keypair,
        Err(_) => {
            return Err(anyhow::anyhow!("Failed to parse authority keys"))
        }
    };

    let mpl_uri = format!("https://{}.ipfs.nftstorage.link", cid);

    task::spawn_blocking(move || update_uri(&rpc2, &keypair, &mint_upload, mpl_uri.as_str())).await??;
    
    Ok(successful)
}

pub async fn verify_metadata(rpc: &RpcClient, mint_account: &str) -> AnyResult<Metadata> {
    let metadata = metaboss::decode::decode(rpc, mint_account)?;
    let creators = metadata.data.creators.as_ref().unwrap();

    if creators[0].address.to_string() != *VERIFIED_CREATOR
    {
        return Err(anyhow!("Not the right collection"));
    }

    Ok(metadata)
}

pub async fn get_rank_attribute(attributes: Vec<MetadataAttribute>) -> AnyResult<MetadataAttribute> {
    let mut _rank_attribute = if let Some(rank) = attributes
        .into_iter()
        .find(|rank| rank.trait_type == *"Rank")
    {
        return Ok(rank);
    } else {
        return Err(anyhow!("No rank attribute found in metadata"));
    };
}

pub async fn rank_up(attributes: Vec<MetadataAttribute>) -> AnyResult<(Vec<MetadataAttribute>, bool)> {
    let mut json_attributes = to_value(attributes)?;
    let current_rank = json_attributes[0]["value"].as_str().unwrap();

    let chance: u32 = rand::thread_rng().gen_range(1..100);
    let denominator: u32 = match current_rank {
        "Academy" => 20,
        "Genin" => 50,
        "Chuunin" => 70,
        "Jounin" => 80,
        "Special Jounin" => 90,
        _ => return Err(anyhow!("Not a valid rank to use for rankup")),
    };

    let successful = if chance >= denominator {
        true
    } else {
        false
    };

    let new_rank = if chance >= denominator {
        match current_rank {
            "Academy" => "Genin",
            "Genin" => "Chuunin",
            "Chuunin" => "Jonin",
            "Jounin" => "Special Jounin",
            "Special Jounin" => "Kage",
            "Kage" => "Kage",
            _ => return Err(anyhow!("Not a valid rank to use for rankup")),
        }
    } else {
        current_rank
    };

    json_attributes[0]["value"] = json!(new_rank);
    let new_attributes: Vec<MetadataAttribute> = serde_json::from_value(json_attributes)?;

    Ok((new_attributes, successful))
}

pub async fn fetch_inner_metadata(metadata: Metadata, mint_account: &str) -> AnyResult<MetadataInner> {
    let uri = metadata.data.uri;
    let inner_metadata = reqwest::get(uri).await?.json::<MetadataInner>().await?;
    let im = inner_metadata.clone();

    let path = format!("./metadata/{}.json", mint_account);
    let writer = task::spawn_blocking(move || File::create(path)).await??;
    task::spawn_blocking(move || serde_json::to_writer(&writer, &inner_metadata)).await??;

    Ok(im)
}

pub async fn save_metadata(inner_metadata: MetadataInner, mint_account: &str) -> AnyResult<()> {
    let path = format!("./metadata/{}.json", mint_account);
    let writer = task::spawn_blocking(move || File::create(path)).await??;
    task::spawn_blocking(move || serde_json::to_writer(&writer, &inner_metadata)).await??;

    Ok(())
}

pub async fn upload_to_ipfs(mint_account: &'_ str) -> AnyResult<Value> {
    let mint_account = mint_account.to_owned();
    let response = task::spawn_blocking(move || {
        let address = mint_account;

        let path = format!("./metadata/{}.json", address);
        let file = std::fs::File::open(path)?;
            
        let client = reqwest::blocking::Client::new();
        let response = client
            .post("https://api.nft.storage/upload")
            .header("Authorization", format!("Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiJkaWQ6ZXRocjoweDc2ODgzMkQ0MWRhNjNjOWZlQkFCRjAwNWU5ODlENWU4MEFGOTJFRDEiLCJpc3MiOiJuZnQtc3RvcmFnZSIsImlhdCI6MTY1MDAxODU3Njc3NSwibmFtZSI6IkthbWFrdXJhIn0.SfItpLGzaCQmLKCNLXJ_u8cwGPk41Eo_bgj5c8rZVNQ"))
            .body(file)
            .send();

        let return_value = match response {
            Ok(sent) => sent,
            Err(e) => return Err(anyhow::anyhow!(e))
        };

        let blocking_done = match return_value.json::<Value>() {
            Ok(value) => value,
            Err(e) => return Err(anyhow::anyhow!(e))
        };

        Ok(blocking_done)
    }).await??;

    Ok(response)
    
}
