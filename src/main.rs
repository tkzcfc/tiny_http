mod orm_entities;

use crate::orm_entities::{upload_log, upload_user};
use actix_web::http::header::LOCATION;
use actix_web::{get, post, web, App, HttpRequest, HttpResponse, HttpServer, Responder, Result};
use actix_web_httpauth::extractors::basic::{BasicAuth, Config};
use chrono::Utc;
use clap::Parser;
use orm_entities::prelude::*;
use sea_orm::sea_query::SqliteQueryBuilder;
use sea_orm::ActiveValue::Set;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectOptions, ConnectionTrait, Database, DatabaseConnection,
    EntityTrait, NotSet, QueryFilter, Schema, Statement,
};
use serde::{Deserialize, Serialize};
use std::string::String;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// The address to listen on
    #[arg(long, default_value = "0.0.0.0:8000")]
    listen_addr: String,

    #[arg(long, default_value = "sqlite://data.db?mode=rwc")]
    database_url: String,

    #[arg(long, default_value = "")]
    username: String,

    #[arg(long, default_value = "")]
    password: String,
}

#[derive(Clone)]
struct AppState {
    username: Arc<String>,
    password: Arc<String>,
    db_pool: Arc<Mutex<DatabaseConnection>>,
}

#[derive(Deserialize, Debug)]
struct UploadLogData {
    // 日志类型
    log_type: String,
    // 消息
    message: String,
    // 用户
    user: String,
    // 包名
    package: String,
    // 导航服
    nav_url: String,
    // 版本信息
    version: String,
    // logs
    #[serde(default = "default_string")]
    logs: String,
}

fn default_string() -> String {
    "".into()
}

fn map_db_err(err: sea_orm::DbErr) -> actix_web::Error {
    actix_web::error::ErrorInternalServerError(format!("sqlx error:{}", err.to_string()))
}

#[post("/api/upload_log")]
async fn api_upload_log(
    req: HttpRequest,
    app_data: web::Data<AppState>,
    json_data: web::Json<UploadLogData>,
) -> Result<HttpResponse> {
    // println!("{:?}", req);
    // println!("{:?}", json_data);

    if json_data.log_type.starts_with("error") {
        // 计算消息内容哈希值
        let digest = md5::compute(&format!("{}-{}", json_data.message, json_data.log_type));
        let hash_string = format!("{:x}", digest);

        let db = &*app_data.db_pool.lock().await;

        let log_data = UploadLog::find()
            .filter(upload_log::Column::Hash.eq(&hash_string))
            .one(db)
            .await
            .map_err(map_db_err)?;

        let mut save_user = true;
        if let Some(ref log_data) = log_data {
            if log_data.user_list.split(",").count() > 100 {
                save_user = false;
            }
        }

        let mut user_id = None;
        if save_user {
            let ip = if let Some(x) = req.connection_info().realip_remote_addr() {
                x.to_string()
            } else {
                "unknown".to_string()
            };
            let user = upload_user::ActiveModel {
                id: NotSet,
                package: Set(json_data.package.to_owned()),
                nav_url: Set(json_data.nav_url.to_owned()),
                version: Set(json_data.version.to_owned()),
                logs: Set(json_data.logs.to_owned()),
                user: Set(json_data.user.to_owned()),
                ip: Set(ip),
                time: Set(Utc::now().naive_utc()),
            };

            let user = user.save(db).await.map_err(map_db_err)?;
            user_id = Some(user.id.unwrap());
        }

        let log_active_model = if let Some(log_data) = log_data {
            // 上报总数
            let total_count = log_data.total_count + 1;
            // 上报用户列表
            let mut user_list: Vec<_> = log_data
                .user_list
                .split(",")
                .map(|x| x.to_string())
                .collect();
            if let Some(user_id) = user_id {
                user_list.push(format!("{}", user_id));
            }
            let user_list = user_list.join(",");
            // 状态更新
            let status = if log_data.status == 0 { 0 } else { -1 };

            let mut log_active_model: upload_log::ActiveModel = log_data.into();
            log_active_model.total_count = Set(total_count);
            log_active_model.last_time = Set(Utc::now().naive_utc());
            log_active_model.user_list = Set(user_list);
            log_active_model.status = Set(status);

            log_active_model
        } else {
            let mut user_list: Vec<String> = vec![];
            if let Some(user_id) = user_id {
                user_list.push(format!("{user_id}"));
            }
            let user_list = user_list.join(",");

            upload_log::ActiveModel {
                id: NotSet,
                hash: Set(hash_string),
                user_list: Set(user_list),
                first_time: Set(Utc::now().naive_utc()),
                last_time: Set(Utc::now().naive_utc()),
                total_count: Set(1),
                status: Set(0),
                resolution_time: Set(Utc::now().naive_utc()),
                log_type: Set(json_data.log_type.to_owned()),
                message: Set(json_data.message.to_owned()),
            }
        };

        log_active_model.save(db).await.map_err(map_db_err)?;
    }

    Ok(HttpResponse::Ok().body("{\"data\": \"ok\"}"))
}

