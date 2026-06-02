use crate::api::{map_db_err, require_admin, require_user, utc_now_millis, write_audit, AppState};
use crate::orm_entities::prelude::{LogTypeMapping, UploadLog, UploadUser};
use crate::orm_entities::{log_type_mapping, upload_log, upload_log_resolution, upload_user};
use actix_session::Session;
use actix_web::{post, web, HttpRequest, HttpResponse, Responder};
use regex::Regex;
use sea_orm::ActiveValue::Set;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, Condition, ConnectionTrait, EntityTrait, IntoActiveModel,
    ModelTrait, NotSet, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, Statement, Value,
};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Debug)]
struct UploadLogData {
    log_type: String,
    message: String,
    user: String,
    package: String,
    nav_url: String,
    version: String,
    #[serde(default = "default_string")]
    logs: String,
}

fn default_string() -> String {
    "".into()
}

fn normalize_error_message(error_msg: &str) -> String {
    let addr_regex = Regex::new(r"0x[0-9a-fA-F]+").unwrap();
    addr_regex
        .replace_all(error_msg, "[MEMORY_ADDRESS]")
        .to_string()
}

#[post("/api/upload_log")]
pub async fn api_upload_log(
    req: HttpRequest,
    app_data: web::Data<AppState>,
    json_data: web::Json<UploadLogData>,
) -> actix_web::Result<HttpResponse> {
    if json_data.log_type.starts_with("error") {
        let normalized = normalize_error_message(&json_data.message);
        let digest = md5::compute(format!("{}-{}", normalized, json_data.log_type));
        let hash_string = format!("{:x}", digest);
        let db = app_data.db_pool.as_ref();
        let now = utc_now_millis();
        let now_text = now.to_string();

        let log_data = UploadLog::find()
            .filter(upload_log::Column::Hash.eq(&hash_string))
            .one(db)
            .await
            .map_err(map_db_err)?;

        let ip = req
            .connection_info()
            .realip_remote_addr()
            .unwrap_or("unknown")
            .to_string();
        let source_user = upload_user::ActiveModel {
            id: NotSet,
            package: Set(json_data.package.to_owned()),
            nav_url: Set(json_data.nav_url.to_owned()),
            version: Set(json_data.version.to_owned()),
            logs: Set(String::new()),
            user: Set(json_data.user.to_owned()),
            ip: Set(ip.clone()),
            time: Set(now),
        }
        .insert(db)
        .await
        .map_err(map_db_err)?;
        insert_user_raw_log(&app_data, source_user.id, &json_data.logs, &now_text).await?;

        let log_id = if let Some(log_data) = log_data {
            let total_count = log_data.total_count + 1;
            let status = if log_data.status == 0 { 0 } else { -1 };
            let log_id = log_data.id;

            let mut log_active_model: upload_log::ActiveModel = log_data.into();
            log_active_model.total_count = Set(total_count);
            log_active_model.last_time = Set(now);
            log_active_model.updated_at = Set(Some(now));
            log_active_model.status = Set(status);
            if status == -1 {
                log_active_model.resolved_by_user_id = Set(None);
                log_active_model.resolved_by_username = Set(None);
            }

            log_active_model.update(db).await.map_err(map_db_err)?;
            log_id
        } else {
            upload_log::ActiveModel {
                id: NotSet,
                hash: Set(hash_string.clone()),
                first_time: Set(now),
                last_time: Set(now),
                total_count: Set(1),
                status: Set(0),
                resolution_time: Set(now),
                resolved_by_user_id: Set(None),
                resolved_by_username: Set(None),
                updated_at: Set(Some(now)),
                log_type: Set(json_data.log_type.to_owned()),
                message: Set(json_data.message.to_owned()),
            }
            .insert(db)
            .await
            .map_err(map_db_err)?
            .id
        };

        insert_log_source(
            &app_data,
            log_id,
            &hash_string,
            source_user.id,
            &json_data.log_type,
            &json_data.package,
            &json_data.nav_url,
            &json_data.version,
            &json_data.user,
            &ip,
            &now_text,
        )
        .await?;
    }

    Ok(HttpResponse::Ok().json(serde_json::json!({ "data": "ok" })))
}

