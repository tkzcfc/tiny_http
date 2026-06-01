#!/usr/bin/env python3
import argparse
import hashlib
import json
import random
import shutil
import sqlite3
from datetime import datetime, timedelta, timezone
from pathlib import Path


LOG_TYPES = [
    ("error_999", "999平台"),
    ("error_opt_999", "999新版平台"),
    ("error_pay", "支付平台"),
]

PACKAGES = [
    "com.demo.alpha",
    "com.demo.beta",
    "com.demo.gamma",
    "com.slots789.ow",
    "com.game.center",
]

REGIONS = ["CN", "HK", "SG", "US", "TH", "VN"]
CLI_TYPES = ["launcher_v2", "launcher_v3", "desktop_new"]


def utc_text(dt):
    if dt.tzinfo is not None:
        dt = dt.astimezone(timezone.utc).replace(tzinfo=None)
    return dt.strftime("%Y-%m-%d %H:%M:%S.%f")[:-3]


def utc_now():
    return datetime.now(timezone.utc).replace(tzinfo=None)


def normalize_message(message):
    return message


def log_hash(log_type, message):
    value = f"{normalize_message(message)}-{log_type}".encode("utf-8")
    return hashlib.md5(value).hexdigest()


def connect(db_path):
    conn = sqlite3.connect(db_path)
    conn.execute("PRAGMA journal_mode=WAL")
    conn.execute("PRAGMA synchronous=NORMAL")
    return conn


def reset_db(db_path, backup=True):
    db = Path(db_path)
    related = [db, Path(f"{db}-wal"), Path(f"{db}-shm")]
    if backup and db.exists():
        stamp = datetime.now().strftime("%Y%m%d_%H%M%S")
        backup_path = db.with_name(f"{db.stem}.backup_{stamp}{db.suffix}")
        shutil.copy2(db, backup_path)
        print(f"backup: {backup_path}")
    for path in related:
        if path.exists():
            path.unlink()
            print(f"removed: {path}")


