//! `SeaORM` Entity for raw upload user logs.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "upload_user_logs")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub upload_user_id: i32,
    #[sea_orm(column_type = "Text")]
    pub logs: String,
    pub created_at: DateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
