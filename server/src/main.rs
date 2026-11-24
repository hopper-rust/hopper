//! Hopper's website: the page, `GET /api/check?mod=<link or slug>&game=<version>`, which
//! answers the checker's report as JSON, and `GET /api/port` (same query), which answers a zip
//! of the starting Horizon project the generator writes. Reports are kept for an hour, so a mod checked twice is
//! downloaded once. Meant to run behind a reverse proxy that adds HTTPS (see `server/README.md`).

use axum::extract::{Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use hopper_core::library::{Library, Status};
use serde::Deserialize;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;

const PAGE: &str = include_str!("../static/index.html");
const LOGO: &[u8] = include_bytes!("../../assets/logo.png");

/// How long a report is kept.
const KEEP: Duration = Duration::from_secs(60 * 60);
/// At most this many reports are kept; the oldest go first.
const KEEP_AT_MOST: usize = 1000;
/// Checks running at once; the rest wait their turn.
const AT_ONCE: usize = 4;
/// Jars larger than this aren't downloaded.
const MAX_JAR: u64 = 50 << 20;

struct App {
    library: Library,
    kept: Mutex<HashMap<String, (Instant, Arc<String>)>>,
    turns: Semaphore,
}

#[derive(Deserialize)]
struct CheckQuery {
    #[serde(rename = "mod")]
    target: String,
    game: Option<String>,
}

fn error(code: StatusCode, message: impl Into<String>) -> Response {
    (code, Json(serde_json::json!({ "error": message.into() }))).into_response()
}

fn json_body(body: Arc<String>) -> Response {
    ([(header::CONTENT_TYPE, "application/json")], body.as_str().to_owned()).into_response()
}

async fn check(State(app): State<Arc<App>>, Query(q): Query<CheckQuery>) -> Response {
    let Some(slug) = hopper_core::modrinth::slug(&q.target) else {
        return error(StatusCode::BAD_REQUEST, "That isn't a Modrinth link or project name.");
    };
    let game = q.game.filter(|g| !g.trim().is_empty());
    let key = format!("{}|{}", slug.to_ascii_lowercase(), game.as_deref().unwrap_or(""));
    if let Some(body) = kept(&app, &key) {
        return json_body(body);
    }
    let Ok(_turn) = app.turns.acquire().await else {
        return error(StatusCode::SERVICE_UNAVAILABLE, "Hopper is shutting down.");
    };
    // Another request may have checked it while this one waited.
    if let Some(body) = kept(&app, &key) {
        return json_body(body);
    }
    let worker = app.clone();
    let slug = slug.to_string();
    let done = tokio::task::spawn_blocking(move || {
        hopper_core::modrinth::check(&worker.library, &slug, game.as_deref(), MAX_JAR)
    })
    .await;
    match done {
        Ok(Ok(checked)) => {
            let body = Arc::new(serde_json::to_string(&checked).unwrap_or_default());
            keep(&app, key, body.clone());
            json_body(body)
        }
        Ok(Err(message)) => error(StatusCode::BAD_GATEWAY, message),
        Err(_) => error(StatusCode::INTERNAL_SERVER_ERROR, "The check failed."),
    }
}

async fn port(State(app): State<Arc<App>>, Query(q): Query<CheckQuery>) -> Response {
    let Some(slug) = hopper_core::modrinth::slug(&q.target) else {
        return error(StatusCode::BAD_REQUEST, "That isn't a Modrinth link or project name.");
    };
    let game = q.game.filter(|g| !g.trim().is_empty());
    let Ok(_turn) = app.turns.acquire().await else {
        return error(StatusCode::SERVICE_UNAVAILABLE, "Hopper is shutting down.");
    };
    let worker = app.clone();
    let slug = slug.to_string();
    let done = tokio::task::spawn_blocking(move || -> Result<(String, Vec<u8>), String> {
        let ported = hopper_core::port::port(&worker.library, &slug, game.as_deref(), MAX_JAR)?;
        let project = ported.project;
        Ok((project.id.clone(), zip_project(&project).map_err(|e| e.to_string())?))
    })
    .await;
    match done {
        Ok(Ok((id, bytes))) => (
            [
                (header::CONTENT_TYPE, "application/zip".to_string()),
                (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{id}-horizon.zip\"")),
            ],
            bytes,
        )
            .into_response(),
        Ok(Err(message)) => error(StatusCode::BAD_GATEWAY, message),
        Err(_) => error(StatusCode::INTERNAL_SERVER_ERROR, "The port failed."),
    }
}

/// The project's files in a zip, under a folder named after the mod.
fn zip_project(project: &hopper_core::generate::Project) -> zip::result::ZipResult<Vec<u8>> {
    use std::io::Write;
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (path, bytes) in &project.files {
        zip.start_file(format!("{}/{path}", project.id), options)?;
        zip.write_all(bytes)?;
    }
    Ok(zip.finish()?.into_inner())
}

fn kept(app: &App, key: &str) -> Option<Arc<String>> {
    let kept = app.kept.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    kept.get(key).filter(|(at, _)| at.elapsed() < KEEP).map(|(_, body)| body.clone())
}

fn keep(app: &App, key: String, body: Arc<String>) {
    let mut kept = app.kept.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    kept.retain(|_, (at, _)| at.elapsed() < KEEP);
    if kept.len() >= KEEP_AT_MOST {
        if let Some(oldest) = kept.iter().min_by_key(|(_, (at, _))| *at).map(|(k, _)| k.clone()) {
            kept.remove(&oldest);
        }
    }
    kept.insert(key, (Instant::now(), body));
}

/// The library's size, for the page.
async fn library_stats(State(app): State<Arc<App>>) -> Response {
    let count = |s: Status| app.library.entries.iter().filter(|e| e.info.status == s).count();
    Json(serde_json::json!({
        "entries": app.library.entries.len(),
        "direct": count(Status::Direct),
        "partial": count(Status::Partial),
        "missing": count(Status::Missing),
    }))
    .into_response()
}

async fn page() -> Response {
    ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], PAGE).into_response()
}

async fn logo() -> Response {
    ([(header::CONTENT_TYPE, "image/png"), (header::CACHE_CONTROL, "public, max-age=86400")], LOGO).into_response()
}

struct Args {
    listen: SocketAddr,
    library: PathBuf,
}

fn args() -> Result<Args, String> {
    let mut a = Args { listen: ([127, 0, 0, 1], 8080).into(), library: PathBuf::from("library") };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut value = || it.next().ok_or(format!("{arg} needs a value"));
        match arg.as_str() {
            "--listen" => a.listen = value()?.parse().map_err(|e| format!("--listen: {e}"))?,
            "--library" => a.library = value()?.into(),
            _ => return Err("Usage: hopper-server [--listen 127.0.0.1:8080] [--library ./library]".into()),
        }
    }
    Ok(a)
}

#[tokio::main]
async fn main() -> Result<(), String> {
    let a = args()?;
    let library = Library::load(&a.library)?;
    println!("hopper-server: {} library entries, listening on http://{}", library.entries.len(), a.listen);
    let app = Arc::new(App { library, kept: Mutex::default(), turns: Semaphore::new(AT_ONCE) });
    let router = Router::new()
        .route("/", get(page))
        .route("/logo.png", get(logo))
        .route("/api/check", get(check))
        .route("/api/port", get(port))
        .route("/api/library", get(library_stats))
        .with_state(app);
    let listener = tokio::net::TcpListener::bind(a.listen).await.map_err(|e| format!("{}: {e}", a.listen))?;
    axum::serve(listener, router)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .map_err(|e| e.to_string())
}
