use rbatis::crud::CRUD;
use rbatis::crud_table;
use rbatis::rbatis::Rbatis;

use std::collections::HashMap;

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

pub mod metadata;

#[crud_table(table_name:accounts)]
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct WalletAccount {
    pub pubkey: String,
    pub nonce: String,
    pub created_at: rbatis::DateUtc,
}

#[crud_table(table_name:tasks)]
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Task {
    pub id: rbatis::Uuid,
    pub account: String,
    pub created_at: rbatis::DateUtc,
    pub mint_address: String,
    pub price: i64,
    pub success: bool
}
#[crud_table(table_name:payments)]
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Payment {
    pub id: rbatis::Uuid,
    pub account: String,
    pub created_at: rbatis::DateUtc,
    pub task_id: String,
    pub amount: i64,
    pub success: bool,
    pub tx: String,
}

pub struct Database {
    pub db: Rbatis,
}

impl Database {
    pub async fn migrate(rb: &Rbatis) -> () {
        rb.link("sqlite://database.db")
            .await
            .expect("Failed to connect to DB");
        rb.exec("CREATE TABLE IF NOT EXISTS accounts(pubkey VARCHAR(60), nonce VARCHAR(60), created_at DATE, PRIMARY KEY(pubkey))", vec![])
            .await
            .expect("Failed to create table ACCOUNTS");
        rb.exec("CREATE TABLE IF NOT EXISTS tasks(id VARCHAR(60), account VARCHAR(60), success BOOLEAN, created_at DATE, mint_address VARCHAR(60), price INT, PRIMARY KEY(id))", vec![])
            .await
            .expect("Failed to create table TASKS");
        rb.exec("CREATE TABLE IF NOT EXISTS payments(id VARCHAR(60), account VARCHAR(60), success BOOLEAN, created_at DATE, tx VARCHAR(100), task_id VARCHAR(60), amount INT, PRIMARY KEY(id))", vec![])
            .await
            .expect("Failed to create table PAYMENTS");
    }
}

impl Payment {
    pub fn new(request: crate::util::PaymentCreate<'_>, price: i64) -> Payment {
        let new_payment = Payment {
            id: rbatis::Uuid::new(),
            account: request.account.to_string(),
            created_at: rbatis::DateUtc::now(),
            success: false,
            task_id: request.task_id.to_string(),
            amount: price,
            tx: "".to_string()
        };

        new_payment
    }

    pub async fn save(&self, db: &Rbatis) -> Result<()> {
        db.save(self, &[]).await?;

        Ok(())
    }

    pub async fn fetch_one_by_id(id: &str, db: &Rbatis) -> Result<Option<Payment>> {
        let result: Option<Payment> = db
            .fetch_by_column("id", id.to_string())
            .await?;

        Ok(result)
    }

    pub async fn fetch_by_account(account: &str, db: &Rbatis) -> Result<Vec<Payment>> {
        let result: Vec<Payment> = db
            .fetch_list_by_column("account", &[account.to_string()])
            .await?;

        Ok(result)
    }

    pub async fn confirm_payment(&self, db: &Rbatis) -> Result<()> {
        db.update_by_column("success", self).await?;

        Ok(())
    }
}

impl Task {
    pub fn new(request: crate::util::TaskCreate<'_>, price: i64) -> Task {
        let new_task = Task {
            id: rbatis::Uuid::new(),
            account: request.account.to_string(),
            mint_address: request.mint_address.to_string(),
            created_at: rbatis::DateUtc::now(),
            success: false,
            price,
        };

        new_task
    }

    pub async fn save(&self, db: &Rbatis) -> Result<()> {
        db.save(self, &[]).await?;

        Ok(())
    }

    pub async fn fetch_one_by_address(&self, db: &Rbatis) -> Result<Option<Task>> {
        let result: Option<Task> = db
            .fetch_by_column("mint_address", &self.mint_address)
            .await?;

        Ok(result)
    }

    pub async fn fetch_one_by_id(id: &str, db: &Rbatis) -> Result<Option<Task>> {
        let result: Option<Task> = db
            .fetch_by_column("id", id.to_string())
            .await?;

        Ok(result)
    }

    pub async fn fetch_by_account(account: &str, db: &Rbatis) -> Result<Vec<Task>> {
        let result: Vec<Task> = db
            .fetch_list_by_column("account", &[account.to_string()])
            .await?;

        Ok(result)
    }

    pub async fn update_task(&self, db: &Rbatis) -> Result<()> {
        let _result = db.update_by_column("success", self).await?;

        Ok(())
    }

    pub async fn delete_task(&self, db: &Rbatis) -> Result<()> {
        let _result = db
            .remove_by_column::<Task, _>("id", &self.id)
            .await?;

        Ok(())
    }
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

    pub async fn fetch(pubkey: &str, db: &Rbatis) -> Result<Option<WalletAccount>> {
        let result: Option<WalletAccount> = db.fetch_by_column("pubkey", pubkey).await?;

        Ok(result)
    }
}