#[derive(Deserialize, Debug)]
struct LogListRequestData {
    page: i32,
    page_size: i32,
    #[serde(default)]
    log_type: String,
    #[serde(default = "default_sort_by")]
    sort_by: String,
    #[serde(default = "default_sort_order")]
    sort_order: String,
    #[serde(default = "default_status_filter")]
    status_filter: String,
}

fn default_sort_by() -> String {
    "last_time".to_string()
}

fn default_sort_order() -> String {
    "desc".to_string()
}

fn default_status_filter() -> String {
    "all".to_string()
}

#[derive(Serialize, Debug)]
struct LogListItemData {
    hash: String,
    log_type: String,
    log_type_name: String,
    first_time: i64,
    last_time: i64,
    total_count: i32,
    status: i32,
    message: String,
    resolved_by_username: Option<String>,
}

#[derive(Serialize, Debug)]
struct LogListResponseData {
    success: bool,
    log_type: String,
    log_type_name: String,
    total: i32,
    pending: i32,
    recurred: i32,
    solved: i32,
    filtered_total: i32,
    total_pages: i32,
    is_admin: bool,
    items: Vec<LogListItemData>,
}

#[post("/api/log_list")]
pub async fn api_log_list(
    session: Session,
    app_data: web::Data<AppState>,
    json_data: web::Json<LogListRequestData>,
) -> actix_web::Result<HttpResponse> {
    let actor = require_user(&session, &app_data).await?;
    let db = app_data.db_pool.as_ref();
    if json_data.log_type.is_empty() {
        return Ok(HttpResponse::Ok().json(LogListResponseData {
            success: true,
            log_type: String::new(),
            log_type_name: String::new(),
            total: 0,
            pending: 0,
            recurred: 0,
            solved: 0,
            filtered_total: 0,
            total_pages: 0,
            is_admin: actor.is_admin(),
            items: vec![],
        }));
    }
    let base_condition =
        Condition::all().add(upload_log::Column::LogType.eq(json_data.log_type.clone()));
    let mut list_condition = base_condition.clone();
    match json_data.status_filter.as_str() {
        "pending" => {
            list_condition = list_condition.add(upload_log::Column::Status.eq(0));
        }
        "recurred" => {
            list_condition = list_condition.add(upload_log::Column::Status.eq(-1));
        }
        "solved" => {
            list_condition = list_condition.add(upload_log::Column::Status.eq(1));
        }
        _ => {}
    }
    let log_type_name = display_name_for_log_type(db, &json_data.log_type).await?;

    let page = json_data.page.max(1);
    let page_size = json_data.page_size.clamp(1, 100);
    let offset = ((page - 1) * page_size) as u64;
    let limit = page_size as u64;

    let total_count = UploadLog::find()
        .filter(base_condition.clone())
        .count(db)
        .await
        .map_err(map_db_err)? as i32;
    let pending_count = UploadLog::find()
        .filter(
            Condition::all()
                .add(base_condition.clone())
                .add(upload_log::Column::Status.eq(0)),
        )
        .count(db)
        .await
        .map_err(map_db_err)? as i32;
    let solved_count = UploadLog::find()
        .filter(
            Condition::all()
                .add(base_condition.clone())
                .add(upload_log::Column::Status.eq(1)),
        )
        .count(db)
        .await
        .map_err(map_db_err)? as i32;
    let recurred_count = UploadLog::find()
        .filter(
            Condition::all()
                .add(base_condition.clone())
                .add(upload_log::Column::Status.eq(-1)),
        )
        .count(db)
        .await
        .map_err(map_db_err)? as i32;
    let filtered_count = UploadLog::find()
        .filter(list_condition.clone())
        .count(db)
        .await
        .map_err(map_db_err)? as i32;
    let total_pages = if filtered_count == 0 {
        0
    } else {
        (filtered_count as f64 / page_size as f64).ceil() as i32
    };

    let mut query = UploadLog::find().filter(list_condition);
    let sort_desc = json_data.sort_order != "asc";
    query = match (json_data.sort_by.as_str(), sort_desc) {
        ("total_count", true) => query.order_by_desc(upload_log::Column::TotalCount),
        ("total_count", false) => query.order_by_asc(upload_log::Column::TotalCount),
        (_, false) => query.order_by_asc(upload_log::Column::LastTime),
        _ => query.order_by_desc(upload_log::Column::LastTime),
    };

    let items = query
        .offset(offset)
        .limit(limit)
        .all(db)
        .await
        .map_err(map_db_err)?
        .into_iter()
        .map(|log| LogListItemData {
            log_type_name: display_name(&log.log_type, None),
            log_type: log.log_type,
            hash: log.hash,
            first_time: log.first_time.and_utc().timestamp(),
            last_time: log.last_time.and_utc().timestamp(),
            total_count: log.total_count,
            status: log.status,
            message: log.message,
            resolved_by_username: log.resolved_by_username,
        })
        .collect();

    Ok(HttpResponse::Ok().json(LogListResponseData {
        success: true,
        log_type: json_data.log_type.clone(),
        log_type_name,
        total: total_count,
        pending: pending_count,
        recurred: recurred_count,
        solved: solved_count,
        filtered_total: filtered_count,
        total_pages,
        is_admin: actor.is_admin(),
        items,
    }))
}

