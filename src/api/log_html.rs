use crate::api::{user_authentication, AppState};
use actix_web::http::header::LOCATION;
use actix_web::{get, web, HttpRequest, HttpResponse, Responder};
use actix_web_httpauth::extractors::basic::BasicAuth;

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

#[get("/log_content/{log_type:.+}")]
pub async fn log_content(
    _req: HttpRequest,
    _credentials: BasicAuth,
    _app_data: web::Data<AppState>,
) -> actix_web::Result<impl Responder> {
    let html = include_str!("../html/log_content.html");
    Ok(HttpResponse::Ok().content_type("text/html").body(html))
}
