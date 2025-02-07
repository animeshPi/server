use actix_web::{web, App, HttpResponse, HttpServer, Responder};
use serde::Serialize;
use std::fs;
use std::path::Path;

#[derive(Serialize)]
struct FileInfo {
    name: String,
}

async fn foo() -> impl Responder {
    HttpResponse::NotFound().body("404 Not Found")
}

async fn index(query: web::Query<std::collections::HashMap<String, String>>) -> impl Responder {
    if let Some(file_name) = query.get("download") {
        return download_file(file_name.clone()).await;
    }

    // Read the HTML file content
    let html_content = fs::read_to_string("./static/index.html").unwrap();

    // Call list_files to get the JSON data
    let files_data = list_files().await;

    // Inject the JSON data into the HTML content
    let html_with_data = html_content.replace(
        "<script>",
        &format!(
            "<script>\nwindow.filesData = {};\n",
            files_data
        )
    );

    HttpResponse::Ok()
        .content_type("text/html")
        .body(html_with_data)
}

async fn list_files() -> String {
    let folder_path = Path::new("/home/pi/Downloads");
    let mut files_data = Vec::new();

    if !folder_path.is_dir() {
        return "[]".to_string(); // Return empty JSON array if invalid folder path
    }

    for entry in fs::read_dir(folder_path).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_file() {
            let file_name = entry.file_name().to_string_lossy().to_string();
            files_data.push(FileInfo { name: file_name });
        }
    }

    serde_json::to_string(&files_data).unwrap()
}

async fn download_file(file_name: String) -> HttpResponse {
    let folder_path = Path::new("/home/pi/Downloads");
    let file_path = folder_path.join(&file_name);

    if !file_path.exists() || !file_path.is_file() {
        return HttpResponse::NotFound().body("File not found");
    }

    let file_content = fs::read(&file_path).unwrap();
    HttpResponse::Ok()
        .content_type("application/octet-stream")
        .append_header(("Content-Disposition", format!("attachment; filename=\"{}\"", file_name)))
        .body(file_content)
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    HttpServer::new(|| {
        App::new()
            .route("/", web::get().to(index))
            .default_service(
                web::route().to(foo),
            )
    })
    .bind(("127.0.0.1", 8080))?
    .run()
    .await
}
