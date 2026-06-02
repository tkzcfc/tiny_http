use crate::api::{map_db_err, require_admin, require_user, utc_now_millis, AppState};
use crate::orm_entities::upload_statistics_cli_cfg;
use actix_session::Session;
use actix_web::{get, post, web, HttpRequest, HttpResponse};
use chrono::{Duration, TimeZone, Utc};
use sea_orm::ActiveValue::Set;
use sea_orm::{ActiveModelTrait, ConnectionTrait, NotSet, Statement, Value};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Debug)]
struct UploadStatisticsCliCfgData {
    cli_type: String,
    user: String,
    package: String,
    configuration_info: String,
    region: String,
}

#[post("/api/upload_statistics_cli_cfg")]
pub async fn api_upload_statistics(
    req: HttpRequest,
    app_data: web::Data<AppState>,
    json_data: web::Json<UploadStatisticsCliCfgData>,
) -> actix_web::Result<HttpResponse> {
    let ip = req
        .connection_info()
        .realip_remote_addr()
        .unwrap_or("unknown")
        .to_string();

    let now = utc_now_millis();
    let time_text = now.to_string();
    let data = upload_statistics_cli_cfg::ActiveModel {
        id: NotSet,
        cli_type: Set(json_data.cli_type.to_owned()),
        user: Set(json_data.user.to_owned()),
        package: Set(json_data.package.to_owned()),
        configuration_info: Set(json_data.configuration_info.to_owned()),
        ip: Set(ip),
        region: Set(json_data.region.to_owned()),
        time: Set(now),
        bucket_hour: Set(Some(format!(
            "{}:00:00",
            &time_text[..13.min(time_text.len())]
        ))),
        bucket_day: Set(Some(time_text[..10.min(time_text.len())].to_string())),
        bucket_month: Set(Some(time_text[..7.min(time_text.len())].to_string())),
    };
    data.save(app_data.db_pool.as_ref())
        .await
        .map_err(map_db_err)?;
    increment_client_stats_cache(
        &app_data,
        &json_data.cli_type,
        &json_data.region,
        &json_data.package,
        &time_text,
    )
    .await?;

    Ok(HttpResponse::Ok().json(serde_json::json!({ "data": "ok" })))
}

#[derive(Deserialize)]
pub struct StatsQuery {
    from: Option<i64>,
    to: Option<i64>,
    range: Option<String>,
    granularity: Option<String>,
    limit: Option<u32>,
    log_type: Option<String>,
    cli_type: Option<String>,
    source_kind: Option<String>,
    client_kind: Option<String>,
    include_source_tops: Option<bool>,
}

#[derive(Serialize)]
struct TrendPoint {
    bucket: String,
    count: i64,
}

#[derive(Serialize)]
struct CountItem {
    name: String,
    count: i64,
}

#[get("/api/admin/stats/errors")]
pub async fn error_stats(
    session: Session,
    app_data: web::Data<AppState>,
    query: web::Query<StatsQuery>,
) -> actix_web::Result<HttpResponse> {
    require_user(&session, &app_data).await?;
    let log_type = query.log_type.as_deref().filter(|value| !value.is_empty());
    let range = normalize_range(
        &app_data,
        &query,
        "upload_log",
        "last_time",
        filter_pair("log_type", log_type),
    )
    .await?;
    let granularity = normalize_granularity(query.granularity.as_deref());
    let limit = query.limit.unwrap_or(10).clamp(1, 50);
    let type_filter = if log_type.is_some() {
        " AND log_type = ?"
    } else {
        ""
    };

    let trend = query_pairs(
        &app_data,
        &format!(
            "SELECT {} AS name, COUNT(*) AS count FROM upload_log
             WHERE last_time >= ? AND last_time <= ?{type_filter}
             GROUP BY name ORDER BY name",
            time_bucket_sql("last_time", granularity)
        ),
        range,
        value_for_log_type(log_type),
    )
    .await?
    .into_iter()
    .map(|(bucket, count)| TrendPoint { bucket, count })
    .collect::<Vec<_>>();
    let total_in_range = trend.iter().map(|item| item.count).sum::<i64>();
    let source_user_trend = query_pairs(
        &app_data,
        &format!(
            "SELECT {} AS name, COUNT(DISTINCT user) AS count FROM upload_log_sources
             WHERE reported_at >= ? AND reported_at <= ?{type_filter}
               AND COALESCE(user, '') <> ''
             GROUP BY name ORDER BY name",
            time_bucket_sql("reported_at", granularity)
        ),
        range,
        value_for_log_type(log_type),
    )
    .await?
    .into_iter()
    .map(|(bucket, count)| TrendPoint { bucket, count })
    .collect::<Vec<_>>();

    let include_source_tops = query.include_source_tops.unwrap_or(false);
    let source_tops = if include_source_tops {
        Some(error_source_tops(&app_data, range, limit, log_type).await?)
    } else {
        None
    };

    let mut response = serde_json::json!({
        "range": { "from": range.0, "to": range.1 },
        "granularity": granularity,
        "log_type": log_type,
        "total_in_range": total_in_range,
        "trend": trend,
        "source_user_trend": source_user_trend,
    });

    if let Some(source_tops) = source_tops {
        merge_source_tops(&mut response, source_tops);
    }

    Ok(HttpResponse::Ok().json(response))
}