def create_schema(conn):
    statements = [
        """CREATE TABLE IF NOT EXISTS upload_user (
            id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
            package TINYTEXT NOT NULL,
            nav_url TINYTEXT NOT NULL,
            version TEXT NOT NULL,
            logs TEXT NOT NULL,
            user TINYTEXT NOT NULL,
            ip TINYTEXT NOT NULL,
            time TEXT NOT NULL
        )""",
        """CREATE TABLE IF NOT EXISTS upload_log (
            id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
            hash TINYTEXT NOT NULL,
            user_list TEXT NOT NULL,
            first_time TEXT NOT NULL,
            last_time TEXT NOT NULL,
            total_count INTEGER NOT NULL,
            status INTEGER NOT NULL,
            resolution_time TEXT NOT NULL,
            resolved_by_user_id INTEGER,
            resolved_by_username TINYTEXT,
            updated_at TEXT,
            log_type TINYTEXT NOT NULL,
            message TEXT NOT NULL
        )""",
        """CREATE TABLE IF NOT EXISTS upload_statistics_cli_cfg (
            id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
            cli_type TINYTEXT NOT NULL,
            user TINYTEXT NOT NULL,
            package TINYTEXT NOT NULL,
            configuration_info TEXT NOT NULL,
            ip TINYTEXT NOT NULL,
            region TINYTEXT NOT NULL,
            time TEXT NOT NULL,
            bucket_hour TINYTEXT,
            bucket_day TINYTEXT,
            bucket_month TINYTEXT
        )""",
        """CREATE TABLE IF NOT EXISTS upload_log_sources (
            id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
            upload_log_id INTEGER NOT NULL,
            upload_log_hash TEXT NOT NULL,
            upload_user_id INTEGER NOT NULL,
            log_type TEXT NOT NULL,
            package TEXT NOT NULL,
            nav_url TEXT NOT NULL,
            version TEXT NOT NULL,
            user TEXT NOT NULL,
            ip TEXT NOT NULL,
            reported_at TEXT NOT NULL
        )""",
        """CREATE TABLE IF NOT EXISTS upload_user_logs (
            upload_user_id INTEGER NOT NULL PRIMARY KEY,
            logs TEXT NOT NULL,
            created_at TEXT NOT NULL
        )""",
        """CREATE TABLE IF NOT EXISTS upload_log_resolutions (
            id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
            upload_log_id INTEGER NOT NULL,
            upload_log_hash TINYTEXT NOT NULL,
            resolved_by_user_id INTEGER NOT NULL,
            resolved_by_username TINYTEXT NOT NULL,
            resolved_at TEXT NOT NULL
        )""",
        """CREATE TABLE IF NOT EXISTS admin_users (
            id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
            username TINYTEXT NOT NULL UNIQUE,
            password_hash TEXT NOT NULL,
            role TINYTEXT NOT NULL,
            enabled BOOLEAN NOT NULL DEFAULT 1,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            last_login_at TEXT
        )""",
        """CREATE TABLE IF NOT EXISTS audit_logs (
            id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
            actor_user_id INTEGER,
            actor_username TINYTEXT NOT NULL,
            actor_role TINYTEXT NOT NULL,
            action TINYTEXT NOT NULL,
            target_type TINYTEXT NOT NULL,
            target_id TEXT NOT NULL,
            result TINYTEXT NOT NULL,
            ip TINYTEXT NOT NULL,
            user_agent TEXT NOT NULL,
            detail TEXT NOT NULL,
            created_at TEXT NOT NULL
        )""",
        """CREATE TABLE IF NOT EXISTS log_type_mappings (
            id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
            log_type TINYTEXT NOT NULL UNIQUE,
            display_name TINYTEXT NOT NULL,
            enabled BOOLEAN NOT NULL DEFAULT 1,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )""",
        """CREATE TABLE IF NOT EXISTS schema_version (
            version INTEGER NOT NULL PRIMARY KEY,
            description TEXT NOT NULL,
            applied_at TEXT NOT NULL
        )""",
        """CREATE TABLE IF NOT EXISTS client_stats_trend_cache (
            granularity TEXT NOT NULL,
            bucket TEXT NOT NULL,
            cli_type TEXT NOT NULL,
            count INTEGER NOT NULL,
            PRIMARY KEY (granularity, bucket, cli_type)
        )""",
        """CREATE TABLE IF NOT EXISTS client_stats_breakdown_cache (
            kind TEXT NOT NULL,
            bucket TEXT NOT NULL,
            cli_type TEXT NOT NULL,
            name TEXT NOT NULL,
            count INTEGER NOT NULL,
            PRIMARY KEY (kind, bucket, cli_type, name)
        )""",
    ]
    for statement in statements:
        conn.execute(statement)

    indexes = [
        "CREATE INDEX IF NOT EXISTS idx_upload_log_hash ON upload_log(hash)",
        "CREATE INDEX IF NOT EXISTS idx_upload_log_last_time ON upload_log(last_time)",
        "CREATE INDEX IF NOT EXISTS idx_upload_log_type_last_time ON upload_log(log_type, last_time)",
        "CREATE INDEX IF NOT EXISTS idx_upload_log_type_status_last_time ON upload_log(log_type, status, last_time)",
        "CREATE INDEX IF NOT EXISTS idx_statistics_time ON upload_statistics_cli_cfg(time)",
        "CREATE INDEX IF NOT EXISTS idx_statistics_cli_type_time ON upload_statistics_cli_cfg(cli_type, time)",
        "CREATE INDEX IF NOT EXISTS idx_client_trend_cache_lookup ON client_stats_trend_cache(granularity, cli_type, bucket)",
        "CREATE INDEX IF NOT EXISTS idx_client_breakdown_cache_lookup ON client_stats_breakdown_cache(kind, cli_type, bucket)",
        "CREATE INDEX IF NOT EXISTS idx_upload_log_sources_log ON upload_log_sources(upload_log_id, reported_at)",
        "CREATE INDEX IF NOT EXISTS idx_upload_log_sources_type_time ON upload_log_sources(log_type, reported_at)",
        "CREATE INDEX IF NOT EXISTS idx_upload_log_sources_reported_at ON upload_log_sources(reported_at)",
        "CREATE INDEX IF NOT EXISTS idx_upload_log_resolutions_log ON upload_log_resolutions(upload_log_id, resolved_at)",
        "CREATE INDEX IF NOT EXISTS idx_audit_logs_created_at ON audit_logs(created_at)",
    ]
    for statement in indexes:
        conn.execute(statement)

    now = utc_text(utc_now())
    conn.execute(
        "INSERT OR IGNORE INTO schema_version(version, description, applied_at) VALUES (?, ?, ?)",
        (1, "initial full schema", now),
    )
    conn.commit()


def seed_log_types(conn):
    now = utc_text(utc_now())
    for log_type, display_name in LOG_TYPES:
        conn.execute(
            """INSERT OR REPLACE INTO log_type_mappings(log_type, display_name, enabled, created_at, updated_at)
               VALUES (?, ?, 1, COALESCE((SELECT created_at FROM log_type_mappings WHERE log_type = ?), ?), ?)""",
            (log_type, display_name, log_type, now, now),
        )


