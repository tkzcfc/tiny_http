use crate::api::{map_db_err, user_authentication, AppState};
use crate::orm_entities::prelude::UploadStatisticsCliCfg;
use crate::orm_entities::upload_statistics_cli_cfg;
use actix_web::{get, web, HttpRequest, HttpResponse, Responder};
use actix_web_httpauth::extractors::basic::BasicAuth;
use chrono::NaiveDate;
use sea_orm::ColumnTrait;
use sea_orm::EntityTrait;
use sea_orm::QueryFilter;
use std::collections::HashMap;

#[get("/statistics_users/{cli_type:.+}")]
pub async fn statistics_users(
    req: HttpRequest,
    credentials: BasicAuth,
    app_data: web::Data<AppState>,
) -> actix_web::Result<impl Responder> {
    if user_authentication(&req, &credentials, &app_data).await? {
        let cli_type = req.match_info().query("cli_type");

        let data_list = UploadStatisticsCliCfg::find()
            .filter(upload_statistics_cli_cfg::Column::CliType.eq(cli_type))
            .all(app_data.db_pool.get().unwrap())
            .await
            .map_err(map_db_err)?;

        let mut date_player_count = HashMap::new();
        for data in data_list {
            let formatted_date = data.time.format("%Y-%m-%d").to_string();
            let count = date_player_count.entry(formatted_date).or_insert(0);
            *count += 1;
        }

        let mut lines = Vec::new();
        for (date, player_count) in date_player_count {
            let line = format!(
                "
        <tr>
        <td style=\"border: 1px solid black; padding: 8px;\">{}</td>
        <td style=\"border: 1px solid black; padding: 8px;\">{}</td>
        </tr>",
                date, player_count
            );

            lines.push((line, date));
        }

        lines.sort_by(|a, b| {
            let date_a = NaiveDate::parse_from_str(&*a.1, "%Y-%m-%d").unwrap();
            let date_b = NaiveDate::parse_from_str(&*b.1, "%Y-%m-%d").unwrap();
            date_a.cmp(&date_b)
        });

        let table_item_codes: String = lines
            .iter()
            .map(|x| x.0.clone())
            .collect::<Vec<_>>()
            .join("\n");

        let html = include_str!("../html/statistics_users.html");
        let html = html.replace("{TABLE_ITEM_CODE}", &table_item_codes);
        Ok(HttpResponse::Ok().content_type("text/html").body(html))
    } else {
        Ok(HttpResponse::Ok().body("{\"data\": \"ok\"}"))
    }
}
