use crate::api::{map_db_err, AppState};
use crate::orm_entities::upload_statistics_cli_cfg;
use actix_web::{post, web, HttpRequest, HttpResponse};
use chrono::Utc;
use sea_orm::ActiveValue::Set;
use sea_orm::{ActiveModelTrait, NotSet};
use serde::Deserialize;

#[derive(Deserialize, Debug)]
struct UploadStatisticsCliCfgData {
    // 客户端类型
    cli_type: String,
    // 用户
    user: String,
    // 包名
    package: String,
    // 客户端配置信息
    configuration_info: String,
}

#[post("/api/upload_statistics_cli_cfg")]
async fn api_upload_statistics(
    req: HttpRequest,
    app_data: web::Data<AppState>,
    json_data: web::Json<UploadStatisticsCliCfgData>,
) -> actix_web::Result<HttpResponse> {
    let ip = if let Some(x) = req.connection_info().realip_remote_addr() {
        x.to_string()
    } else {
        "unknown".to_string()
    };

    let data = upload_statistics_cli_cfg::ActiveModel {
        id: NotSet,
        cli_type: Set(json_data.cli_type.to_owned()),
        user: Set(json_data.user.to_owned()),
        package: Set(json_data.package.to_owned()),
        configuration_info: Set(json_data.configuration_info.to_owned()),
        ip: Set(ip),
        time: Set(Utc::now().naive_utc()),
    };
    let _ = data
        .save(app_data.db_pool.get().unwrap())
        .await
        .map_err(map_db_err)?;

    Ok(HttpResponse::Ok().body("{\"data\": \"ok\"}"))
}
