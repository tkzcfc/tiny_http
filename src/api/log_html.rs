use crate::api::{map_db_err, user_authentication, AppState};
use crate::orm_entities::prelude::{UploadLog, UploadUser};
use crate::orm_entities::{upload_log, upload_user};
use actix_web::http::header::LOCATION;
use actix_web::{get, web, HttpRequest, HttpResponse, Responder};
use actix_web_httpauth::extractors::basic::BasicAuth;
use sea_orm::ColumnTrait;
use sea_orm::EntityTrait;
use sea_orm::QueryFilter;
use serde::Deserialize;

const FAVICON: &[u8] = include_bytes!("../html/favicon.ico");

#[get("/{filename:.*}")]
pub async fn index(
    req: HttpRequest,
    credentials: BasicAuth,
    app_data: web::Data<AppState>,
) -> actix_web::Result<impl Responder> {
    let _ = user_authentication(&req, &credentials, &app_data).await?;

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

    let html = include_str!("../html/index.html");
    Ok(HttpResponse::Ok().content_type("text/html").body(html))
}

#[derive(Deserialize, Debug)]
struct LogVersionInfo {
    #[serde(default = "default_string")]
    branch: String,
    game_id: i32,
}

fn default_string() -> String {
    "".into()
}

#[get("/log_content/{log_type:.+}")]
pub async fn log_content(
    req: HttpRequest,
    credentials: BasicAuth,
    app_data: web::Data<AppState>,
) -> actix_web::Result<impl Responder> {
    let is_admin = user_authentication(&req, &credentials, &app_data).await?;

    let log_type = req.match_info().query("log_type");
    let query_str = req.query_string();
    let show_999_proto_err = query_str.find("show_proto_err").is_some();

    let logs = UploadLog::find()
        .filter(upload_log::Column::LogType.eq(log_type))
        .all(app_data.db_pool.get().unwrap())
        .await
        .map_err(map_db_err)?;

    if logs.is_empty() {
        let html = include_str!("../html/log_list_empty.html");
        return Ok(HttpResponse::Ok().content_type("text/html").body(html));
    }

    let mut menu_list_script = String::new();
    let mut menu_list_count = 0;
    for log in logs {
        let lines = log.message.split("\n").collect::<Vec<_>>();

        let mut first_line = "empty";
        if !lines.is_empty() {
            first_line = lines[0];
        }

        let mut prefix = "[Lobby]".to_string();
        let user_list: Vec<_> = log.user_list.split(",").collect();
        if user_list.len() > 0 {
            if let Some(user_data) = UploadUser::find()
                .filter(upload_user::Column::Id.eq(user_list[0]))
                .one(app_data.db_pool.get().unwrap())
                .await
                .map_err(map_db_err)?
            {
                if let Ok(version_info) =
                    serde_json::from_str::<LogVersionInfo>(&*user_data.version)
                {
                    prefix = if version_info.game_id == 0 {
                        let branch_name = if version_info.branch.is_empty() {
                            "".to_string()
                        } else {
                            format!("({})", version_info.branch)
                        };
                        format!("[Lobby{}]", branch_name)
                    } else {
                        format!("[Game{}]", version_info.game_id)
                    }
                }
            }
        }

        prefix = format!("({}){}<{}>", menu_list_count + 1, prefix, log.total_count);

        if !show_999_proto_err {
            if first_line.starts_with("LUA ERROR: type mismatch for")
                || first_line.starts_with("LUA ERROR: unfinished bytes")
            {
                continue;
            }
        }

        //  0 未解决， 1 已解决,  -1已解决之后又上报了
        let script = match log.status {
            -1 => {
                format!("\n            <li class=\"yellow-dot\" id=item-{} onclick=\"onClickMenu('{}', event.currentTarget)\">{}{}</li>", log.hash, log.hash, prefix, first_line)
            }
            1 => {
                format!("\n            <li class=\"green-dot\" id=item-{} onclick=\"onClickMenu('{}', event.currentTarget)\">{}{}</li>", log.hash, log.hash, prefix, first_line)
            }
            _ => {
                format!("\n            <li class=\"red-dot\" id=item-{} onclick=\"onClickMenu('{}', event.currentTarget)\">{}{}</li>", log.hash, log.hash, prefix, first_line)
            }
        };

        menu_list_script.push_str(&script);
        menu_list_count += 1;
    }

    let template = include_str!("../html/log_content.html");
    let mut html = template.replace("{MENU_ITEM_CODE}", &menu_list_script);
    if is_admin {
        html = html.replace("{LOG_CONTENT_HEAD_CODE}", "");
    } else {
        html = html.replace(
            "{LOG_CONTENT_HEAD_CODE}",
            &format!("<div class=\"red-button\" onclick=\"removeAllLogs('{}')\" style=\"width: 100px; text-align: center;\"> 删除所有 </div>", log_type),
        );
    }

    Ok(HttpResponse::Ok().content_type("text/html").body(html))
}
