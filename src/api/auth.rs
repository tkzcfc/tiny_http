use crate::api::{
    clear_session_user, find_user_by_username, map_db_err, require_user, set_session_user,
    utc_now_millis, write_audit, AppState,
};
use crate::orm_entities::admin_user;
use actix_session::Session;
use actix_web::{get, post, web, HttpRequest, HttpResponse};
use argon2::password_hash::{PasswordHash, PasswordVerifier};
use argon2::Argon2;
use sea_orm::{ActiveModelTrait, IntoActiveModel, Set};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct LoginRequest {
    username: String,
    password: String,
}

#[derive(Serialize)]
struct MeResponse {
    authenticated: bool,
    id: Option<i32>,
    username: Option<String>,
    role: Option<String>,
    is_admin: bool,
}

#[post("/api/auth/login")]
pub async fn login(
    req: HttpRequest,
    session: Session,
    app_data: web::Data<AppState>,
    json_data: web::Json<LoginRequest>,
) -> actix_web::Result<HttpResponse> {
    let Some(user) = find_user_by_username(app_data.db_pool.as_ref(), &json_data.username)
        .await
        .map_err(map_db_err)?
    else {
        write_audit(
            &req,
            &app_data,
            None,
            "login",
            "admin_user",
            &json_data.username,
            "failed",
            "user not found",
        )
        .await;
        return Err(actix_web::error::ErrorUnauthorized("invalid credentials"));
    };

    if !user.enabled {
        write_audit(
            &req,
            &app_data,
            None,
            "login",
            "admin_user",
            &json_data.username,
            "failed",
            "user disabled",
        )
        .await;
        return Err(actix_web::error::ErrorUnauthorized("invalid credentials"));
    }

    let parsed_hash = PasswordHash::new(&user.password_hash).map_err(|err| {
        actix_web::error::ErrorInternalServerError(format!("invalid password hash: {err}"))
    })?;
    if Argon2::default()
        .verify_password(json_data.password.as_bytes(), &parsed_hash)
        .is_err()
    {
        write_audit(
            &req,
            &app_data,
            None,
            "login",
            "admin_user",
            &json_data.username,
            "failed",
            "bad password",
        )
        .await;
        return Err(actix_web::error::ErrorUnauthorized("invalid credentials"));
    }

    set_session_user(&session, user.id)?;
    let mut model: admin_user::ActiveModel = user.clone().into_active_model();
    let now = utc_now_millis();
    model.last_login_at = Set(Some(now));
    model.updated_at = Set(now);
    model
        .update(app_data.db_pool.as_ref())
        .await
        .map_err(map_db_err)?;

    let actor = crate::api::AuthUser {
        id: user.id,
        username: user.username.clone(),
        role: user.role.clone(),
    };
    write_audit(
        &req,
        &app_data,
        Some(&actor),
        "login",
        "admin_user",
        &user.id.to_string(),
        "success",
        "",
    )
    .await;

    Ok(HttpResponse::Ok().json(MeResponse {
        authenticated: true,
        id: Some(user.id),
        username: Some(user.username),
        role: Some(user.role.clone()),
        is_admin: user.role == "admin",
    }))
}

#[post("/api/auth/logout")]
pub async fn logout(
    req: HttpRequest,
    session: Session,
    app_data: web::Data<AppState>,
) -> actix_web::Result<HttpResponse> {
    let actor = require_user(&session, &app_data).await.ok();
    clear_session_user(&session);
    write_audit(
        &req,
        &app_data,
        actor.as_ref(),
        "logout",
        "session",
        "",
        "success",
        "",
    )
    .await;
    Ok(HttpResponse::Ok().json(serde_json::json!({ "success": true })))
}

#[get("/api/auth/me")]
pub async fn me(
    session: Session,
    app_data: web::Data<AppState>,
) -> actix_web::Result<HttpResponse> {
    match require_user(&session, &app_data).await {
        Ok(user) => {
            let is_admin = user.is_admin();
            Ok(HttpResponse::Ok().json(MeResponse {
                authenticated: true,
                id: Some(user.id),
                username: Some(user.username),
                role: Some(user.role),
                is_admin,
            }))
        }
        Err(_) => Ok(HttpResponse::Ok().json(MeResponse {
            authenticated: false,
            id: None,
            username: None,
            role: None,
            is_admin: false,
        })),
    }
}