#[get("/api/admin/stats/error_sources")]
pub async fn error_source_stats(
    session: Session,
    app_data: web::Data<AppState>,
    query: web::Query<StatsQuery>,
) -> actix_web::Result<HttpResponse> {
    require_user(&session, &app_data).await?;
    let log_type = query.log_type.as_deref().filter(|value| !value.is_empty());
    let range = normalize_range(
        &app_data,
        &query,
        "upload_log",
        "last_time",
        filter_pair("log_type", log_type),
    )
    .await?;
    let limit = query.limit.unwrap_or(10).clamp(1, 50);
    let (response_key, column) = source_kind_column(query.source_kind.as_deref());
    let items = error_source_top_count(&app_data, column, range, limit, log_type).await?;

    Ok(HttpResponse::Ok().json(serde_json::json!({
        "range": { "from": range.0, "to": range.1 },
        "log_type": log_type,
        response_key: items,
    })))
}

#[get("/api/admin/stats/clients")]
pub async fn client_stats(
    session: Session,
    app_data: web::Data<AppState>,
    query: web::Query<StatsQuery>,
) -> actix_web::Result<HttpResponse> {
    require_admin(&session, &app_data).await?;
    let cli_type = query.cli_type.as_deref().filter(|value| !value.is_empty());
    let range = normalize_range(
        &app_data,
        &query,
        "upload_statistics_cli_cfg",
        "time",
        filter_pair("cli_type", cli_type),
    )
    .await?;
    let granularity = normalize_granularity(query.granularity.as_deref());

    let trend = client_trend_points(&app_data, range, granularity, cli_type)
        .await?
        .into_iter()
        .map(|(bucket, count)| TrendPoint { bucket, count })
        .collect::<Vec<_>>();

    Ok(HttpResponse::Ok().json(serde_json::json!({
        "range": { "from": range.0, "to": range.1 },
        "granularity": granularity,
        "cli_type": cli_type,
        "trend": trend,
    })))
}

#[get("/api/admin/stats/client_breakdown")]
pub async fn client_breakdown_stats(
    session: Session,
    app_data: web::Data<AppState>,
    query: web::Query<StatsQuery>,
) -> actix_web::Result<HttpResponse> {
    require_admin(&session, &app_data).await?;
    let cli_type = query.cli_type.as_deref().filter(|value| !value.is_empty());
    let range = normalize_range(
        &app_data,
        &query,
        "upload_statistics_cli_cfg",
        "time",
        filter_pair("cli_type", cli_type),
    )
    .await?;
    let limit = query.limit.unwrap_or(10).clamp(1, 50);
    let (response_key, column) = client_kind_column(query.client_kind.as_deref());
    let items = client_breakdown_counts(&app_data, column, range, limit, cli_type).await?;

    Ok(HttpResponse::Ok().json(serde_json::json!({
        "range": { "from": range.0, "to": range.1 },
        "cli_type": cli_type,
        response_key: items,
    })))
}

#[get("/api/admin/stats/client_types")]
pub async fn client_types(
    session: Session,
    app_data: web::Data<AppState>,
) -> actix_web::Result<HttpResponse> {
    require_admin(&session, &app_data).await?;
    let rows = app_data
        .db_pool
        .query_all(Statement::from_sql_and_values(
            app_data.db_pool.get_database_backend(),
            "SELECT cli_type AS name, SUM(count) AS count
             FROM client_stats_trend_cache
             WHERE granularity = 'day' AND cli_type <> ''
             GROUP BY cli_type ORDER BY count DESC, name",
            vec![],
        ))
        .await
        .map_err(map_db_err)?;
    let items = rows
        .into_iter()
        .map(|row| CountItem {
            name: row.try_get("", "name").unwrap_or_default(),
            count: row.try_get("", "count").unwrap_or_default(),
        })
        .collect::<Vec<_>>();

    Ok(HttpResponse::Ok().json(serde_json::json!({ "items": items })))
}

