use actix_files::NamedFile;
use actix_web::{get, web, App, HttpRequest, HttpResponse, HttpServer, Responder, Result};
use actix_web::http::header::LOCATION;
use clap::Parser;
use std::fs;
use std::path::{Path, PathBuf};
use std::fmt::Write as FmtWrite;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// The address to listen on
    #[arg(short, long, default_value = "0.0.0.0")]
    address: String,

    /// The port to listen on
    #[arg(short, long, default_value_t = 8000)]
    port: u16,

    #[arg(short, long, default_value = "./")]
    base_path: String,
}

#[derive(Clone)]
struct AppState {
    base_path: String,
}

#[get("/{filename:.*}")]
async fn serve_file(req: HttpRequest, data: web::Data<AppState>) -> Result<impl Responder> {
    let path: PathBuf = req.match_info().query("filename").parse().unwrap();
    let full_path = PathBuf::from(&data.base_path).join(&path);

    if full_path.is_dir() {
        Ok(HttpResponse::Ok().content_type("text/html").body(generate_directory_listing(&full_path)?))
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
    ).unwrap();

    write!(&mut html, "<h1>Directory listing for {}</h1>", path.display()).unwrap();
    write!(&mut html, "<ul>").unwrap();

    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let entry_path = entry.path();
        let entry_name = entry.file_name();

        if entry_path.is_dir() {
            write!(
                &mut html,
                "<li><a href=\"{}/\">{}/</a></li>",
                entry_name.to_string_lossy(),
                entry_name.to_string_lossy()
            ).unwrap();
        } else {
            write!(
                &mut html,
                "<li><a href=\"{}\">{}</a></li>",
                entry_name.to_string_lossy(),
                entry_name.to_string_lossy()
            ).unwrap();
        }
    }

    write!(&mut html, "</ul></body></html>").unwrap();
    Ok(html)
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
async fn main() -> std::io::Result<()> {
    let args = Args::parse();

    let bind_address = format!("{}:{}", args.address, args.port);
    println!("Starting server at http://{}", bind_address);

    let app_state = AppState {
        base_path: args.base_path,
    };


    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(app_state.clone()))
            .service(serve_file)
    })
        .bind(&bind_address)?
        .run()
        .await
}
