use actix_multipart::{Field, Multipart};
use actix_session::{Session, SessionMiddleware, storage::CookieSessionStore};
use actix_web::{web, App, HttpRequest, HttpResponse, HttpServer, Responder, get};
use actix_web::cookie::Key;
use actix_files::NamedFile;
use futures::{StreamExt, TryStreamExt};
use rand::Rng;
use sanitize_filename::sanitize;
use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Serialize)]
struct FileInfo {
    name: String,
    is_directory: bool,
    path: String,
}

#[derive(serde::Deserialize, Serialize)]
struct LoginForm {
    username: String,
    password: String,
}

async fn login(
    session: Session,
    query: web::Query<HashMap<String, String>>,
) -> impl Responder {
    if let Ok(Some(true)) = session.get::<bool>("authenticated") {
        return HttpResponse::SeeOther()
            .append_header(("Location", "/"))
            .finish();
    }

    let mut html = fs::read_to_string("./static/login.html").unwrap_or_else(|_| String::from(
        r#"<html><body>
            <form action="/login" method="post">
                <input name="username" placeholder="Username">
                <input type="password" name="password" placeholder="Password">
                <button type="submit">Login</button>
            </form>
            <!--ERROR-->
        </body></html>"#,
    ));

    if query.contains_key("error") {
        html = html.replace("<!--ERROR-->", "<p style='color:red'>Invalid credentials!</p>");
    }

    HttpResponse::Ok().content_type("text/html").body(html)
}

async fn login_post(
    session: Session,
    form: web::Form<LoginForm>,
) -> impl Responder {
    if form.username == "admin" && form.password == "password" {
        session.insert("authenticated", true)
            .expect("Session insert failed");
        HttpResponse::SeeOther()
            .append_header(("Location", "/"))
            .finish()
    } else {
        HttpResponse::SeeOther()
            .append_header(("Location", "/login?error=1"))
            .finish()
    }
}

async fn logout(session: Session) -> impl Responder {
    session.remove("authenticated");
    HttpResponse::SeeOther()
        .append_header(("Location", "/login"))
        .finish()
}

fn generate_unique_path(mut path: PathBuf) -> PathBuf {
    let original_path = path.clone();
    let mut counter = 1;

    while path.exists() {
        let stem = original_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        let extension = original_path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("");

        let new_filename = if extension.is_empty() {
            format!("{} ({})", stem, counter)
        } else {
            format!("{} ({}).{}", stem, counter, extension)
        };

        path = original_path.with_file_name(new_filename);
        counter += 1;
    }
    path
}

async fn handle_upload(mut field: Field) -> Result<(), actix_web::Error> {
    let cd = field.content_disposition();
    let filename = cd.get_filename().unwrap_or_default();

    if filename.is_empty() {
        return Ok(());
    }

    let components: Vec<&str> = filename.split('/').collect();
    let sanitized_components: Vec<String> = components
        .iter()
        .map(|&c| sanitize(c))
        .filter(|s| !s.is_empty())
        .collect();

    if sanitized_components.is_empty() {
        return Ok(());
    }

    let base_path = Path::new("/home/pi/Downloads");
    let mut full_path = base_path.to_path_buf();
    for component in &sanitized_components[..sanitized_components.len() - 1] {
        full_path.push(component);
    }

    let file_name = &sanitized_components[sanitized_components.len() - 1];
    full_path.push(file_name);
    full_path = generate_unique_path(full_path);

    if let Some(parent) = full_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut f = web::block(move || std::fs::File::create(&full_path)).await??;

    while let Some(chunk) = field.next().await {
        let data = chunk?;
        f = web::block(move || {
            f.write_all(&data)?;
            Ok::<_, std::io::Error>(f) // Explicitly annotate the Result type
        })
        .await??;
    }

    Ok(())
}