#[derive(Deserialize, Debug)]
struct LogContentRequestData {
    hash: String,
    #[serde(default = "default_source_page")]
    source_page: i32,
    #[serde(default = "default_source_page_size")]
    source_page_size: i32,
}

fn default_source_page() -> i32 {
    1
}

fn default_source_page_size() -> i32 {
    20
}

#[derive(Serialize, Debug)]
struct LogContentResponseBriefUserData {
    id: i32,
    package: String,
    nav_url: String,
    version: String,
    user: String,
    ip: String,
    time: String,
}

#[derive(Serialize, Debug)]
struct LogResolutionHistoryItem {
    id: i32,
    resolved_by_user_id: i32,
    resolved_by_username: String,
    resolved_at: i64,
}

#[derive(Serialize, Debug)]
struct LogContentResponseData {
    hash: String,
    log_type: String,
    log_type_name: String,
    sources: Vec<LogContentResponseBriefUserData>,
    source_page: i32,
    source_page_size: i32,
    source_total: i32,
    source_total_pages: i32,
    first_time: i64,
    last_time: i64,
    total_count: i32,
    status: i32,
    resolution_time: i64,
    resolved_by_user_id: Option<i32>,
    resolved_by_username: Option<String>,
    resolution_history: Vec<LogResolutionHistoryItem>,
    message: String,
    can_remove: bool,
}

#[post("/api/log_content")]
pub async fn api_log_content(
    session: Session,
    app_data: web::Data<AppState>,
    json_data: web::Json<LogContentRequestData>,
) -> actix_web::Result<HttpResponse> {
    let actor = require_user(&session, &app_data).await?;
    let Some(logs) = UploadLog::find()
        .filter(upload_log::Column::Hash.eq(&json_data.hash))
        .one(app_data.db_pool.as_ref())
        .await
        .map_err(map_db_err)?
    else {
        return Err(actix_web::error::ErrorNotFound("log not found"));
    };

    let source_page = json_data.source_page.max(1);
    let source_page_size = json_data.source_page_size.clamp(10, 100);
    let source_data =
        log_sources_for_content(&app_data, logs.id, source_page, source_page_size).await?;

    let log_type_name =
        display_name_for_log_type(app_data.db_pool.as_ref(), &logs.log_type).await?;
    let resolution_history = resolution_history_for_log(&app_data, logs.id).await?;

    Ok(HttpResponse::Ok().json(LogContentResponseData {
        hash: logs.hash,
        log_type: logs.log_type,
        log_type_name,
        sources: source_data.items,
        source_page,
        source_page_size,
        source_total: source_data.total,
        source_total_pages: source_data.total_pages,
        first_time: logs.first_time.and_utc().timestamp(),
        last_time: logs.last_time.and_utc().timestamp(),
        total_count: logs.total_count,
        status: logs.status,
        resolution_time: logs.resolution_time.and_utc().timestamp(),
        resolved_by_user_id: logs.resolved_by_user_id,
        resolved_by_username: logs.resolved_by_username,
        resolution_history,
        message: logs.message,
        can_remove: logs.status == 1 && actor.is_admin(),
    }))
}

