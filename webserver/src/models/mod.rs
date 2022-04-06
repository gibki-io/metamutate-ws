use rbatis::crud::CRUD;
use rbatis::rbatis::Rbatis;
use rbatis::crud_table;

use std::collections::HashMap;

use serde::{Serialize, Deserialize};
use anyhow::{Result, anyhow};

#[crud_table(table_name:accounts)]
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct WalletAccount {
    pub pubkey: String,
    pub nonce: String,
    pub created_at: rbatis::DateUtc
}

pub struct Database {
    pub db: Rbatis
}

impl WalletAccount {
    pub async fn save(&self, db: &Rbatis) -> Result<()> {
        let result: Option<WalletAccount> = db.fetch_by_column("pubkey", &self.pubkey).await?;
        if let Some(_account) = result {
            db.update_by_column("nonce", self).await?;
        } else {
            db.save(self, &[]).await?;
        }
        Ok(())
    }

    pub async fn lookup(pubkey: &str, db: &Rbatis) -> Result<Option<WalletAccount>> {
        let result: Option<WalletAccount> = db.fetch_by_column("pubkey", pubkey).await?;

        Ok(result)
    }
}