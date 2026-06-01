mod api;
mod migrations;
mod orm_entities;

use crate::api::AppState;
use actix_files::Files;
use actix_session::{storage::CookieSessionStore, SessionMiddleware};
use actix_web::cookie::Key;
use actix_web::{web, App, HttpServer};
use clap::Parser;
use rand_core::{OsRng, RngCore};
use sea_orm::{ConnectOptions, Database};
use std::sync::Arc;
use std::time::Duration;
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// The address to listen on
    #[arg(long, default_value = "0.0.0.0:8000")]
    listen_addr: String,

    #[arg(long, default_value = "sqlite://data.db?mode=rwc")]
    database_url: String,

    /// Bootstrap admin account. Used only when the database has no admin user.
    #[arg(long, default_value = "")]
    admin_account: String,

    /// Bootstrap admin password. Used only when the database has no admin user.
    #[arg(long, default_value = "")]
    admin_password: String,

    /// Session signing key text. If empty, a random key is generated on startup.
    #[arg(long, default_value = "")]
    session_key: String,

    /// Path to the built Vue admin frontend dist directory.
    #[arg(long, default_value = "./frontend/dist")]
    admin_dist_path: String,
}

#[actix_web::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let args = Args::parse();

    let mut opt = ConnectOptions::new(&args.database_url);
    opt.max_connections(100)
        .min_connections(5)
        .connect_timeout(Duration::from_secs(8))
        .acquire_timeout(Duration::from_secs(8))
        .idle_timeout(Duration::from_secs(60))
        .max_lifetime(Duration::from_secs(600))
        .sqlx_logging(true);

    let db_pool = Database::connect(opt)
        .await
        .expect("Database initialization failed");

    migrations::run(
        &db_pool,
        Some((args.admin_account.as_str(), args.admin_password.as_str())),
    )
    .await?;

    tracing::info!(
        listen_addr = %args.listen_addr,
        admin_dist_path = %args.admin_dist_path,
        "starting server"
    );

    let app_state = AppState {
        db_pool: Arc::new(db_pool),
    };
    let session_key = make_session_key(&args.session_key);
    let admin_dist_path = args.admin_dist_path.clone();

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(app_state.clone()))
            .wrap(
                SessionMiddleware::builder(CookieSessionStore::default(), session_key.clone())
                    .cookie_secure(false)
                    .build(),
            )
            .service(api::auth::login)
            .service(api::auth::logout)
            .service(api::auth::me)
            .service(api::log::api_upload_log)
            .service(api::log::api_log_types)
            .service(api::log::api_save_log_type)
            .service(api::log::api_log_list)
            .service(api::log::api_log_content)
            .service(api::log::api_user_log)
            .service(api::log::api_log_complete)
            .service(api::log::api_log_remove)
            .service(api::log::api_clear_log)
            .service(api::statistics::api_upload_statistics)
            .service(api::statistics::error_stats)
            .service(api::statistics::error_source_stats)
            .service(api::statistics::client_stats)
            .service(api::statistics::client_breakdown_stats)
            .service(api::statistics::client_types)
            .service(api::users::list_users)
            .service(api::users::save_user)
            .service(api::users::audit_logs)
            .service(api::query_ip::api_query_ip_json)
            .service(Files::new("/", admin_dist_path.clone()).index_file("index.html"))
    })
    .bind(&args.listen_addr)?
    .run()
    .await?;

    Ok(())
}

fn make_session_key(value: &str) -> Key {
    if value.is_empty() {
        let mut bytes = [0_u8; 64];
        OsRng.fill_bytes(&mut bytes);
        tracing::warn!("session_key is empty; generated a random key for this server run");
        return Key::from(&bytes);
    }

    let mut bytes = [0_u8; 64];
    for (index, byte) in value.as_bytes().iter().enumerate() {
        bytes[index % bytes.len()] ^= *byte;
    }
    Key::from(&bytes)
}
