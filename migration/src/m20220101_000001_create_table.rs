use entity::{accounts, payments, tasks};
use sea_schema::migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20220101_000001_create_table"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.create_table(
            Table::create()
                .table(accounts::Entity)
                .if_not_exists()
                .col(
                    ColumnDef::new(accounts::Column::Id)
                        .integer()
                        .not_null()
                        .auto_increment()
                        .primary_key(),
                )
                .col(ColumnDef::new(accounts::Column::Nonce).string().not_null())
                .col(ColumnDef::new(accounts::Column::Pubkey).string().not_null())
                .col(ColumnDef::new(accounts::Column::CreatedAt).date_time().not_null())
                .to_owned()
        )
        .await?;
    
        manager.create_table(
            Table::create()
                .table(payments::Entity)
                .if_not_exists()
                .col(
                    ColumnDef::new(payments::Column::Id)
                    .integer()
                    .not_null()
                    .auto_increment()
                    .primary_key()
                )
                .col(ColumnDef::new(payments::Column::Account).string().not_null())
                .col(ColumnDef::new(payments::Column::Amount).integer().not_null())
                .col(ColumnDef::new(payments::Column::CreatedAt).date_time().not_null())
                .col(ColumnDef::new(payments::Column::Success).boolean().not_null())
                .col(ColumnDef::new(payments::Column::TaskId).string().not_null())
                .col(ColumnDef::new(payments::Column::Tx).string().not_null())
                .to_owned()
        )
        .await?;

        manager.create_table(
            Table::create()
            .table(tasks::Entity)
            .if_not_exists()
            .col(
                ColumnDef::new(tasks::Column::Id)
                .integer()
                .not_null()
                .auto_increment()
                .primary_key()
            )
            .col(ColumnDef::new(tasks::Column::Account).string().not_null())
            .col(ColumnDef::new(tasks::Column::CreatedAt).date_time().not_null())
            .col(ColumnDef::new(tasks::Column::MintAddress).string().not_null())
            .col(ColumnDef::new(tasks::Column::Price).integer().not_null())
            .col(ColumnDef::new(tasks::Column::Success).boolean().not_null())
            .to_owned()
        )
        .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(
            Table::drop()
            .table(accounts::Entity)
            .to_owned()
        )
        .await?;

        manager.drop_table(
            Table::drop()
            .table(payments::Entity)
            .to_owned()
        )
        .await?;

        manager.drop_table(
            Table::drop()
            .table(tasks::Entity)
            .to_owned()
        ).await
    }
}
