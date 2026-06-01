//! `SeaORM` Entity for management operation audit logs.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "audit_logs")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = true)]
    pub id: i32,
    pub actor_user_id: Option<i32>,
    #[sea_orm(column_type = "custom(\"TINYTEXT\")")]
    pub actor_username: String,
    #[sea_orm(column_type = "custom(\"TINYTEXT\")")]
    pub actor_role: String,
    #[sea_orm(column_type = "custom(\"TINYTEXT\")")]
    pub action: String,
    #[sea_orm(column_type = "custom(\"TINYTEXT\")")]
    pub target_type: String,
    #[sea_orm(column_type = "Text")]
    pub target_id: String,
    #[sea_orm(column_type = "custom(\"TINYTEXT\")")]
    pub result: String,
    #[sea_orm(column_type = "custom(\"TINYTEXT\")")]
    pub ip: String,
    #[sea_orm(column_type = "Text")]
    pub user_agent: String,
    #[sea_orm(column_type = "Text")]
    pub detail: String,
    pub created_at: DateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