def insert_upload_user(conn, rng, dt, package, user, version):
    logs = "\n".join([
        f"[{utc_text(dt)}] client started",
        f"[{utc_text(dt + timedelta(seconds=3))}] report error context",
        f"user={user} package={package}",
    ])
    ip = f"{rng.randint(11, 223)}.{rng.randint(0, 255)}.{rng.randint(0, 255)}.{rng.randint(1, 254)}"
    cur = conn.execute(
        """INSERT INTO upload_user(package, nav_url, version, logs, user, ip, time)
           VALUES (?, ?, ?, ?, ?, ?, ?)""",
        (package, "https://example.test/nav", version, logs, user, ip, utc_text(dt)),
    )
    user_id = cur.lastrowid
    conn.execute(
        """INSERT OR REPLACE INTO upload_user_logs(upload_user_id, logs, created_at)
           VALUES (?, ?, ?)""",
        (user_id, logs, utc_text(dt)),
    )
    return user_id


def seed_error_logs(conn, count, days, rng):
    now = utc_now()
    buckets = {}
    for index in range(count):
        log_type, _ = rng.choice(LOG_TYPES)
        issue_no = rng.randint(1, max(8, count // 25))
        message = f"RuntimeError: demo issue {issue_no} at module_{issue_no % 5}.rs:42 address 0x{rng.randint(1000, 999999):x}"
        normalized = normalize_message(message)
        digest = log_hash(log_type, normalized)
        dt = now - timedelta(days=rng.randint(0, days - 1), hours=rng.randint(0, 23), minutes=rng.randint(0, 59))
        package = rng.choice(PACKAGES)
        user = str(rng.randint(100000, 999999))
        version = json.dumps({
            "branch": rng.choice(["main", "release", "hotfix"]),
            "patch_time": dt.strftime("%Y-%m-%d"),
            "game_id": rng.randint(-1, 5),
        }, ensure_ascii=True)
        user_id = insert_upload_user(conn, rng, dt, package, user, version)

        key = digest
        if key not in buckets:
            buckets[key] = {
                "hash": digest,
                "user_ids": [],
                "first_time": dt,
                "last_time": dt,
                "status": rng.choice([0, 0, 0, 1]),
                "log_type": log_type,
                "message": normalized,
            }
        item = buckets[key]
        item["user_ids"].append(str(user_id))
        item["first_time"] = min(item["first_time"], dt)
        item["last_time"] = max(item["last_time"], dt)
        if item["status"] == 1 and rng.random() < 0.15:
            item["status"] = -1

    for item in buckets.values():
        resolved_user_id = 1 if item["status"] == 1 else None
        resolved_username = "fc" if item["status"] == 1 else None
        cur = conn.execute(
            """INSERT INTO upload_log(hash, user_list, first_time, last_time, total_count, status,
                                      resolution_time, resolved_by_user_id, resolved_by_username,
                                      updated_at, log_type, message)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)""",
            (
                item["hash"],
                ",".join(item["user_ids"][:120]),
                utc_text(item["first_time"]),
                utc_text(item["last_time"]),
                len(item["user_ids"]),
                item["status"],
                utc_text(item["last_time"]),
                resolved_user_id,
                resolved_username,
                utc_text(item["last_time"]),
                item["log_type"],
                item["message"],
            ),
        )
        log_id = cur.lastrowid
        if item["status"] == 1:
            conn.execute(
                """INSERT INTO upload_log_resolutions(
                       upload_log_id, upload_log_hash, resolved_by_user_id,
                       resolved_by_username, resolved_at
                   )
                   VALUES (?, ?, ?, ?, ?)""",
                (log_id, item["hash"], 1, "fc", utc_text(item["last_time"])),
            )
        conn.execute(
            """INSERT OR IGNORE INTO upload_log_sources(
                   upload_log_id, upload_log_hash, upload_user_id, log_type,
                   package, nav_url, version, user, ip, reported_at
               )
               SELECT ?, ?, u.id, ?, u.package, u.nav_url, u.version, u.user, u.ip, u.time
               FROM upload_user u
               WHERE u.id IN (%s)""" % ",".join(["?"] * len(item["user_ids"])),
            (log_id, item["hash"], item["log_type"], *[int(value) for value in item["user_ids"]]),
        )


def seed_client_stats(conn, count, days, rng):
    now = utc_now()
    rows = []
    for _ in range(count):
        dt = now - timedelta(days=rng.randint(0, days - 1), hours=rng.randint(0, 23), minutes=rng.randint(0, 59))
        cli_type = rng.choice(CLI_TYPES)
        package = rng.choice(PACKAGES)
        user = str(rng.randint(100000, 999999))
        region = rng.choice(REGIONS)
        info = json.dumps({"os": rng.choice(["win", "mac", "linux"]), "first_report": True})
        ip = f"{rng.randint(11, 223)}.{rng.randint(0, 255)}.{rng.randint(0, 255)}.{rng.randint(1, 254)}"
        time_text = utc_text(dt)
        rows.append((
            cli_type,
            user,
            package,
            info,
            ip,
            region,
            time_text,
            f"{time_text[:13]}:00:00",
            time_text[:10],
            time_text[:7],
        ))
    conn.executemany(
        """INSERT INTO upload_statistics_cli_cfg(
               cli_type, user, package, configuration_info, ip, region, time,
               bucket_hour, bucket_day, bucket_month
           )
           VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)""",
        rows,
    )


def rebuild_client_cache(conn):
    conn.execute("DELETE FROM client_stats_trend_cache")
    conn.execute("DELETE FROM client_stats_breakdown_cache")
    trend_sql = [
        ("hour", "substr(time, 1, 13) || ':00:00'"),
        ("day", "substr(time, 1, 10)"),
        ("month", "substr(time, 1, 7)"),
    ]
    for granularity, expr in trend_sql:
        conn.execute(
            f"""INSERT OR REPLACE INTO client_stats_trend_cache(granularity, bucket, cli_type, count)
                SELECT ?, {expr}, cli_type, COUNT(*)
                FROM upload_statistics_cli_cfg GROUP BY {expr}, cli_type""",
            (granularity,),
        )
        conn.execute(
            f"""INSERT OR REPLACE INTO client_stats_trend_cache(granularity, bucket, cli_type, count)
                SELECT ?, {expr}, '', COUNT(*)
                FROM upload_statistics_cli_cfg GROUP BY {expr}""",
            (granularity,),
        )
    for kind, column in [("region", "region"), ("package", "package")]:
        conn.execute(
            f"""INSERT OR REPLACE INTO client_stats_breakdown_cache(kind, bucket, cli_type, name, count)
                SELECT ?, substr(time, 1, 10), cli_type, {column}, COUNT(*)
                FROM upload_statistics_cli_cfg GROUP BY substr(time, 1, 10), cli_type, {column}""",
            (kind,),
        )
        conn.execute(
            f"""INSERT OR REPLACE INTO client_stats_breakdown_cache(kind, bucket, cli_type, name, count)
                SELECT ?, substr(time, 1, 10), '', {column}, COUNT(*)
                FROM upload_statistics_cli_cfg GROUP BY substr(time, 1, 10), {column}""",
            (kind,),
        )


def print_summary(conn):
    queries = [
        ("upload_log", "SELECT COUNT(*) FROM upload_log"),
        ("upload_user", "SELECT COUNT(*) FROM upload_user"),
        ("upload_log_sources", "SELECT COUNT(*) FROM upload_log_sources"),
        ("upload_user_logs", "SELECT COUNT(*) FROM upload_user_logs"),
        ("upload_log_resolutions", "SELECT COUNT(*) FROM upload_log_resolutions"),
        ("upload_statistics_cli_cfg", "SELECT COUNT(*) FROM upload_statistics_cli_cfg"),
        ("trend_cache", "SELECT COUNT(*) FROM client_stats_trend_cache"),
        ("breakdown_cache", "SELECT COUNT(*) FROM client_stats_breakdown_cache"),
        ("log_types", "SELECT log_type, display_name FROM log_type_mappings ORDER BY log_type"),
    ]
    for name, sql in queries:
        print(f"{name}: {conn.execute(sql).fetchall()}")


def main():
    parser = argparse.ArgumentParser(description="Create/reset local SQLite test data for tiny-http.")
    parser.add_argument("--db", default="data.db", help="SQLite database path")
    parser.add_argument("--reset-db", action="store_true", help="Delete the current db before seeding")
    parser.add_argument("--no-backup", action="store_true", help="Do not backup data.db before --reset-db")
    parser.add_argument("--error-logs", type=int, default=1200, help="Number of upload_log events to generate")
    parser.add_argument("--client-stats", type=int, default=5000, help="Number of client first-report rows to generate")
    parser.add_argument("--days", type=int, default=90, help="Spread generated data across N days")
    parser.add_argument("--seed", type=int, default=20260601, help="Random seed")
    args = parser.parse_args()

    if args.reset_db:
        reset_db(args.db, backup=not args.no_backup)

    rng = random.Random(args.seed)
    conn = connect(args.db)
    try:
        create_schema(conn)
        seed_log_types(conn)
        seed_error_logs(conn, args.error_logs, args.days, rng)
        seed_client_stats(conn, args.client_stats, args.days, rng)
        rebuild_client_cache(conn)
        conn.commit()
        print_summary(conn)
        print("")
        print("done. Start the server with bootstrap admin if needed:")
        print("  cargo run -- --admin-account fc --admin-password fc")
    finally:
        conn.close()


if __name__ == "__main__":
    main()