#[post("/api/log_complete")]
pub async fn api_log_complete(
    req: HttpRequest,
    session: Session,
    app_data: web::Data<AppState>,
    json_data: web::Json<LogContentRequestData>,
) -> actix_web::Result<impl Responder> {
    let actor = require_user(&session, &app_data).await?;
    if let Some(log_data_model) = UploadLog::find()
        .filter(upload_log::Column::Hash.eq(&json_data.hash))
        .one(app_data.db_pool.as_ref())
        .await
        .map_err(map_db_err)?
    {
        let now = utc_now_millis();
        let upload_log_id = log_data_model.id;
        let upload_log_hash = log_data_model.hash.clone();
        let mut log_active_model: upload_log::ActiveModel = log_data_model.into();
        log_active_model.status = Set(1);
        log_active_model.resolution_time = Set(now);
        log_active_model.updated_at = Set(Some(now));
        log_active_model.resolved_by_user_id = Set(Some(actor.id));
        log_active_model.resolved_by_username = Set(Some(actor.username.clone()));
        log_active_model
            .save(app_data.db_pool.as_ref())
            .await
            .map_err(map_db_err)?;
        upload_log_resolution::ActiveModel {
            id: NotSet,
            upload_log_id: Set(upload_log_id),
            upload_log_hash: Set(upload_log_hash),
            resolved_by_user_id: Set(actor.id),
            resolved_by_username: Set(actor.username.clone()),
            resolved_at: Set(now),
        }
        .insert(app_data.db_pool.as_ref())
        .await
        .map_err(map_db_err)?;
        write_audit(
            &req,
            &app_data,
            Some(&actor),
            "complete_log",
            "upload_log",
            &json_data.hash,
            "success",
            "",
        )
        .await;
    }

    Ok(HttpResponse::Ok().json(serde_json::json!({ "data": "ok" })))
}

#[derive(Serialize)]
struct LogTypeItem {
    log_type: String,
    display_name: String,
    configured: bool,
    enabled: bool,
    total: i64,
    pending: i64,
    solved: i64,
}

#[post("/api/log_types")]
pub async fn api_log_types(
    session: Session,
    app_data: web::Data<AppState>,
) -> actix_web::Result<HttpResponse> {
    require_user(&session, &app_data).await?;
    let rows = app_data
        .db_pool
        .query_all(Statement::from_sql_and_values(
            app_data.db_pool.get_database_backend(),
            "SELECT types.log_type AS log_type,
                    COALESCE(mapping.display_name, types.log_type) AS display_name,
                    CASE WHEN mapping.id IS NULL THEN 0 ELSE 1 END AS configured,
                    COALESCE(mapping.enabled, 1) AS enabled,
                    COALESCE(stats.total, 0) AS total,
                    COALESCE(stats.pending, 0) AS pending,
                    COALESCE(stats.solved, 0) AS solved
             FROM (
                 SELECT log_type FROM upload_log
                 UNION
                 SELECT log_type FROM log_type_mappings
             ) types
             LEFT JOIN (
                 SELECT log_type,
                        COUNT(*) AS total,
                        SUM(CASE WHEN status = 0 THEN 1 ELSE 0 END) AS pending,
                        SUM(CASE WHEN status = 1 THEN 1 ELSE 0 END) AS solved
                 FROM upload_log
                 GROUP BY log_type
             ) stats ON stats.log_type = types.log_type
             LEFT JOIN log_type_mappings mapping ON mapping.log_type = types.log_type
             WHERE COALESCE(mapping.enabled, 1) = 1
             ORDER BY display_name, types.log_type",
            vec![],
        ))
        .await
        .map_err(map_db_err)?;

    let items = rows
        .into_iter()
        .map(|row| LogTypeItem {
            log_type: row.try_get("", "log_type").unwrap_or_default(),
            display_name: row.try_get("", "display_name").unwrap_or_default(),
            configured: row.try_get::<i32>("", "configured").unwrap_or_default() != 0,
            enabled: row.try_get::<i32>("", "enabled").unwrap_or(1) != 0,
            total: row.try_get("", "total").unwrap_or_default(),
            pending: row.try_get("", "pending").unwrap_or_default(),
            solved: row.try_get("", "solved").unwrap_or_default(),
        })
        .collect::<Vec<_>>();

    Ok(HttpResponse::Ok().json(serde_json::json!({ "items": items })))
}

