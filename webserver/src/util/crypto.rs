use ed25519_dalek::{PublicKey, Verifier, Signature};
use anyhow::Result;
use super::AuthRequest;

pub fn verify_message (request: &AuthRequest, nonce: &str) -> Result<bool> {
    let publickey = PublicKey::from_bytes(&request.pubkey.as_bytes())?;
    let nonce: &[u8] = nonce.as_bytes();
    let signature = Signature::from_bytes(&request.signature.as_bytes())?;
    publickey.verify(nonce, &signature)?;

    Ok(true)
}
