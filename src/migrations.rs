use crate::orm_entities::{
    admin_user, audit_log, client_stats_breakdown_cache, client_stats_trend_cache,
    log_type_mapping, schema_version, upload_log, upload_log_resolution, upload_log_source,
    upload_statistics_cli_cfg, upload_user, upload_user_log,
};
use argon2::password_hash::{rand_core::OsRng, SaltString};
use argon2::{Argon2, PasswordHasher};
use chrono::{NaiveDateTime, Utc};
use sea_orm::sea_query::SqliteQueryBuilder;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseConnection, EntityTrait, QueryFilter,
    Schema, Set, Statement,
};

const CURRENT_SCHEMA_VERSION: i32 = 1;

pub async fn run(
    db: &DatabaseConnection,
    bootstrap_admin: Option<(&str, &str)>,
) -> anyhow::Result<()> {
    create_schema_version_table(db).await?;
    let version = current_version(db).await?;
    if version < CURRENT_SCHEMA_VERSION {
        apply_v1(db).await?;
    }

    if let Some((username, password)) = bootstrap_admin {
        if !username.is_empty() && !password.is_empty() {
            bootstrap_admin_user(db, username, password).await?;
        }
    }

    Ok(())
}

async fn create_schema_version_table(db: &DatabaseConnection) -> anyhow::Result<()> {
    create_entity_table(db, schema_version::Entity).await
}

async fn current_version(db: &DatabaseConnection) -> anyhow::Result<i32> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            db.get_database_backend(),
            "SELECT COALESCE(MAX(version), 0) AS version FROM schema_version",
            vec![],
        ))
        .await?;
    Ok(row
        .and_then(|row| row.try_get::<i32>("", "version").ok())
        .unwrap_or(0))
}

async fn apply_v1(db: &DatabaseConnection) -> anyhow::Result<()> {
    create_entity_tables(db).await?;

    let statements = [
        "CREATE INDEX IF NOT EXISTS idx_upload_log_hash ON upload_log(hash)",
        "CREATE INDEX IF NOT EXISTS idx_upload_log_last_time ON upload_log(last_time)",
        "CREATE INDEX IF NOT EXISTS idx_upload_log_type_last_time ON upload_log(log_type, last_time)",
        "CREATE INDEX IF NOT EXISTS idx_upload_log_type_status_last_time ON upload_log(log_type, status, last_time)",
        "CREATE INDEX IF NOT EXISTS idx_upload_log_sources_log ON upload_log_sources(upload_log_id, reported_at)",
        "CREATE INDEX IF NOT EXISTS idx_upload_log_sources_type_time ON upload_log_sources(log_type, reported_at)",
        "CREATE INDEX IF NOT EXISTS idx_upload_log_sources_reported_at ON upload_log_sources(reported_at)",
        "CREATE INDEX IF NOT EXISTS idx_upload_log_resolutions_log ON upload_log_resolutions(upload_log_id, resolved_at)",
        "CREATE INDEX IF NOT EXISTS idx_statistics_time ON upload_statistics_cli_cfg(time)",
        "CREATE INDEX IF NOT EXISTS idx_statistics_cli_type_time ON upload_statistics_cli_cfg(cli_type, time)",
        "CREATE INDEX IF NOT EXISTS idx_client_trend_cache_lookup ON client_stats_trend_cache(granularity, cli_type, bucket)",
        "CREATE INDEX IF NOT EXISTS idx_client_breakdown_cache_lookup ON client_stats_breakdown_cache(kind, cli_type, bucket)",
        "CREATE INDEX IF NOT EXISTS idx_audit_logs_created_at ON audit_logs(created_at)",
    ];

    for statement in statements {
        exec(db, statement).await?;
    }

    exec(
        db,
        "INSERT OR IGNORE INTO schema_version(version, description, applied_at)
         VALUES (1, 'initial full schema', CURRENT_TIMESTAMP)",
    )
    .await?;

    tracing::info!(
        version = CURRENT_SCHEMA_VERSION,
        "database schema initialized"
    );
    Ok(())
}

async fn create_entity_tables(db: &DatabaseConnection) -> anyhow::Result<()> {
    create_entity_table(db, admin_user::Entity).await?;
    create_entity_table(db, audit_log::Entity).await?;
    create_entity_table(db, log_type_mapping::Entity).await?;
    create_entity_table(db, upload_user::Entity).await?;
    create_entity_table(db, upload_log::Entity).await?;
    create_entity_table(db, upload_statistics_cli_cfg::Entity).await?;
    create_entity_table(db, upload_log_resolution::Entity).await?;
    create_entity_table(db, upload_log_source::Entity).await?;
    create_entity_table(db, upload_user_log::Entity).await?;
    create_entity_table(db, client_stats_trend_cache::Entity).await?;
    create_entity_table(db, client_stats_breakdown_cache::Entity).await?;
    Ok(())
}

async fn create_entity_table<E>(db: &DatabaseConnection, entity: E) -> anyhow::Result<()>
where
    E: EntityTrait,
{
    let backend = db.get_database_backend();
    let schema = Schema::new(backend);
    let statement = schema
        .create_table_from_entity(entity)
        .if_not_exists()
        .to_string(SqliteQueryBuilder);
    db.execute(Statement::from_string(backend, statement))
        .await?;
    Ok(())
}

async fn bootstrap_admin_user(
    db: &DatabaseConnection,
    username: &str,
    password: &str,
) -> anyhow::Result<()> {
    use crate::orm_entities::admin_user;
    use crate::orm_entities::prelude::AdminUser;

    let existing = AdminUser::find()
        .filter(admin_user::Column::Role.eq("admin"))
        .one(db)
        .await?;
    if existing.is_some() {
        return Ok(());
    }

    let salt = SaltString::generate(&mut OsRng);
    let password_hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|err| anyhow::anyhow!("failed to hash bootstrap admin password: {err}"))?
        .to_string();
    let now = utc_now_millis();
    admin_user::ActiveModel {
        id: sea_orm::NotSet,
        username: Set(username.to_owned()),
        password_hash: Set(password_hash),
        role: Set("admin".to_string()),
        enabled: Set(true),
        created_at: Set(now),
        updated_at: Set(now),
        last_login_at: Set(None),
    }
    .insert(db)
    .await?;
    tracing::info!(username, "bootstrap admin user created");
    Ok(())
}

async fn exec(db: &DatabaseConnection, sql: &str) -> anyhow::Result<()> {
    db.execute(Statement::from_string(
        db.get_database_backend(),
        sql.to_owned(),
    ))
    .await?;
    Ok(())
}

fn utc_now_millis() -> NaiveDateTime {
    chrono::DateTime::from_timestamp_millis(Utc::now().timestamp_millis())
        .unwrap_or_else(Utc::now)
        .naive_utc()
}