#[get("/api/files")]
async fn list_files(query: web::Query<HashMap<String, String>>) -> HttpResponse {
    let base_path = Path::new("/home/pi/Downloads");
    let provided_path = query.get("path").map(|s| s.as_str()).unwrap_or("");

    // Construct full path and validate it
    let mut current_path = base_path.join(provided_path);
    if let Ok(canonical_path) = current_path.canonicalize() {
        if !canonical_path.starts_with(base_path) {
            return HttpResponse::BadRequest().body("Invalid path");
        }
        current_path = canonical_path;
    } else {
        return HttpResponse::NotFound().body("Path not found");
    }

    // Ensure we're working with a directory
    if !current_path.is_dir() {
        return HttpResponse::BadRequest().body("Not a directory");
    }

    let mut files_data = Vec::new();
    if let Ok(entries) = fs::read_dir(&current_path) {
        for entry in entries.flatten() {
            let entry_path = entry.path();
            let relative_path = entry_path.strip_prefix(base_path)
                .unwrap()
                .to_str()
                .unwrap()
                .to_string();

            files_data.push(FileInfo {
                name: entry.file_name().to_string_lossy().to_string(),
                is_directory: entry_path.is_dir(),
                path: relative_path,
            });
        }
    }

    HttpResponse::Ok().json(files_data)
}

async fn serve_static_file(req: HttpRequest) -> actix_web::Result<NamedFile> {
    let path = req.match_info().query("filename");
    let file_path = format!("./static/{}", path);
    
    match NamedFile::open_async(file_path).await {
        Ok(file) => Ok(file),
        Err(_) => {
            // Fallback to index.html for SPA routing
            NamedFile::open_async("./static/index.html").await
                .map_err(|_| actix_web::error::ErrorNotFound("File not found"))
        }
    }
}

async fn index(
    session: Session,
    req: HttpRequest,
    query: web::Query<HashMap<String, String>>,
    payload: Option<Multipart>,
) -> impl Responder {
    if let Ok(Some(true)) = session.get::<bool>("authenticated") {
        // Handle POST requests (file uploads)
        if req.method() == "POST" {
            let mut files_uploaded = false;
            if let Some(mut multipart) = payload {
                while let Ok(Some(field)) = multipart.try_next().await {
                    if handle_upload(field).await.is_ok() {
                        files_uploaded = true;
                    }
                }
            }

            return if files_uploaded {
                HttpResponse::Ok().body("Files uploaded successfully")
            } else {
                HttpResponse::BadRequest().content_type("text/html").body(
                    r#"<html><body>Error: No files selected. <a href="/">Go back</a></body></html>"#,
                )
            };
        }

        // Handle file downloads
        if let Some(file_name) = query.get("download") {
            return download_file(file_name.clone()).await;
        }

        // Read HTML template
        let html = fs::read_to_string("./static/index.html").unwrap_or_else(|_| String::from(
            r#"<!DOCTYPE html>
            <html>
                <!-- Your HTML content here -->
            </html>"#,
        ));

        HttpResponse::Ok().content_type("text/html").body(html)
    } else {
        HttpResponse::Found()
            .append_header(("Location", "/login"))
            .finish()
    }
}

async fn download_file(file_name: String) -> HttpResponse {
    let file_path = Path::new("/home/pi/Downloads").join(&file_name);

    if file_path.is_file() {
        match fs::read(&file_path) {
            Ok(content) => HttpResponse::Ok()
                .content_type("application/octet-stream")
                .append_header((
                    "Content-Disposition",
                    format!("attachment; filename=\"{}\"", file_name),
                ))
                .body(content),
            Err(_) => HttpResponse::InternalServerError().body("Error reading file"),
        }
    } else {
        HttpResponse::NotFound().body("File not found")
    }
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // Generate a valid 64-byte secret key for session management
    let mut secret_key = [0u8; 64];
    rand::thread_rng().fill(&mut secret_key);

    HttpServer::new(move || {
        App::new()
            .wrap(
                SessionMiddleware::builder(
                    CookieSessionStore::default(),
                    Key::from(&secret_key)
                )
                .cookie_secure(false)
                .build()
            )
            .service(
                web::resource("/static/{filename:.*}")
                    .route(web::get().to(serve_static_file))
            )
            .service(list_files) // Add the files API endpoint
            .route("/", web::get().to(index))
            .route("/", web::post().to(index))
            .route("/login", web::get().to(login))
            .route("/login", web::post().to(login_post))
            .route("/logout", web::get().to(logout))
            .default_service(web::route().to(|| async { 
                HttpResponse::NotFound().body("404 Not Found") 
            }))
    })
    .bind(("0.0.0.0", 8080))?
    .run()
    .await
}
