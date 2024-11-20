mod api;
mod orm_entities;

use crate::api::AppState;
use crate::orm_entities::{upload_log, upload_statistics_cli_cfg, upload_user};
use actix_web::{web, App, HttpServer};
use clap::Parser;
use sea_orm::sea_query::SqliteQueryBuilder;
use sea_orm::{ConnectOptions, ConnectionTrait, Database, Schema, Statement};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::OnceCell;

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

    #[arg(long, default_value = "")]
    admin_account: String,

    #[arg(long, default_value = "")]
    admin_password: String,
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
    db_pool
        .execute(Statement::from_string(
            backend,
            schema
                .create_table_from_entity(upload_statistics_cli_cfg::Entity)
                .if_not_exists()
                .to_string(SqliteQueryBuilder),
        ))
        .await?;

    println!("Starting server at http://{}", args.listen_addr);

    let app_state = AppState {
        username: Arc::new(args.username.to_owned()),
        password: Arc::new(args.password.to_owned()),
        admin_account: Arc::new(args.admin_account.to_owned()),
        admin_password: Arc::new(args.admin_password.to_owned()),
        db_pool: Arc::new(OnceCell::const_new_with(db_pool)),
    };

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(app_state.clone()))
            .service(api::log::api_upload_log)
            .service(api::log::api_log_content)
            .service(api::log::api_user_log)
            .service(api::log::api_log_complete)
            .service(api::log::api_log_remove)
            .service(api::log::api_clear_log)
            .service(api::log_html::log_content)
            .service(api::statistics::api_upload_statistics)
            .service(api::statistics_html::statistics_users)
            .service(api::log_html::index)
    })
    .bind(&args.listen_addr)?
    .run()
    .await?;

    Ok(())
}
