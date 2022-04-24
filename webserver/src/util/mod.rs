use anyhow::Result;
use chrono::Utc;
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use rocket::{
    http::Status,
    serde::{json::Json, Deserialize, Serialize},
};
use serde_json::Value;

pub mod crypto;

pub struct ApiKey<'r>(pub &'r str);

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub exp: usize,
}

#[derive(Debug)]
pub enum ApiKeyError {
    Missing,
    Invalid,
}

// Requests

#[derive(Deserialize)]
pub struct AuthRequest<'a> {
    pub pubkey: &'a str,
    pub signature: &'a str,
}

#[derive(Deserialize)]
pub struct TaskCreate<'a> {
    pub mint_address: &'a str,
    pub account: &'a str
}

#[derive(Deserialize)]
pub struct PaymentCreate<'a> {
    pub task_id: i32,
    pub account: &'a str
}

#[derive(Deserialize)]
pub struct PaymentReceive<'a> {
    pub payment_id: i32,
    pub tx_id: &'a str
}

// Responses

#[derive(Serialize)]
pub struct SysResponse {
    pub data: Value,
}

pub type WebResponse = (Status, Json<SysResponse>);

// Utility Functions

pub fn create_jwt(pubkey: &str, secret: &str) -> Result<String> {
    let expiration = Utc::now()
        .checked_add_signed(chrono::Duration::minutes(30))
        .expect("valid timestamp")
        .timestamp();

    let claims = Claims {
        sub: pubkey.to_owned(),
        exp: expiration as usize,
    };

    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_ref()),
    )?;
    Ok(token)
}

pub fn decode_jwt(token: &str, secret: &str) -> Result<()> {
    decode::<Claims>(token, &DecodingKey::from_secret(secret.as_ref()), &Validation::new(Algorithm::HS256))?;
    Ok(())
}