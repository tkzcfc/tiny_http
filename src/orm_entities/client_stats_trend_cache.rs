//! `SeaORM` Entity for client statistics trend cache.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "client_stats_trend_cache")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false, column_type = "Text")]
    pub granularity: String,
    #[sea_orm(primary_key, auto_increment = false, column_type = "Text")]
    pub bucket: String,
    #[sea_orm(primary_key, auto_increment = false, column_type = "Text")]
    pub cli_type: String,
    pub count: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
