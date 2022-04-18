use crate::{
    models::{
        metadata::{MetadataAttribute, MetadataInner},
        Database,
    },
    util::{SysResponse, TaskCreate, WebResponse},
};
use anyhow::{anyhow, Result as AnyResult};
use mpl_token_metadata::state::Metadata;
use rand::Rng;
use reqwest::blocking::multipart;
use rocket::serde::json::Json;
use rocket::{http::Status, State};
use serde_json::{json, value::to_value, Value};
use solana_client::rpc_client::RpcClient;
use solana_sdk::signature::Keypair;
use std::fs::File;
use metaboss::update_metadata::update_uri;
use std::io::prelude::*;
use std::io::BufReader;

const VERIFIED_CREATOR: &str = "Bf2jdfoFrqVS2n6eDtzzmb8cbue7B1ibcZF4QCvruqav";

pub async fn handle_update<'a> (mint_account: &'a str) -> Result<(), WebResponse>{
    let rpc: RpcClient = RpcClient::new("url");

    let metadata = match verify_metadata(&rpc, mint_account) {
        Ok(metadata) => metadata,
        Err(_e) => {
            let data = json!({ "error": "NFT entered is not from the right collection" });
            let response = SysResponse { data };

            return Err((Status::Forbidden, Json(response)));
        }
    };

    let mut inner = match fetch_inner_metadata(metadata, mint_account).await {
        Ok(inner) => inner,
        Err(_e) => {
            let data = json!({ "error": "Failed to fetch metadata uri" });
            let response = SysResponse { data };

            return Err((Status::InternalServerError, Json(response)));
        }
    };

    let new_attributes = match rank_up(inner.attributes) {
        Ok(new_attributes) => new_attributes,
        Err(_e) => {
            let data = json!({ "error": "Failed to rank up NFT" });
            let response = SysResponse { data };

            return Err((Status::InternalServerError, Json(response)));
        }
    };

    inner.attributes = new_attributes;

    match save_metadata(inner, mint_account).await {
        Ok(_) => (),
        Err(_) => {
            let data = json!({ "error": "Failed to save NFT" });
            let response = SysResponse { data };

            return Err((Status::InternalServerError, Json(response)));
        }
    };

    // Upload Metadata to IPFS
    let ipfs = match upload_to_ipfs(mint_account) {
        Ok(response) => response,
        Err(_) => {
            let data = json!({ "error": "Failed to upload to IPFS" });
            let response = SysResponse { data };

            return Err((Status::InternalServerError, Json(response)));
        }
    };

    let _status = match ipfs["ok"].as_bool() {
        Some(ok) => {
            if ok == false {
                let data = json!({ "error": "Failed to upload to IPFS" });
                let response = SysResponse { data };

                return Err((Status::NotFound, Json(response)));
            }
        },
        None => {
            let data = json!({ "error": "Failed to upload to IPFS" });
            let response = SysResponse { data };

            return Err((Status::NotFound, Json(response)));
        }
    };

    let cid = match ipfs["value"]["cid"].as_str() {
        Some(cid) => cid,
        None => {
            let data = json!({ "error": "Failed to retrieve IPFS CID" });
            let response = SysResponse { data };

            return Err((Status::NotFound, Json(response)));
        }
    };

    // Upload Metadata to Metaplex
    let keys = match std::fs::read("./keys/kamakura.json") {
        Ok(keys) => keys,
        Err(_) => {
            let data = json!({ "error": "Failed to retrieve signing keys" });
            let response = SysResponse { data };

            return Err((Status::InternalServerError, Json(response)));
        }
    };

    let private_keys: &[u8] = &keys;

    let keypair = match Keypair::from_bytes(private_keys) {
        Ok(keypair) => keypair,
        Err(_) => {
            let data = json!({ "error": "Failed to retrieve signing keys" });
            let response = SysResponse { data };

            return Err((Status::InternalServerError, Json(response)));
        }
    };

    let mpl_uri = format!("https://nftstorage.link/ipfs/{}/{}.json", cid, mint_account);

    match update_uri(&rpc, &keypair, mint_account, mpl_uri.as_str()) {
        Ok(_) => (),
        Err(_) => {
            let data = json!({ "error": "Failed to upload metadata uri to Metaplex" });
            let response = SysResponse { data };

            return Err((Status::InternalServerError, Json(response)));
        }
    };

    Ok(())
}

pub fn verify_metadata(rpc: &RpcClient, mint_account: &str) -> AnyResult<Metadata> {
    let metadata = metaboss::decode::decode(&rpc, mint_account)?;
    let creators = metadata.data.creators.as_ref().unwrap();

    if creators[0].address.to_string() != VERIFIED_CREATOR.to_string()
    {
        return Err(anyhow!("Not the right collection"));
    }

    Ok(metadata)
}

pub fn get_rank_attribute(attributes: Vec<MetadataAttribute>) -> AnyResult<MetadataAttribute> {
    let mut rank_attribute = if let Some(rank) = attributes
        .into_iter()
        .find(|rank| rank.trait_type == "Rank".to_string())
    {
        return Ok(rank);
    } else {
        return Err(anyhow!("No rank attribute found in metadata"));
    };
}

pub fn rank_up(attributes: Vec<MetadataAttribute>) -> AnyResult<Vec<MetadataAttribute>> {
    let mut json_attributes = to_value(attributes)?;
    let current_rank = json_attributes[0]["value"].as_str().unwrap();

    let chance: u32 = rand::thread_rng().gen_range(1..100);
    let denominator: u32 = match current_rank {
        "Academy" => 20,
        "Genin" => 50,
        "Chuunin" => 70,
        "Jounin" => 80,
        "Special Jonin" => 90,
        _ => return Err(anyhow!("Not a valid rank to use for rankup")),
    };

    let new_rank = if chance >= denominator {
        match current_rank {
            "Academy" => "Genin",
            "Genin" => "Chuunin",
            "Chuunin" => "Jonin",
            "Jounin" => "Special Jonin",
            "Special Jonin" => "Kage",
            "Kage" => "Kage",
            _ => return Err(anyhow!("Not a valid rank to use for rankup")),
        }
    } else {
        current_rank
    };

    json_attributes[0]["value"] = json!(new_rank);
    let new_attributes: Vec<MetadataAttribute> = serde_json::from_value(json_attributes)?;

    Ok(new_attributes)
}

pub async fn fetch_inner_metadata(metadata: Metadata, mint_account: &str) -> AnyResult<MetadataInner> {
    let uri = metadata.data.uri;
    let inner_metadata = reqwest::get(uri).await?.json::<MetadataInner>().await?;

    let path = format!("./metadata/{}.json", mint_account);
    serde_json::to_writer(&File::create(path)?, &inner_metadata)?;

    Ok(inner_metadata)
}

pub async fn save_metadata(inner_metadata: MetadataInner, mint_account: &str) -> AnyResult<()> {
    let path = format!("./metadata/{}.json", mint_account);
    serde_json::to_writer(&File::create(path)?, &inner_metadata)?;

    Ok(())
}

pub fn upload_to_ipfs<'a> (mint_account: &'a str) -> AnyResult<Value> {
    let address = mint_account;
    let form = multipart::Form::new()
        .file(format!("{}.json", address), format!("./metadata/{}.json", address))?;
        
    let client = reqwest::blocking::Client::new();
    let response = client
        .post("https://api.nft.storage/upload")
        .multipart(form)
        .send()?
        .json::<Value>()?;
    Ok(response)
}
