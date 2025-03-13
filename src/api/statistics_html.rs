use crate::api::{map_db_err, user_authentication, AppState};
use actix_web::{get, web, HttpRequest, HttpResponse, Responder};
use actix_web_httpauth::extractors::basic::BasicAuth;
use sea_orm::{ConnectionTrait, Statement};

#[get("/statistics_users/{cli_type:.+}")]
pub async fn statistics_users(
    req: HttpRequest,
    credentials: BasicAuth,
    app_data: web::Data<AppState>,
) -> actix_web::Result<impl Responder> {
    if user_authentication(&req, &credentials, &app_data).await? {
        let cli_type = req.match_info().query("cli_type");

        let query = format!(
            "SELECT DATE(time) as date, COUNT(*) as player_count
             FROM upload_statistics_cli_cfg
             WHERE cli_type = '{}'
             GROUP BY DATE(time)
             ORDER BY DATE(time)",
            cli_type
        );

        let db = app_data.db_pool.get().unwrap();

        let results: Vec<(String, i64)> = db
            .query_all(Statement::from_sql_and_values(
                db.get_database_backend(),
                &query,
                vec![],
            ))
            .await
            .map_err(map_db_err)?
            .into_iter()
            .map(|row| {
                let date: String = row.try_get("", "date").unwrap();
                let player_count: i64 = row.try_get("", "player_count").unwrap();
                (date, player_count)
            })
            .collect();

        let mut lines = Vec::new();
        for (date, player_count) in results {
            let line = format!(
                "
        <tr>
        <td style=\"border: 1px solid black; padding: 8px;\">{}</td>
        <td style=\"border: 1px solid black; padding: 8px;\">{}</td>
        </tr>",
                date, player_count
            );

            lines.push(line);
        }

        let table_item_codes: String = lines.join("\n");

        let html = include_str!("../html/statistics_users.html");
        let html = html.replace("{TABLE_ITEM_CODE}", &table_item_codes);

        Ok(HttpResponse::Ok().content_type("text/html").body(html))
    } else {
        Ok(HttpResponse::Ok().body("{\"data\": \"ok\"}"))
    }
}
