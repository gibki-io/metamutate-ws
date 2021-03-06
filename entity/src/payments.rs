//! SeaORM Entity. Generated by sea-orm-codegen 0.7.0

use sea_orm::entity::prelude::*;
use serde::{Serialize, Deserialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "payments")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub account: String,
    pub amount: i32,
    #[sea_orm(column_type = "DateTime")]
    pub created_at: DateTime,
    pub success: bool,
    pub task_id: i32,
    pub tx: String,
}

#[derive(Copy, Clone, Debug, EnumIter)]
pub enum Relation {}

impl RelationTrait for Relation {
    fn def(&self) -> RelationDef {
        panic!("No RelationDef")
    }
}

impl ActiveModelBehavior for ActiveModel {}
