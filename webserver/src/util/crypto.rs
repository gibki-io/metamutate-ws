use anyhow::Result;
use super::AuthRequest;
use ed25519_dalek::{PublicKey, Verifier, Signature};

pub fn verify_message (request: &AuthRequest, nonce: &str) -> Result<bool> {
    let signature_base58 = bs58::decode(request.signature).into_vec()?;
    let pubkey_base58 = bs58::decode(request.pubkey).into_vec()?;

    let public_key = PublicKey::from_bytes(&pubkey_base58)?;
    let signature = Signature::from_bytes(&signature_base58)?;

    match public_key.verify(nonce.as_bytes(), &signature) {
        Ok(_) => Ok(true),
        Err(e) => Err(anyhow::anyhow!(e))
    }
}
