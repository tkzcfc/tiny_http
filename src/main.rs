use actix_files::NamedFile;
use actix_web::http::header::LOCATION;
use actix_web::{get, post, web, App, HttpRequest, HttpResponse, HttpServer, Responder, Result};
use actix_web_httpauth::extractors::basic::{BasicAuth, Config};
use anyhow::anyhow;
use chrono::Local;
use clap::Parser;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::fmt::Write as FmtWrite;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// The address to listen on
    #[arg(long, default_value = "0.0.0.0")]
    address: String,

    /// The port to listen on
    #[arg(long, default_value_t = 8000)]
    port: u16,

    #[arg(long, default_value = "./")]
    save_path: String,

    #[arg(long, default_value = "")]
    username: String,

    #[arg(long, default_value = "")]
    password: String,
}

#[derive(Clone)]
struct AppState {
    save_path: String,
    username: String,
    password: String,
    file_lock: Arc<RwLock<HashMap<String, Arc<Mutex<bool>>>>>,
}

#[get("/{filename:.*}")]
async fn serve_file(
    req: HttpRequest,
    credentials: BasicAuth,
    data: web::Data<AppState>,
) -> Result<impl Responder> {
    if !data.username.is_empty() || !data.password.is_empty() {
        let username = credentials.user_id();
        let password = credentials.password().unwrap_or_default();

        // 验证用户名和密码
        if username != data.username || password != data.password {
            let config = req.app_data::<Config>().cloned().unwrap_or_default();
            return Err(actix_web_httpauth::extractors::AuthenticationError::from(config).into());
        }
    }

    let path: PathBuf = req.match_info().query("filename").parse().unwrap();
    let full_path = PathBuf::from(&data.save_path).join(&path);

    if full_path.is_dir() {
        Ok(HttpResponse::Ok()
            .content_type("text/html")
            .body(generate_directory_listing(&full_path)?))
    } else {
        match NamedFile::open(&full_path) {
            Ok(file) => Ok(file.into_response(&req)),
            Err(_) => {
                // Redirect to the homepage if the file is not found
                Ok(HttpResponse::Found()
                    .append_header((LOCATION, "/"))
                    .finish())
            }
        }
    }
}

fn generate_directory_listing(path: &Path) -> std::io::Result<String> {
    let mut html = String::new();
    write!(
        &mut html,
        "<html><head><title>Directory listing for {}</title></head><body>",
        path.display()
    )
    .unwrap();

    write!(
        &mut html,
        "<h1>Directory listing for {}</h1>",
        path.display()
    )
    .unwrap();
    write!(&mut html, "<ul>").unwrap();

    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let entry_path = entry.path();
        let entry_name = entry.file_name();

        if entry_path.is_dir() {
            write!(
                &mut html,
                "<li><a href=\"{}/\">{}/</a></li>",
                entry_name.to_string_lossy(),
                entry_name.to_string_lossy()
            )
            .unwrap();
        } else {
            write!(
                &mut html,
                "<li><a href=\"{}\">{}</a></li>",
                entry_name.to_string_lossy(),
                entry_name.to_string_lossy()
            )
            .unwrap();
        }
    }

    write!(&mut html, "</ul></body></html>").unwrap();
    Ok(html)
}

#[derive(Deserialize, Debug)]
struct UploadLogData {
    // 日志类型
    log_type: String,
    // 消息
    message: String,
    // 用户
    user: String,
    // 包名
    package: String,
    // 导航服
    nav_url: String,
    // 版本信息
    version: String,
}

#[derive(Deserialize, Serialize)]
struct UploadUser {
    // 用户
    user: String,
    // 包名信息
    package: String,
    // 导航信息
    nav_url: String,
    // 版本信息
    version: String,
    // 上报次数
    count: u32,
    // 首次上报时间
    first_time: String,
    // 最后一次上报时间
    last_time: String,
    // ip
    ip: String,
}

#[derive(Deserialize, Serialize)]
struct LogData {
    message: String,
    // 上报的用户信息列表（最多保存100条）
    upload_users: Vec<UploadUser>,
    // 首次上报时间
    first_time: String,
    // 最后一次上报时间
    last_time: String,
    // 上报次数
    total_count: u32,
}

