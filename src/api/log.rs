use crate::api::{map_db_err, user_authentication, AppState};
use crate::orm_entities::prelude::{UploadLog, UploadUser};
use crate::orm_entities::{upload_log, upload_user};
use actix_web::{post, web, HttpRequest, HttpResponse, Responder};
use actix_web_httpauth::extractors::basic::BasicAuth;
use chrono::Utc;
use sea_orm::ActiveValue::Set;
use sea_orm::ColumnTrait;
use sea_orm::QueryFilter;
use sea_orm::{ActiveModelTrait, EntityTrait, ModelTrait, NotSet};
use serde::{Deserialize, Serialize};

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

#[post("/api/upload_log")]
pub async fn api_upload_log(
    req: HttpRequest,
    app_data: web::Data<AppState>,
    json_data: web::Json<UploadLogData>,
) -> actix_web::Result<HttpResponse> {
    // println!("{:?}", req);
    // println!("{:?}", json_data);

    if json_data.log_type.starts_with("error") {
        // 计算消息内容哈希值
        let digest = md5::compute(&format!("{}-{}", json_data.message, json_data.log_type));
        let hash_string = format!("{:x}", digest);

        let log_data = UploadLog::find()
            .filter(upload_log::Column::Hash.eq(&hash_string))
            .one(app_data.db_pool.get().unwrap())
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

            let user = user
                .save(app_data.db_pool.get().unwrap())
                .await
                .map_err(map_db_err)?;
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

        log_active_model
            .save(app_data.db_pool.get().unwrap())
            .await
            .map_err(map_db_err)?;
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
    can_remove: bool,
}

#[post("/api/log_content")]
pub async fn api_log_content(
    req: HttpRequest,
    credentials: BasicAuth,
    app_data: web::Data<AppState>,
    json_data: web::Json<LogContentRequestData>,
) -> actix_web::Result<HttpResponse> {
    let is_admin = user_authentication(&req, &credentials, &app_data).await?;

    let logs = UploadLog::find()
        .filter(upload_log::Column::Hash.eq(&json_data.hash))
        .one(app_data.db_pool.get().unwrap())
        .await
        .map_err(map_db_err)?;

    if let Some(logs) = logs {
        let mut user_list = vec![];

        for (_, id) in logs.user_list.split(",").enumerate() {
            if let Some(user_data) = UploadUser::find()
                .filter(upload_user::Column::Id.eq(id))
                .one(app_data.db_pool.get().unwrap())
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
            can_remove: if logs.status == 1 { is_admin } else { false },
        };

        Ok(HttpResponse::Ok().body(serde_json::to_string(&response)?))
    } else {
        Ok(HttpResponse::Ok().body(format!("no file: {}", json_data.hash)))
    }
}

#[post("/api/log_complete")]
pub async fn api_log_complete(
    req: HttpRequest,
    credentials: BasicAuth,
    app_data: web::Data<AppState>,
    json_data: web::Json<LogContentRequestData>,
) -> actix_web::Result<impl Responder> {
    let _ = user_authentication(&req, &credentials, &app_data).await?;

    if let Some(log_data_model) = UploadLog::find()
        .filter(upload_log::Column::Hash.eq(&json_data.hash))
        .one(app_data.db_pool.get().unwrap())
        .await
        .map_err(map_db_err)?
    {
        let mut log_active_model: upload_log::ActiveModel = log_data_model.into();
        log_active_model.status = Set(1);
        log_active_model.resolution_time = Set(Utc::now().naive_utc());
        log_active_model
            .save(app_data.db_pool.get().unwrap())
            .await
            .map_err(map_db_err)?;
    }

    Ok(HttpResponse::Ok().body("{\"data\": \"ok\"}"))
}

#[post("/api/log_remove")]
pub async fn api_log_remove(
    req: HttpRequest,
    credentials: BasicAuth,
    app_data: web::Data<AppState>,
    json_data: web::Json<LogContentRequestData>,
) -> actix_web::Result<impl Responder> {
    if !user_authentication(&req, &credentials, &app_data).await? {
        return Ok(HttpResponse::Forbidden().finish());
    }

    if let Some(log_data_model) = UploadLog::find()
        .filter(upload_log::Column::Hash.eq(&json_data.hash))
        .one(app_data.db_pool.get().unwrap())
        .await
        .map_err(map_db_err)?
    {
        for (_, id) in log_data_model.user_list.split(",").enumerate() {
            let upload_user = UploadUser::find()
                .filter(upload_user::Column::Id.eq(id))
                .one(app_data.db_pool.get().unwrap())
                .await
                .map_err(map_db_err)?;

            if let Some(upload_user) = upload_user {
                upload_user
                    .delete(app_data.db_pool.get().unwrap())
                    .await
                    .map_err(map_db_err)?;
            }
        }

        let log_active_model: upload_log::ActiveModel = log_data_model.into();
        log_active_model
            .delete(app_data.db_pool.get().unwrap())
            .await
            .map_err(map_db_err)?;
    }

    Ok(HttpResponse::Ok().body("{\"data\": \"ok\"}"))
}

#[derive(Deserialize, Debug)]
struct UserLogResponseData {
    id: String,
}

#[post("/api/user_log")]
pub async fn api_user_log(
    _req: HttpRequest,
    app_data: web::Data<AppState>,
    json_data: web::Json<UserLogResponseData>,
) -> actix_web::Result<HttpResponse> {
    if let Some(user_data) = UploadUser::find()
        .filter(upload_user::Column::Id.eq(&json_data.id))
        .one(app_data.db_pool.get().unwrap())
        .await
        .map_err(map_db_err)?
    {
        Ok(HttpResponse::Ok().body(user_data.logs))
    } else {
        Ok(HttpResponse::Ok().body("empty"))
    }
}

#[derive(Deserialize, Debug)]
struct ClearLogResponseData {
    log_type: String,
}

#[post("/api/clear_log")]
pub async fn api_clear_log(
    req: HttpRequest,
    credentials: BasicAuth,
    app_data: web::Data<AppState>,
    json_data: web::Json<crate::api::log::ClearLogResponseData>,
) -> actix_web::Result<HttpResponse> {
    if !user_authentication(&req, &credentials, &app_data).await? {
        return Ok(HttpResponse::Forbidden().finish());
    }
    let logs = UploadLog::find()
        .filter(upload_log::Column::LogType.eq(&json_data.log_type))
        .all(app_data.db_pool.get().unwrap())
        .await
        .map_err(map_db_err)?;

    for log in logs {
        for v in log
            .user_list
            .split(",")
            .filter_map(|s| s.parse::<i32>().ok())
        {
            let _ = UploadUser::delete_by_id(v)
                .exec(app_data.db_pool.get().unwrap())
                .await
                .map_err(map_db_err)?;
        }
    }

    UploadLog::delete_many()
        .filter(upload_log::Column::LogType.eq(&json_data.log_type))
        .exec(app_data.db_pool.get().unwrap())
        .await
        .map_err(map_db_err)?;

    Ok(HttpResponse::Ok().body("{\"data\": \"ok\"}"))
}