#[derive(Deserialize)]
struct SaveLogTypeRequest {
    log_type: String,
    display_name: String,
    enabled: bool,
}

#[post("/api/admin/log_types/save")]
pub async fn api_save_log_type(
    req: HttpRequest,
    session: Session,
    app_data: web::Data<AppState>,
    json_data: web::Json<SaveLogTypeRequest>,
) -> actix_web::Result<HttpResponse> {
    let actor = require_admin(&session, &app_data).await?;
    let log_type = json_data.log_type.trim();
    let display_name = json_data.display_name.trim();
    if log_type.is_empty() || display_name.is_empty() {
        return Err(actix_web::error::ErrorBadRequest(
            "log_type and display_name are required",
        ));
    }

    let now = utc_now_millis();
    if let Some(existing) = LogTypeMapping::find()
        .filter(log_type_mapping::Column::LogType.eq(log_type))
        .one(app_data.db_pool.as_ref())
        .await
        .map_err(map_db_err)?
    {
        let mut model = existing.into_active_model();
        model.display_name = Set(display_name.to_string());
        model.enabled = Set(json_data.enabled);
        model.updated_at = Set(now);
        model
            .update(app_data.db_pool.as_ref())
            .await
            .map_err(map_db_err)?;
    } else {
        log_type_mapping::ActiveModel {
            id: NotSet,
            log_type: Set(log_type.to_string()),
            display_name: Set(display_name.to_string()),
            enabled: Set(json_data.enabled),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(app_data.db_pool.as_ref())
        .await
        .map_err(map_db_err)?;
    }

    write_audit(
        &req,
        &app_data,
        Some(&actor),
        "save_log_type",
        "log_type_mapping",
        log_type,
        "success",
        display_name,
    )
    .await;

    Ok(HttpResponse::Ok().json(serde_json::json!({ "success": true })))
}

async fn display_name_for_log_type(
    db: &sea_orm::DatabaseConnection,
    log_type: &str,
) -> actix_web::Result<String> {
    let mapping = LogTypeMapping::find()
        .filter(log_type_mapping::Column::LogType.eq(log_type))
        .filter(log_type_mapping::Column::Enabled.eq(true))
        .one(db)
        .await
        .map_err(map_db_err)?;
    Ok(display_name(
        log_type,
        mapping
            .as_ref()
            .map(|mapping| mapping.display_name.as_str()),
    ))
}

fn display_name(log_type: &str, configured_name: Option<&str>) -> String {
    configured_name
        .filter(|name| !name.is_empty())
        .unwrap_or(log_type)
        .to_string()
}

#[post("/api/log_remove")]
pub async fn api_log_remove(
    req: HttpRequest,
    session: Session,
    app_data: web::Data<AppState>,
    json_data: web::Json<LogContentRequestData>,
) -> actix_web::Result<impl Responder> {
    let actor = require_admin(&session, &app_data).await?;

    if let Some(log_data_model) = UploadLog::find()
        .filter(upload_log::Column::Hash.eq(&json_data.hash))
        .one(app_data.db_pool.as_ref())
        .await
        .map_err(map_db_err)?
    {
        delete_sources_for_filter(
            &app_data,
            "upload_log_id = ?",
            vec![log_data_model.id.into()],
        )
        .await?;
        delete_resolution_history_for_filter(
            &app_data,
            "upload_log_id = ?",
            vec![log_data_model.id.into()],
        )
        .await?;

        log_data_model
            .delete(app_data.db_pool.as_ref())
            .await
            .map_err(map_db_err)?;
        write_audit(
            &req,
            &app_data,
            Some(&actor),
            "remove_log",
            "upload_log",
            &json_data.hash,
            "success",
            "",
        )
        .await;
    }

    Ok(HttpResponse::Ok().json(serde_json::json!({ "data": "ok" })))
}

#[derive(Deserialize, Debug)]
struct UserLogRequestData {
    id: i32,
}

#[derive(Serialize, Debug)]
struct UserLogResponseData {
    id: i32,
    package: Option<String>,
    nav_url: Option<String>,
    version: Option<String>,
    user: Option<String>,
    ip: Option<String>,
    time: Option<String>,
    logs: String,
}

#[post("/api/user_log")]
pub async fn api_user_log(
    session: Session,
    app_data: web::Data<AppState>,
    json_data: web::Json<UserLogRequestData>,
) -> actix_web::Result<HttpResponse> {
    require_user(&session, &app_data).await?;
    let user_data = UploadUser::find_by_id(json_data.id)
        .one(app_data.db_pool.as_ref())
        .await
        .map_err(map_db_err)?;
    if let Some(raw_log) = user_raw_log(&app_data, json_data.id).await? {
        Ok(HttpResponse::Ok().json(UserLogResponseData {
            id: json_data.id,
            package: user_data.as_ref().map(|user| user.package.clone()),
            nav_url: user_data.as_ref().map(|user| user.nav_url.clone()),
            version: user_data.as_ref().map(|user| user.version.clone()),
            user: user_data.as_ref().map(|user| user.user.clone()),
            ip: user_data.as_ref().map(|user| user.ip.clone()),
            time: user_data
                .as_ref()
                .map(|user| user.time.format("%Y-%m-%d %H:%M:%S%.3f").to_string()),
            logs: if raw_log.is_empty() {
                format!("No logs for id {}", json_data.id)
            } else {
                raw_log
            },
        }))
    } else if let Some(user_data) = user_data {
        Ok(HttpResponse::Ok().json(UserLogResponseData {
            id: user_data.id,
            package: Some(user_data.package),
            nav_url: Some(user_data.nav_url),
            version: Some(user_data.version),
            user: Some(user_data.user),
            ip: Some(user_data.ip),
            time: Some(user_data.time.format("%Y-%m-%d %H:%M:%S%.3f").to_string()),
            logs: if user_data.logs.is_empty() {
                format!("No logs for id {}", json_data.id)
            } else {
                user_data.logs
            },
        }))
    } else {
        Ok(HttpResponse::Ok().json(UserLogResponseData {
            id: json_data.id,
            package: None,
            nav_url: None,
            version: None,
            user: None,
            ip: None,
            time: None,
            logs: format!("Not found user log for id {}", json_data.id),
        }))
    }
}

#[derive(Deserialize, Debug)]
struct ClearLogRequestData {
    log_type: String,
}

#[post("/api/clear_log")]
pub async fn api_clear_log(
    req: HttpRequest,
    session: Session,
    app_data: web::Data<AppState>,
    json_data: web::Json<ClearLogRequestData>,
) -> actix_web::Result<HttpResponse> {
    let actor = require_admin(&session, &app_data).await?;
    let logs = UploadLog::find()
        .filter(upload_log::Column::LogType.eq(&json_data.log_type))
        .all(app_data.db_pool.as_ref())
        .await
        .map_err(map_db_err)?;

    delete_sources_for_filter(
        &app_data,
        "log_type = ?",
        vec![json_data.log_type.clone().into()],
    )
    .await?;
    delete_resolution_history_for_log_type(&app_data, &json_data.log_type).await?;

    UploadLog::delete_many()
        .filter(upload_log::Column::LogType.eq(&json_data.log_type))
        .exec(app_data.db_pool.as_ref())
        .await
        .map_err(map_db_err)?;

    write_audit(
        &req,
        &app_data,
        Some(&actor),
        "clear_log",
        "upload_log",
        &json_data.log_type,
        "success",
        format!("removed {} log groups", logs.len()),
    )
    .await;

    Ok(HttpResponse::Ok().json(serde_json::json!({ "data": "ok" })))
}

async fn insert_user_raw_log(
    app_data: &web::Data<AppState>,
    upload_user_id: i32,
    logs: &str,
    created_at: &str,
) -> actix_web::Result<()> {
    app_data
        .db_pool
        .execute(Statement::from_sql_and_values(
            app_data.db_pool.get_database_backend(),
            "INSERT OR REPLACE INTO upload_user_logs(upload_user_id, logs, created_at)
             VALUES (?, ?, ?)",
            vec![
                upload_user_id.into(),
                logs.to_owned().into(),
                created_at.to_owned().into(),
            ],
        ))
        .await
        .map_err(map_db_err)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn insert_log_source(
    app_data: &web::Data<AppState>,
    upload_log_id: i32,
    upload_log_hash: &str,
    upload_user_id: i32,
    log_type: &str,
    package: &str,
    nav_url: &str,
    version: &str,
    user: &str,
    ip: &str,
    reported_at: &str,
) -> actix_web::Result<()> {
    app_data
        .db_pool
        .execute(Statement::from_sql_and_values(
            app_data.db_pool.get_database_backend(),
            "INSERT OR IGNORE INTO upload_log_sources(
                upload_log_id, upload_log_hash, upload_user_id, log_type,
                package, nav_url, version, user, ip, reported_at
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            vec![
                upload_log_id.into(),
                upload_log_hash.to_owned().into(),
                upload_user_id.into(),
                log_type.to_owned().into(),
                package.to_owned().into(),
                nav_url.to_owned().into(),
                version.to_owned().into(),
                user.to_owned().into(),
                ip.to_owned().into(),
                reported_at.to_owned().into(),
            ],
        ))
        .await
        .map_err(map_db_err)?;
    Ok(())
}

async fn log_sources_for_content(
    app_data: &web::Data<AppState>,
    upload_log_id: i32,
    page: i32,
    page_size: i32,
) -> actix_web::Result<PaginatedLogSources> {
    let offset = ((page - 1) * page_size) as i64;
    let total_row = app_data
        .db_pool
        .query_one(Statement::from_sql_and_values(
            app_data.db_pool.get_database_backend(),
            "SELECT COUNT(*) AS total FROM upload_log_sources WHERE upload_log_id = ?",
            vec![upload_log_id.into()],
        ))
        .await
        .map_err(map_db_err)?;
    let total = total_row
        .and_then(|row| row.try_get::<i64>("", "total").ok())
        .unwrap_or_default() as i32;
    let source_total_pages = total_pages(total, page_size);

    let rows = app_data
        .db_pool
        .query_all(Statement::from_sql_and_values(
            app_data.db_pool.get_database_backend(),
            "SELECT upload_user_id AS id, package, nav_url, version, user, ip, reported_at
             FROM upload_log_sources
             WHERE upload_log_id = ?
             ORDER BY reported_at DESC, id DESC
             LIMIT ? OFFSET ?",
            vec![upload_log_id.into(), page_size.into(), offset.into()],
        ))
        .await
        .map_err(map_db_err)?;

    Ok(PaginatedLogSources {
        items: rows
            .into_iter()
            .map(|row| LogContentResponseBriefUserData {
                id: row.try_get("", "id").unwrap_or_default(),
                package: row.try_get("", "package").unwrap_or_default(),
                nav_url: row.try_get("", "nav_url").unwrap_or_default(),
                version: row.try_get("", "version").unwrap_or_default(),
                user: row.try_get("", "user").unwrap_or_default(),
                ip: row.try_get("", "ip").unwrap_or_default(),
                time: row.try_get::<String>("", "reported_at").unwrap_or_default(),
            })
            .collect(),
        total,
        total_pages: source_total_pages,
    })
}

struct PaginatedLogSources {
    items: Vec<LogContentResponseBriefUserData>,
    total: i32,
    total_pages: i32,
}

fn total_pages(total: i32, page_size: i32) -> i32 {
    if total <= 0 {
        0
    } else {
        (total as f64 / page_size as f64).ceil() as i32
    }
}

async fn user_raw_log(
    app_data: &web::Data<AppState>,
    upload_user_id: i32,
) -> actix_web::Result<Option<String>> {
    let row = app_data
        .db_pool
        .query_one(Statement::from_sql_and_values(
            app_data.db_pool.get_database_backend(),
            "SELECT logs FROM upload_user_logs WHERE upload_user_id = ?",
            vec![upload_user_id.into()],
        ))
        .await
        .map_err(map_db_err)?;
    Ok(row.and_then(|row| row.try_get("", "logs").ok()))
}

async fn resolution_history_for_log(
    app_data: &web::Data<AppState>,
    upload_log_id: i32,
) -> actix_web::Result<Vec<LogResolutionHistoryItem>> {
    let rows = app_data
        .db_pool
        .query_all(Statement::from_sql_and_values(
            app_data.db_pool.get_database_backend(),
            "SELECT id, resolved_by_user_id, resolved_by_username, resolved_at
             FROM upload_log_resolutions
             WHERE upload_log_id = ?
             ORDER BY resolved_at DESC, id DESC",
            vec![upload_log_id.into()],
        ))
        .await
        .map_err(map_db_err)?;

    Ok(rows
        .into_iter()
        .map(|row| LogResolutionHistoryItem {
            id: row.try_get("", "id").unwrap_or_default(),
            resolved_by_user_id: row.try_get("", "resolved_by_user_id").unwrap_or_default(),
            resolved_by_username: row.try_get("", "resolved_by_username").unwrap_or_default(),
            resolved_at: row
                .try_get::<chrono::NaiveDateTime>("", "resolved_at")
                .map(|time| time.and_utc().timestamp())
                .unwrap_or_default(),
        })
        .collect())
}

async fn delete_sources_for_filter(
    app_data: &web::Data<AppState>,
    where_clause: &str,
    values: Vec<Value>,
) -> actix_web::Result<()> {
    let user_id_sql = format!("SELECT upload_user_id FROM upload_log_sources WHERE {where_clause}");
    let delete_raw_sql =
        format!("DELETE FROM upload_user_logs WHERE upload_user_id IN ({user_id_sql})");
    app_data
        .db_pool
        .execute(Statement::from_sql_and_values(
            app_data.db_pool.get_database_backend(),
            &delete_raw_sql,
            values.clone(),
        ))
        .await
        .map_err(map_db_err)?;

    let delete_users_sql = format!("DELETE FROM upload_user WHERE id IN ({user_id_sql})");
    app_data
        .db_pool
        .execute(Statement::from_sql_and_values(
            app_data.db_pool.get_database_backend(),
            &delete_users_sql,
            values.clone(),
        ))
        .await
        .map_err(map_db_err)?;

    let delete_sources_sql = format!("DELETE FROM upload_log_sources WHERE {where_clause}");
    app_data
        .db_pool
        .execute(Statement::from_sql_and_values(
            app_data.db_pool.get_database_backend(),
            &delete_sources_sql,
            values,
        ))
        .await
        .map_err(map_db_err)?;

    Ok(())
}

async fn delete_resolution_history_for_filter(
    app_data: &web::Data<AppState>,
    where_clause: &str,
    values: Vec<Value>,
) -> actix_web::Result<()> {
    let sql = format!("DELETE FROM upload_log_resolutions WHERE {where_clause}");
    app_data
        .db_pool
        .execute(Statement::from_sql_and_values(
            app_data.db_pool.get_database_backend(),
            &sql,
            values,
        ))
        .await
        .map_err(map_db_err)?;
    Ok(())
}

async fn delete_resolution_history_for_log_type(
    app_data: &web::Data<AppState>,
    log_type: &str,
) -> actix_web::Result<()> {
    app_data
        .db_pool
        .execute(Statement::from_sql_and_values(
            app_data.db_pool.get_database_backend(),
            "DELETE FROM upload_log_resolutions
             WHERE upload_log_id IN (SELECT id FROM upload_log WHERE log_type = ?)",
            vec![log_type.to_owned().into()],
        ))
        .await
        .map_err(map_db_err)?;
    Ok(())
}
