//! `SeaORM` Entity for upload log resolution history.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "upload_log_resolutions")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = true)]
    pub id: i32,
    pub upload_log_id: i32,
    #[sea_orm(column_type = "custom(\"TINYTEXT\")")]
    pub upload_log_hash: String,
    pub resolved_by_user_id: i32,
    #[sea_orm(column_type = "custom(\"TINYTEXT\")")]
    pub resolved_by_username: String,
    pub resolved_at: DateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
