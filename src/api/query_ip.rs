use actix_web::get;
use actix_web::HttpRequest;
use actix_web::HttpResponse;
use actix_web::Responder;
use serde::Serialize;
use serde_json::to_string;

#[derive(Serialize, Debug)]
struct QueryIpResponse {
    status: String,
    query: String,
}

#[get("api/query_ip/json")]
pub async fn api_query_ip_json(req: HttpRequest) -> actix_web::Result<impl Responder> {
    let response = QueryIpResponse {
        status: "success".into(),
        query: if let Some(x) = req.connection_info().realip_remote_addr() {
            x.to_string()
        } else {
            "127.0.0.1".into()
        },
    };

    let body = to_string(&response)?;
    Ok(HttpResponse::Ok().body(body))
}