#[post("/api/upload_log")]
async fn upload_log(
    req: HttpRequest,
    data: web::Data<AppState>,
    log_data: web::Json<UploadLogData>,
) -> Result<HttpResponse> {
    // println!("{:?}", req);
    // println!("{:?}", log_data);

    if log_data.log_type.starts_with("error") {
        // 计算错误内容哈希值
        let digest = md5::compute(&log_data.message);
        let hash_string = format!("{:x}.txt", digest);

        let mut is_new = false;
        let lock = if let Some(lock) = data.file_lock.read().await.get(&hash_string) {
            lock.clone()
        } else {
            is_new = true;
            Arc::new(Mutex::new(true))
        };

        if is_new {
            data.file_lock
                .write()
                .await
                .insert(hash_string.clone(), lock.clone());
        }

        let lock = lock.lock().await;
        // 保存文件路径
        let dir_name = Path::new(&data.save_path).join(&log_data.log_type);
        if !dir_name.exists() || !dir_name.is_dir() {
            std::fs::create_dir_all(dir_name.clone())?;
        }
        let save_to_file_name = dir_name.join(hash_string.clone());

        let fmt = "%Y-%m-%d %H:%M:%S";
        let now = Local::now().format(fmt).to_string();

        let mut data = if save_to_file_name.exists() && save_to_file_name.is_file() {
            serde_json::from_reader(std::fs::File::open(save_to_file_name.clone())?)?
        } else {
            LogData {
                message: log_data.message.clone(),
                upload_users: vec![],
                first_time: now.clone(),
                last_time: "".to_string(),
                total_count: 0,
            }
        };

        data.total_count = data.total_count + 1;
        data.last_time = now.clone();

        let mut is_exists = false;
        for it in &mut data.upload_users {
            if it.user == log_data.user
                && it.version == log_data.version
                && it.nav_url == log_data.nav_url
            {
                it.last_time = now.clone();
                it.count = it.count + 1;
                is_exists = true;
                break;
            }
        }

        if !is_exists && data.upload_users.len() < 50 {
            let ip = if let Some(x) = req.connection_info().realip_remote_addr() {
                x.to_string()
            } else {
                "unknown".to_string()
            };

            data.upload_users.push(UploadUser {
                user: log_data.user.clone(),
                package: log_data.package.clone(),
                nav_url: log_data.nav_url.clone(),
                version: log_data.version.clone(),
                count: 1,
                first_time: now.clone(),
                last_time: now.clone(),
                ip,
            })
        }

        let json_str = serde_json::to_string_pretty(&data)?;
        let mut file = std::fs::File::create(save_to_file_name)?;
        file.write_all(json_str.as_bytes())?;

        drop(lock);
    }

    Ok(HttpResponse::Ok().body("{\"data\": \"ok\"}"))
}

/// # 更简单的方式： https://actix.rs/docs/static-files
/// ```
/// use actix_files as fs;
/// use actix_web::{App, HttpServer};
///
/// #[actix_web::main]
/// async fn main() -> std::io::Result<()> {
///     HttpServer::new(|| App::new().service(fs::Files::new("/static", ".").show_files_listing()))
///         .bind(("127.0.0.1", 8080))?
///         .run()
///         .await
/// }
/// ```
#[actix_web::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let bind_address = format!("{}:{}", args.address, args.port);
    println!("Starting server at http://{}", bind_address);

    let mut app_state = AppState {
        save_path: args.save_path,
        username: args.username,
        password: args.password,
        file_lock: Arc::new(RwLock::new(HashMap::new())),
    };

    if !Path::new(&app_state.save_path).is_absolute() {
        let current_dir = env::current_dir()?;
        let mut save_path = app_state.save_path.clone();
        if save_path.starts_with("./") {
            save_path = (&save_path[2..]).to_string();
        }
        if let Some(path) = current_dir.join(save_path).to_str() {
            app_state.save_path = path.to_string();
        } else {
            return Err(anyhow!("path '{}' conversion failed", app_state.save_path));
        }
    }

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(app_state.clone()))
            .service(serve_file)
            .service(upload_log)
    })
    .bind(&bind_address)?
    .run()
    .await?;

    Ok(())
}
