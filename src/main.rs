use actix_multipart::{Field, Multipart};
use actix_web::{web, App, HttpRequest, HttpResponse, HttpServer, Responder};
use futures::{StreamExt, TryStreamExt};
use sanitize_filename::sanitize;
use serde::Serialize;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Serialize)]
struct FileInfo {
    name: String,
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

async fn index(
    req: HttpRequest,
    query: web::Query<std::collections::HashMap<String, String>>,
    payload: Option<Multipart>,
) -> impl Responder {
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

    if let Some(file_name) = query.get("download") {
        return download_file(file_name.clone()).await;
    }

    let html_content = fs::read_to_string("./static/index.html").unwrap();
    let files_data = list_files().await;
    let html_with_data = html_content.replace(
        "<script>",
        &format!("<script>\nwindow.filesData = {};\n", files_data),
    );

    HttpResponse::Ok()
        .content_type("text/html")
        .body(html_with_data)
}

async fn list_files() -> String {
    let folder_path = Path::new("/home/pi/Downloads");
    let mut files_data = Vec::new();

    if let Ok(entries) = fs::read_dir(folder_path) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                files_data.push(FileInfo {
                    name: entry.file_name().to_string_lossy().to_string(),
                });
            }
        }
    }

    serde_json::to_string(&files_data).unwrap_or_else(|_| "[]".to_string())
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
    HttpServer::new(|| {
        App::new()
            .route("/", web::get().to(index))
            .route("/", web::post().to(index))
            .default_service(
                web::route().to(|| async { HttpResponse::NotFound().body("404 Not Found") }),
            )
    })
    .bind(("127.0.0.1", 8080))?
    .run()
    .await
}