async fn normalize_range(
    app_data: &web::Data<AppState>,
    query: &StatsQuery,
    table: &str,
    time_column: &str,
    filter: Option<(&str, &str)>,
) -> actix_web::Result<(i64, i64)> {
    let now = Utc::now().timestamp();
    let default_to = max_timestamp(app_data, table, time_column, filter)
        .await?
        .unwrap_or(now)
        .min(now);
    let to = query.to.unwrap_or(default_to).min(now);
    let from = if let Some(from) = query.from {
        from
    } else if query.range.as_deref() == Some("all") {
        min_timestamp(app_data, table, time_column, filter)
            .await?
            .unwrap_or(to - Duration::days(30).num_seconds())
    } else {
        to - range_days(query.range.as_deref()) * 24 * 60 * 60
    };
    Ok((from.min(to), to))
}

fn filter_pair<'a>(column: &'a str, value: Option<&'a str>) -> Option<(&'a str, &'a str)> {
    value.map(|value| (column, value))
}

fn range_days(value: Option<&str>) -> i64 {
    match value {
        Some("30d") => 30,
        Some("90d") => 90,
        Some("12m") => 365,
        _ => 30,
    }
}

fn normalize_granularity(value: Option<&str>) -> &'static str {
    match value {
        Some("hour") => "hour",
        Some("month") => "month",
        _ => "day",
    }
}

fn time_bucket_sql(column: &str, granularity: &str) -> String {
    if granularity == "hour" {
        format!("substr({column}, 1, 13) || ':00:00'")
    } else if granularity == "month" {
        format!("substr({column}, 1, 7)")
    } else {
        format!("substr({column}, 1, 10)")
    }
}

fn bucket_from_ts(ts: i64, granularity: &str) -> String {
    let value = Utc
        .timestamp_opt(ts, 0)
        .single()
        .unwrap_or_else(Utc::now)
        .naive_utc()
        .to_string();
    if granularity == "hour" {
        format!("{}:00:00", &value[..13.min(value.len())])
    } else if granularity == "month" {
        value[..7.min(value.len())].to_string()
    } else {
        value[..10.min(value.len())].to_string()
    }
}

async fn client_trend_points(
    app_data: &web::Data<AppState>,
    range: (i64, i64),
    granularity: &str,
    cli_type: Option<&str>,
) -> actix_web::Result<Vec<(String, i64)>> {
    let from_bucket = bucket_from_ts(range.0, granularity);
    let to_bucket = bucket_from_ts(range.1, granularity);
    let cli_type = cli_type.unwrap_or("");
    let rows = app_data
        .db_pool
        .query_all(Statement::from_sql_and_values(
            app_data.db_pool.get_database_backend(),
            "SELECT bucket AS name, count AS count
             FROM client_stats_trend_cache
             WHERE granularity = ? AND cli_type = ? AND bucket >= ? AND bucket <= ?
             ORDER BY bucket",
            vec![
                granularity.to_owned().into(),
                cli_type.to_owned().into(),
                from_bucket.into(),
                to_bucket.into(),
            ],
        ))
        .await
        .map_err(map_db_err)?;

    Ok(rows
        .into_iter()
        .map(|row| {
            let name: String = row.try_get("", "name").unwrap_or_default();
            let count: i64 = row.try_get("", "count").unwrap_or_default();
            (name, count)
        })
        .collect())
}

async fn client_breakdown_counts(
    app_data: &web::Data<AppState>,
    kind: &str,
    range: (i64, i64),
    limit: u32,
    cli_type: Option<&str>,
) -> actix_web::Result<Vec<CountItem>> {
    let from_bucket = bucket_from_ts(range.0, "day");
    let to_bucket = bucket_from_ts(range.1, "day");
    let cli_type = cli_type.unwrap_or("");
    let rows = app_data
        .db_pool
        .query_all(Statement::from_sql_and_values(
            app_data.db_pool.get_database_backend(),
            &format!(
                "SELECT name AS name, SUM(count) AS count
                 FROM client_stats_breakdown_cache
                 WHERE kind = ? AND cli_type = ? AND bucket >= ? AND bucket <= ?
                 GROUP BY name ORDER BY count DESC LIMIT {limit}"
            ),
            vec![
                kind.to_owned().into(),
                cli_type.to_owned().into(),
                from_bucket.into(),
                to_bucket.into(),
            ],
        ))
        .await
        .map_err(map_db_err)?;

    Ok(rows
        .into_iter()
        .map(|row| CountItem {
            name: row.try_get("", "name").unwrap_or_default(),
            count: row.try_get("", "count").unwrap_or_default(),
        })
        .collect())
}