#[derive(Deserialize, Debug)]
struct LogContentRequestData {
    hash: String,
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
struct LogContentResponseData {
    hash: String,
    user_list: Vec<LogContentResponseBriefUserData>,
    first_time: String,
    last_time: String,
    total_count: i32,
    ///  0 未解决， 1 已解决, -1已解决之后又上报了
    status: i32,
    resolution_time: String,
    message: String,
}

#[post("/api/log_content")]
async fn api_log_content(
    _req: HttpRequest,
    app_data: web::Data<AppState>,
    json_data: web::Json<LogContentRequestData>,
) -> Result<HttpResponse> {
    let db = &*app_data.db_pool.lock().await;
    let logs = UploadLog::find()
        .filter(upload_log::Column::Hash.eq(&json_data.hash))
        .one(db)
        .await
        .map_err(map_db_err)?;

    if let Some(logs) = logs {
        let mut user_list = vec![];

        for (_, id) in logs.user_list.split(",").enumerate() {
            if let Some(user_data) = UploadUser::find()
                .filter(upload_user::Column::Id.eq(id))
                .one(db)
                .await
                .map_err(map_db_err)?
            {
                user_list.push(LogContentResponseBriefUserData {
                    id: user_data.id,
                    package: user_data.package,
                    nav_url: user_data.nav_url,
                    version: user_data.version,
                    user: user_data.user,
                    ip: user_data.ip,
                    time: user_data.time.format("%m-%d %H:%M:%S").to_string(),
                });
            }
        }

        let response = LogContentResponseData {
            hash: logs.hash,
            user_list,
            first_time: logs.first_time.format("%Y-%m-%d %H:%M:%S").to_string(),
            last_time: logs.last_time.format("%Y-%m-%d %H:%M:%S").to_string(),
            total_count: logs.total_count,
            status: logs.status,
            resolution_time: logs.resolution_time.format("%Y-%m-%d %H:%M:%S").to_string(),
            message: logs.message,
        };

        Ok(HttpResponse::Ok().body(serde_json::to_string(&response)?))
    } else {
        Ok(HttpResponse::Ok().body(format!("no file: {}", json_data.hash)))
    }
}

#[post("/api/log_complete")]
async fn api_log_complete(
    _req: HttpRequest,
    app_data: web::Data<AppState>,
    json_data: web::Json<LogContentRequestData>,
) -> Result<impl Responder> {
    let db = &*app_data.db_pool.lock().await;
    if let Some(log_data_model) = UploadLog::find()
        .filter(upload_log::Column::Hash.eq(&json_data.hash))
        .one(db)
        .await
        .map_err(map_db_err)?
    {
        let mut log_active_model: upload_log::ActiveModel = log_data_model.into();
        log_active_model.status = Set(1);
        log_active_model.resolution_time = Set(Utc::now().naive_utc());
        log_active_model.save(db).await.map_err(map_db_err)?;
    }

    Ok(HttpResponse::Ok().body("{\"data\": \"ok\"}"))
}

#[derive(Deserialize, Debug)]
struct UserLogResponseData {
    id: String,
}

#[post("/api/user_log")]
async fn api_user_log(
    _req: HttpRequest,
    app_data: web::Data<AppState>,
    json_data: web::Json<UserLogResponseData>,
) -> Result<HttpResponse> {
    let db = &*app_data.db_pool.lock().await;

    if let Some(user_data) = UploadUser::find()
        .filter(upload_user::Column::Id.eq(&json_data.id))
        .one(db)
        .await
        .map_err(map_db_err)?
    {
        Ok(HttpResponse::Ok().body(user_data.logs))
    } else {
        Ok(HttpResponse::Ok().body("empty"))
    }
}

// 用户鉴权
async fn user_authentication(
    req: &HttpRequest,
    credentials: &BasicAuth,
    app_data: &web::Data<AppState>,
) -> Result<()> {
    if !app_data.username.is_empty() || !app_data.password.is_empty() {
        let username = credentials.user_id();
        let password = credentials.password().unwrap_or_default();

        // 验证用户名和密码
        if username != app_data.username.as_str() || password != app_data.password.as_str() {
            let config = req.app_data::<Config>().cloned().unwrap_or_default();
            return Err(actix_web_httpauth::extractors::AuthenticationError::from(config).into());
        }
    }
    Ok(())
}

const FAVICON: &[u8] = include_bytes!("favicon.ico");

#[get("/{filename:.*}")]
async fn index(
    req: HttpRequest,
    credentials: BasicAuth,
    app_data: web::Data<AppState>,
) -> Result<impl Responder> {
    user_authentication(&req, &credentials, &app_data).await?;

    let file_name = req.match_info().query("filename");

    if file_name == "favicon.ico" {
        return Ok(HttpResponse::Ok()
            .content_type("image/vnd.microsoft.icon")
            .body(FAVICON));
    }

    if file_name != "index.html" {
        return Ok(HttpResponse::Found()
            .append_header((LOCATION, "/index.html"))
            .finish());
    }

    let html = include_str!("index.html");
    Ok(HttpResponse::Ok().content_type("text/html").body(html))
}

#[get("/log_content/{log_type:.+}")]
async fn log_content(
    req: HttpRequest,
    credentials: BasicAuth,
    app_data: web::Data<AppState>,
) -> Result<impl Responder> {
    user_authentication(&req, &credentials, &app_data).await?;

    let log_type = req.match_info().query("log_type");
    // let hash = req.match_info().query("hash");

    let db = &*app_data.db_pool.lock().await;
    let logs = UploadLog::find()
        .filter(upload_log::Column::LogType.eq(log_type))
        .all(db)
        .await
        .map_err(map_db_err)?;

    if logs.is_empty() {
        let html = include_str!("log_list_empty.html");
        return Ok(HttpResponse::Ok().content_type("text/html").body(html));
    }

    let mut menu_list_script = String::new();
    for log in logs {
        let lines = log.message.split("\n").collect::<Vec<_>>();

        let mut first_line = "empty";
        if !lines.is_empty() {
            first_line = lines[0];
        }

        //  0 未解决， 1 已解决,  -1已解决之后又上报了
        let script = match log.status {
            -1 => {
                format!("\n            <li class=\"yellow-dot\" id=item-{} onclick=\"onClickMenu('{}', event.currentTarget)\">{}</li>", log.hash, log.hash, first_line)
            }
            1 => {
                format!("\n            <li class=\"green-dot\" id=item-{} onclick=\"onClickMenu('{}', event.currentTarget)\">{}</li>", log.hash, log.hash, first_line)
            }
            _ => {
                format!("\n            <li class=\"red-dot\" id=item-{} onclick=\"onClickMenu('{}', event.currentTarget)\">{}</li>", log.hash, log.hash, first_line)
            }
        };

        menu_list_script.push_str(&script);
    }

    let template = include_str!("log_content.html");
    let html = template.replace("{MENU_ITEM_CODE}", &menu_list_script);

    Ok(HttpResponse::Ok().content_type("text/html").body(html))
}

#[actix_web::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let mut opt = ConnectOptions::new(&args.database_url);
    opt.max_connections(100)
        .min_connections(5)
        .connect_timeout(Duration::from_secs(8))
        .acquire_timeout(Duration::from_secs(8))
        .idle_timeout(Duration::from_secs(8))
        .max_lifetime(Duration::from_secs(8))
        .sqlx_logging(true)
        .sqlx_logging_level(log::LevelFilter::Info);

    let db_pool = Database::connect(opt)
        .await
        .expect("Database initialization failed");

    let backend = db_pool.get_database_backend();
    let schema = Schema::new(backend);
    db_pool
        .execute(Statement::from_string(
            backend,
            schema
                .create_table_from_entity(upload_user::Entity)
                .if_not_exists()
                .to_string(SqliteQueryBuilder),
        ))
        .await?;
    db_pool
        .execute(Statement::from_string(
            backend,
            schema
                .create_table_from_entity(upload_log::Entity)
                .if_not_exists()
                .to_string(SqliteQueryBuilder),
        ))
        .await?;

    println!("Starting server at http://{}", args.listen_addr);

    let app_state = AppState {
        username: Arc::new(args.username.to_owned()),
        password: Arc::new(args.password.to_owned()),
        db_pool: Arc::new(Mutex::new(db_pool)),
    };

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(app_state.clone()))
            .service(api_upload_log)
            .service(api_log_content)
            .service(api_user_log)
            .service(api_log_complete)
            .service(log_content)
            .service(index)
    })
    .bind(&args.listen_addr)?
    .run()
    .await?;

    Ok(())
}
