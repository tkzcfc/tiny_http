use crate::orm_entities::prelude::AdminUser;
use crate::orm_entities::{admin_user, audit_log};
use actix_session::Session;
use actix_web::{web, HttpRequest};
use chrono::{NaiveDateTime, Utc};
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const MAX_JSON_BYTES: usize = 256 * 1024;
const LOGIN_FAIL_LIMIT: u32 = 5;
const LOGIN_BLOCK_DURATION: Duration = Duration::from_secs(15 * 60);

pub mod auth;
pub mod log;
pub mod query_ip;
pub mod statistics;
pub mod users;

const SESSION_USER_ID: &str = "user_id";

#[derive(Clone)]
pub struct AppState {
    pub db_pool: Arc<DatabaseConnection>,
    pub login_guard: Arc<LoginGuard>,
}

#[derive(Default)]
pub struct LoginGuard {
    inner: Mutex<HashMap<String, LoginFailState>>,
}

#[derive(Default)]
struct LoginFailState {
    failures: u32,
    blocked_until: Option<Instant>,
}

impl LoginGuard {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock_map(&self) -> std::sync::MutexGuard<'_, HashMap<String, LoginFailState>> {
        self.inner.lock().unwrap_or_else(|err| err.into_inner())
    }

    pub fn is_blocked(&self, ip: &str) -> bool {
        let mut map = self.lock_map();
        let Some(state) = map.get(ip) else {
            return false;
        };
        if let Some(until) = state.blocked_until {
            if Instant::now() < until {
                return true;
            }
            map.remove(ip);
        }
        false
    }

    /// Records a failed login. Returns true if the IP is now blocked.
    pub fn record_failure(&self, ip: &str) -> bool {
        let mut map = self.lock_map();
        let state = map.entry(ip.to_string()).or_default();
        if let Some(until) = state.blocked_until {
            if Instant::now() < until {
                return true;
            }
            state.blocked_until = None;
            state.failures = 0;
        }
        state.failures = state.failures.saturating_add(1);
        if state.failures >= LOGIN_FAIL_LIMIT {
            state.blocked_until = Some(Instant::now() + LOGIN_BLOCK_DURATION);
            state.failures = 0;
            true
        } else {
            false
        }
    }

    pub fn clear(&self, ip: &str) {
        self.lock_map().remove(ip);
    }
}

pub fn client_ip(req: &HttpRequest) -> String {
    req.peer_addr()
        .map(|addr| addr.ip().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

pub fn map_db_err(err: sea_orm::DbErr) -> actix_web::Error {
    actix_web::error::ErrorInternalServerError(format!("database error:{}", err))
}

pub fn utc_now_millis() -> NaiveDateTime {
    chrono::DateTime::from_timestamp_millis(Utc::now().timestamp_millis())
        .unwrap_or_else(Utc::now)
        .naive_utc()
}

#[derive(Clone, Debug, Serialize)]
pub struct AuthUser {
    pub id: i32,
    pub username: String,
    pub role: String,
}

impl AuthUser {
    pub fn is_admin(&self) -> bool {
        self.role == "admin"
    }
}

pub fn set_session_user(session: &Session, user_id: i32) -> actix_web::Result<()> {
    session.insert(SESSION_USER_ID, user_id).map_err(|err| {
        actix_web::error::ErrorInternalServerError(format!("failed to write session: {err}"))
    })
}

pub fn clear_session_user(session: &Session) {
    session.remove(SESSION_USER_ID);
}

pub async fn require_user(
    session: &Session,
    app_data: &web::Data<AppState>,
) -> actix_web::Result<AuthUser> {
    let user_id = session
        .get::<i32>(SESSION_USER_ID)
        .map_err(|err| {
            actix_web::error::ErrorInternalServerError(format!("failed to read session: {err}"))
        })?
        .ok_or_else(|| actix_web::error::ErrorUnauthorized("login required"))?;

    let user = AdminUser::find_by_id(user_id)
        .one(app_data.db_pool.as_ref())
        .await
        .map_err(map_db_err)?
        .filter(|user| user.enabled)
        .ok_or_else(|| actix_web::error::ErrorUnauthorized("login required"))?;

    Ok(AuthUser {
        id: user.id,
        username: user.username,
        role: user.role,
    })
}

pub async fn require_admin(
    session: &Session,
    app_data: &web::Data<AppState>,
) -> actix_web::Result<AuthUser> {
    let user = require_user(session, app_data).await?;
    if user.is_admin() {
        Ok(user)
    } else {
        Err(actix_web::error::ErrorForbidden("admin role required"))
    }
}

pub async fn find_user_by_username(
    db: &DatabaseConnection,
    username: &str,
) -> Result<Option<admin_user::Model>, sea_orm::DbErr> {
    AdminUser::find()
        .filter(admin_user::Column::Username.eq(username))
        .one(db)
        .await
}

#[allow(clippy::too_many_arguments)]
pub async fn write_audit(
    req: &HttpRequest,
    app_data: &web::Data<AppState>,
    actor: Option<&AuthUser>,
    action: &str,
    target_type: &str,
    target_id: &str,
    result: &str,
    detail: impl Into<String>,
) {
    let ip = client_ip(req);
    let user_agent = req
        .headers()
        .get("user-agent")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_string();
    let (actor_user_id, actor_username, actor_role) = if let Some(actor) = actor {
        (Some(actor.id), actor.username.clone(), actor.role.clone())
    } else {
        (None, "".to_string(), "".to_string())
    };

    let model = audit_log::ActiveModel {
        id: sea_orm::NotSet,
        actor_user_id: Set(actor_user_id),
        actor_username: Set(actor_username),
        actor_role: Set(actor_role),
        action: Set(action.to_string()),
        target_type: Set(target_type.to_string()),
        target_id: Set(target_id.to_string()),
        result: Set(result.to_string()),
        ip: Set(ip),
        user_agent: Set(user_agent),
        detail: Set(detail.into()),
        created_at: Set(utc_now_millis()),
    };

    if let Err(err) = model.insert(app_data.db_pool.as_ref()).await {
        tracing::warn!(%err, action, target_type, target_id, "failed to write audit log");
    }
}

#[cfg(test)]
mod tests {
    use super::LoginGuard;
    use std::net::SocketAddr;

    #[test]
    fn blocks_after_five_failures() {
        let guard = LoginGuard::new();
        let ip = "203.0.113.10";
        for _ in 0..4 {
            assert!(!guard.record_failure(ip));
            assert!(!guard.is_blocked(ip));
        }
        assert!(guard.record_failure(ip));
        assert!(guard.is_blocked(ip));
    }

    #[test]
    fn success_clears_failures() {
        let guard = LoginGuard::new();
        let ip = "203.0.113.11";
        for _ in 0..4 {
            assert!(!guard.record_failure(ip));
        }
        guard.clear(ip);
        assert!(!guard.is_blocked(ip));
        assert!(!guard.record_failure(ip));
        assert!(!guard.is_blocked(ip));
    }

    #[test]
    fn client_ip_uses_peer_address() {
        let addr: SocketAddr = "198.51.100.7:4444".parse().unwrap();
        assert_eq!(addr.ip().to_string(), "198.51.100.7");
    }
}
