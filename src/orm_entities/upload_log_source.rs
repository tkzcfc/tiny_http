//! `SeaORM` Entity for normalized upload log sources.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "upload_log_sources")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = true)]
    pub id: i32,
    pub upload_log_id: i32,
    #[sea_orm(column_type = "Text")]
    pub upload_log_hash: String,
    pub upload_user_id: i32,
    #[sea_orm(column_type = "Text")]
    pub log_type: String,
    #[sea_orm(column_type = "Text")]
    pub package: String,
    #[sea_orm(column_type = "Text")]
    pub nav_url: String,
    #[sea_orm(column_type = "Text")]
    pub version: String,
    #[sea_orm(column_type = "Text")]
    pub user: String,
    #[sea_orm(column_type = "Text")]
    pub ip: String,
    pub reported_at: DateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