async fn increment_client_stats_cache(
    app_data: &web::Data<AppState>,
    cli_type: &str,
    region: &str,
    package: &str,
    time: &str,
) -> actix_web::Result<()> {
    let hour = format!("{}:00:00", &time[..13.min(time.len())]);
    let day = time[..10.min(time.len())].to_string();
    let month = time[..7.min(time.len())].to_string();
    for (granularity, bucket) in [("hour", hour), ("day", day.clone()), ("month", month)] {
        for value in [cli_type, ""] {
            increment_cache_row(
                app_data,
                "client_stats_trend_cache",
                &["granularity", "bucket", "cli_type"],
                &[granularity, &bucket, value],
            )
            .await?;
        }
    }
    for (kind, name) in [("region", region), ("package", package)] {
        for value in [cli_type, ""] {
            increment_cache_row(
                app_data,
                "client_stats_breakdown_cache",
                &["kind", "bucket", "cli_type", "name"],
                &[kind, &day, value, name],
            )
            .await?;
        }
    }
    Ok(())
}

async fn increment_cache_row(
    app_data: &web::Data<AppState>,
    table: &str,
    columns: &[&str],
    values: &[&str],
) -> actix_web::Result<()> {
    let column_list = columns.join(", ");
    let placeholders = std::iter::repeat("?")
        .take(columns.len())
        .collect::<Vec<_>>()
        .join(", ");
    let conflict_columns = columns.join(", ");
    let sql = format!(
        "INSERT INTO {table}({column_list}, count) VALUES ({placeholders}, 1)
         ON CONFLICT({conflict_columns}) DO UPDATE SET count = count + 1"
    );
    let values = values
        .iter()
        .map(|value| (*value).to_owned().into())
        .collect::<Vec<Value>>();
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

async fn upload_source_top_counts(
    app_data: &web::Data<AppState>,
    column: &str,
    range: (i64, i64),
    limit: u32,
    log_type: Option<&str>,
) -> actix_web::Result<Vec<CountItem>> {
    let type_filter = if log_type.is_some() {
        " AND log_type = ?"
    } else {
        ""
    };
    query_pairs(
        app_data,
        &format!(
            "SELECT {column} AS name, COUNT(*) AS count FROM upload_log_sources
             WHERE reported_at >= ? AND reported_at <= ?{type_filter}
               AND COALESCE({column}, '') <> ''
             GROUP BY name ORDER BY count DESC LIMIT {limit}"
        ),
        range,
        value_for_log_type(log_type),
    )
    .await
    .map(|items| {
        items
            .into_iter()
            .map(|(name, count)| CountItem { name, count })
            .collect()
    })
}

struct ErrorSourceTops {
    top_versions: Vec<CountItem>,
    top_packages: Vec<CountItem>,
    top_users: Vec<CountItem>,
    top_ips: Vec<CountItem>,
}

async fn error_source_tops(
    app_data: &web::Data<AppState>,
    range: (i64, i64),
    limit: u32,
    log_type: Option<&str>,
) -> actix_web::Result<ErrorSourceTops> {
    Ok(ErrorSourceTops {
        top_versions: upload_source_top_counts(app_data, "version", range, limit, log_type).await?,
        top_packages: upload_source_top_counts(app_data, "package", range, limit, log_type).await?,
        top_users: upload_source_top_counts(app_data, "user", range, limit, log_type).await?,
        top_ips: upload_source_top_counts(app_data, "ip", range, limit, log_type).await?,
    })
}

async fn error_source_top_count(
    app_data: &web::Data<AppState>,
    column: &str,
    range: (i64, i64),
    limit: u32,
    log_type: Option<&str>,
) -> actix_web::Result<Vec<CountItem>> {
    upload_source_top_counts(app_data, column, range, limit, log_type).await
}

fn source_kind_column(value: Option<&str>) -> (&'static str, &'static str) {
    match value {
        Some("version") => ("top_versions", "version"),
        Some("user") => ("top_users", "user"),
        Some("ip") => ("top_ips", "ip"),
        _ => ("top_packages", "package"),
    }
}

fn client_kind_column(value: Option<&str>) -> (&'static str, &'static str) {
    match value {
        Some("package") => ("by_package", "package"),
        _ => ("by_region", "region"),
    }
}

fn merge_source_tops(response: &mut serde_json::Value, source_tops: ErrorSourceTops) {
    if let Some(object) = response.as_object_mut() {
        object.insert(
            "top_versions".to_string(),
            serde_json::json!(source_tops.top_versions),
        );
        object.insert(
            "top_packages".to_string(),
            serde_json::json!(source_tops.top_packages),
        );
        object.insert(
            "top_users".to_string(),
            serde_json::json!(source_tops.top_users),
        );
        object.insert(
            "top_ips".to_string(),
            serde_json::json!(source_tops.top_ips),
        );
    }
}

async fn query_pairs(
    app_data: &web::Data<AppState>,
    sql: &str,
    range: (i64, i64),
    extra_values: Vec<Value>,
) -> actix_web::Result<Vec<(String, i64)>> {
    let from = Utc
        .timestamp_opt(range.0, 0)
        .single()
        .unwrap_or_else(Utc::now)
        .naive_utc()
        .to_string();
    let to = Utc
        .timestamp_opt(range.1, 0)
        .single()
        .unwrap_or_else(Utc::now)
        .naive_utc()
        .format("%Y-%m-%d %H:%M:%S.999999999")
        .to_string();

    let mut values = vec![from.into(), to.into()];
    values.extend(extra_values);

    let rows = app_data
        .db_pool
        .query_all(Statement::from_sql_and_values(
            app_data.db_pool.get_database_backend(),
            sql,
            values,
        ))
        .await
        .map_err(map_db_err)?;

    Ok(rows
        .into_iter()
        .map(|row| {
            let name: String = row.try_get("", "name").unwrap_or_default();
            let count: i64 = row.try_get("", "count").unwrap_or_default();
            (name, count)
        })
        .collect())
}

fn value_for_log_type(log_type: Option<&str>) -> Vec<Value> {
    value_for_filter(log_type)
}

fn value_for_filter(value: Option<&str>) -> Vec<Value> {
    value
        .map(|value| vec![value.to_owned().into()])
        .unwrap_or_default()
}

async fn max_timestamp(
    app_data: &web::Data<AppState>,
    table: &str,
    time_column: &str,
    filter: Option<(&str, &str)>,
) -> actix_web::Result<Option<i64>> {
    let where_clause = filter
        .map(|(column, _)| format!(" WHERE {column} = ?"))
        .unwrap_or_default();
    let values = filter
        .map(|(_, value)| vec![value.to_owned().into()])
        .unwrap_or_default();
    let sql = format!(
        "SELECT strftime('%s', {time_column}) AS max_ts
         FROM {table}{where_clause}
         ORDER BY {time_column} DESC LIMIT 1"
    );
    let row = app_data
        .db_pool
        .query_one(Statement::from_sql_and_values(
            app_data.db_pool.get_database_backend(),
            &sql,
            values,
        ))
        .await
        .map_err(map_db_err)?;

    Ok(row.and_then(|row| {
        row.try_get::<String>("", "max_ts")
            .ok()
            .and_then(|value| value.parse::<i64>().ok())
    }))
}

async fn min_timestamp(
    app_data: &web::Data<AppState>,
    table: &str,
    time_column: &str,
    filter: Option<(&str, &str)>,
) -> actix_web::Result<Option<i64>> {
    let where_clause = filter
        .map(|(column, _)| format!(" WHERE {column} = ?"))
        .unwrap_or_default();
    let values = filter
        .map(|(_, value)| vec![value.to_owned().into()])
        .unwrap_or_default();
    let sql = format!(
        "SELECT strftime('%s', {time_column}) AS min_ts
         FROM {table}{where_clause}
         ORDER BY {time_column} ASC LIMIT 1"
    );
    let row = app_data
        .db_pool
        .query_one(Statement::from_sql_and_values(
            app_data.db_pool.get_database_backend(),
            &sql,
            values,
        ))
        .await
        .map_err(map_db_err)?;

    Ok(row.and_then(|row| {
        row.try_get::<String>("", "min_ts")
            .ok()
            .and_then(|value| value.parse::<i64>().ok())
    }))
}
