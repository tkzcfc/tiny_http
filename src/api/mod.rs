use actix_web::{web, HttpRequest};
use actix_web_httpauth::extractors::basic::{BasicAuth, Config};
use sea_orm::DatabaseConnection;
use std::sync::Arc;
use tokio::sync::OnceCell;

pub mod log;
pub mod log_html;
pub mod statistics;
pub mod statistics_html;

#[derive(Clone)]
pub struct AppState {
    pub username: Arc<String>,
    pub password: Arc<String>,
    pub admin_account: Arc<String>,
    pub admin_password: Arc<String>,
    pub db_pool: Arc<OnceCell<DatabaseConnection>>,
}

pub fn map_db_err(err: sea_orm::DbErr) -> actix_web::Error {
    actix_web::error::ErrorInternalServerError(format!("sqlx error:{}", err.to_string()))
}

// 用户鉴权
pub async fn user_authentication(
    req: &HttpRequest,
    credentials: &BasicAuth,
    app_data: &web::Data<AppState>,
) -> actix_web::Result<bool> {
    let username = credentials.user_id();
    let password = credentials.password().unwrap_or_default();

    if !app_data.admin_account.is_empty() || !app_data.admin_password.is_empty() {
        // 验证用户名和密码
        if username == app_data.admin_account.as_str()
            && password == app_data.admin_password.as_str()
        {
            return Ok(true);
        };
    }

    if !app_data.username.is_empty() || !app_data.password.is_empty() {
        // 验证用户名和密码
        return if username == app_data.username.as_str() || password == app_data.password.as_str() {
            Ok(false)
        } else {
            let config = req.app_data::<Config>().cloned().unwrap_or_default();
            Err(actix_web_httpauth::extractors::AuthenticationError::from(config).into())
        };
    }

    return Ok(false);
}
