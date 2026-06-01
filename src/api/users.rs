use crate::api::{map_db_err, require_admin, utc_now_millis, write_audit, AppState};
use crate::orm_entities::prelude::{AdminUser, AuditLog};
use crate::orm_entities::{admin_user, audit_log};
use actix_session::Session;
use actix_web::{get, post, web, HttpRequest, HttpResponse};
use argon2::password_hash::{rand_core::OsRng, SaltString};
use argon2::{Argon2, PasswordHasher};
use sea_orm::{
    ActiveModelTrait, EntityTrait, IntoActiveModel, NotSet, PaginatorTrait, QueryOrder, Set,
};
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
struct UserItem {
    id: i32,
    username: String,
    role: String,
    enabled: bool,
    created_at: i64,
    updated_at: i64,
    last_login_at: Option<i64>,
}

#[get("/api/admin/users")]
pub async fn list_users(
    session: Session,
    app_data: web::Data<AppState>,
) -> actix_web::Result<HttpResponse> {
    require_admin(&session, &app_data).await?;
    let users = AdminUser::find()
        .order_by_asc(admin_user::Column::Username)
        .all(app_data.db_pool.as_ref())
        .await
        .map_err(map_db_err)?
        .into_iter()
        .map(|user| UserItem {
            id: user.id,
            username: user.username,
            role: user.role,
            enabled: user.enabled,
            created_at: user.created_at.and_utc().timestamp(),
            updated_at: user.updated_at.and_utc().timestamp(),
            last_login_at: user.last_login_at.map(|time| time.and_utc().timestamp()),
        })
        .collect::<Vec<_>>();

    Ok(HttpResponse::Ok().json(serde_json::json!({ "items": users })))
}

#[derive(Deserialize)]
pub struct SaveUserRequest {
    id: Option<i32>,
    username: String,
    password: Option<String>,
    role: String,
    enabled: bool,
}

#[post("/api/admin/users/save")]
pub async fn save_user(
    req: HttpRequest,
    session: Session,
    app_data: web::Data<AppState>,
    json_data: web::Json<SaveUserRequest>,
) -> actix_web::Result<HttpResponse> {
    let actor = require_admin(&session, &app_data).await?;
    let role = if json_data.role == "admin" {
        "admin"
    } else {
        "user"
    };
    let now = utc_now_millis();

    if let Some(id) = json_data.id {
        let user = AdminUser::find_by_id(id)
            .one(app_data.db_pool.as_ref())
            .await
            .map_err(map_db_err)?
            .ok_or_else(|| actix_web::error::ErrorNotFound("user not found"))?;
        let mut model = user.into_active_model();
        model.username = Set(json_data.username.clone());
        model.role = Set(role.to_string());
        model.enabled = Set(json_data.enabled);
        model.updated_at = Set(now);
        if let Some(password) = &json_data.password {
            if !password.is_empty() {
                model.password_hash = Set(hash_password(password)?);
            }
        }
        model
            .update(app_data.db_pool.as_ref())
            .await
            .map_err(map_db_err)?;
        write_audit(
            &req,
            &app_data,
            Some(&actor),
            "save_user",
            "admin_user",
            &id.to_string(),
            "success",
            "",
        )
        .await;
    } else {
        let password = json_data
            .password
            .as_ref()
            .filter(|password| !password.is_empty())
            .ok_or_else(|| actix_web::error::ErrorBadRequest("password required"))?;
        let user = admin_user::ActiveModel {
            id: NotSet,
            username: Set(json_data.username.clone()),
            password_hash: Set(hash_password(password)?),
            role: Set(role.to_string()),
            enabled: Set(json_data.enabled),
            created_at: Set(now),
            updated_at: Set(now),
            last_login_at: Set(None),
        }
        .insert(app_data.db_pool.as_ref())
        .await
        .map_err(map_db_err)?;
        write_audit(
            &req,
            &app_data,
            Some(&actor),
            "create_user",
            "admin_user",
            &user.id.to_string(),
            "success",
            "",
        )
        .await;
    }

    Ok(HttpResponse::Ok().json(serde_json::json!({ "success": true })))
}

#[derive(Deserialize)]
pub struct AuditQuery {
    page: Option<u64>,
    page_size: Option<u64>,
}

#[get("/api/admin/audit_logs")]
pub async fn audit_logs(
    session: Session,
    app_data: web::Data<AppState>,
    query: web::Query<AuditQuery>,
) -> actix_web::Result<HttpResponse> {
    require_admin(&session, &app_data).await?;
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(50).clamp(1, 200);
    let paginator = AuditLog::find()
        .order_by_desc(audit_log::Column::CreatedAt)
        .paginate(app_data.db_pool.as_ref(), page_size);
    let total = paginator.num_items().await.map_err(map_db_err)?;
    let items = paginator
        .fetch_page(page - 1)
        .await
        .map_err(map_db_err)?
        .into_iter()
        .map(|item| {
            serde_json::json!({
                "id": item.id,
                "actor_user_id": item.actor_user_id,
                "actor_username": item.actor_username,
                "actor_role": item.actor_role,
                "action": item.action,
                "target_type": item.target_type,
                "target_id": item.target_id,
                "result": item.result,
                "ip": item.ip,
                "user_agent": item.user_agent,
                "detail": item.detail,
                "created_at": item.created_at.and_utc().timestamp(),
            })
        })
        .collect::<Vec<_>>();

    Ok(HttpResponse::Ok().json(serde_json::json!({
        "total": total,
        "items": items,
    })))
}

fn hash_password(password: &str) -> actix_web::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|err| {
            actix_web::error::ErrorInternalServerError(format!("failed to hash password: {err}"))
        })
}
