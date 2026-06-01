//! `SeaORM` Entity for database schema versions.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "schema_version")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub version: i32,
    #[sea_orm(column_type = "Text")]
    pub description: String,
    pub applied_at: DateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
