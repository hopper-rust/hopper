//! Hopper's website: the page, `GET /api/check?mod=<link or slug>&game=<version>`, which
//! answers the checker's report as JSON, and `GET /api/port` (same query), which answers a zip
//! of the starting Horizon project the generator writes. Results are kept per mod version (see
//! `cache.rs`), so each version is downloaded and checked once. Meant to run behind a reverse
//! proxy that adds HTTPS (see `server/README.md`).

mod cache;

use axum::extract::{Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use cache::Cache;
use hopper_core::library::{Library, Status};
use hopper_core::modrinth::{self, Resolved};
use serde::Deserialize;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::sync::Semaphore;

const PAGE: &str = include_str!("../static/index.html");
const LOGO: &[u8] = include_bytes!("../../assets/logo.png");

/// Downloads and checks running at once; the rest wait their turn.
const AT_ONCE: usize = 4;
/// Jars larger than this aren't downloaded.
const MAX_JAR: u64 = 50 << 20;

struct App {
    library: Library,
    cache: Cache,
    /// Link and Minecraft version → what Modrinth said, for [`cache::LOOKUP`].
    lookups: Mutex<HashMap<String, (Instant, Arc<Resolved>)>>,
    turns: Semaphore,
}

#[derive(Deserialize)]
struct CheckQuery {
    #[serde(rename = "mod")]
    target: String,
    game: Option<String>,
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Check,
    Port,
}

fn error(code: StatusCode, message: impl Into<String>) -> Response {
    (code, Json(serde_json::json!({ "error": message.into() }))).into_response()
}

/// A result, saying whether it was kept from before (`X-Hopper-Cache: hit`) or made now.
fn answer(kind: Kind, body: &[u8], hit: bool) -> Response {
    let cached = ("x-hopper-cache", if hit { "hit" } else { "miss" }.to_string());
    match kind {
        Kind::Check => ([(header::CONTENT_TYPE.as_str(), "application/json".to_string()), cached], body.to_vec()).into_response(),
        Kind::Port => {
            // The zip's top folder is the mod's id.
            let id = zip::ZipArchive::new(std::io::Cursor::new(body))
                .ok()
                .and_then(|mut z| z.by_index(0).ok().map(|f| f.name().split('/').next().unwrap_or("mod").to_string()))
                .unwrap_or_else(|| "mod".into());
            (
                [
                    (header::CONTENT_TYPE.as_str(), "application/zip".to_string()),
                    (header::CONTENT_DISPOSITION.as_str(), format!("attachment; filename=\"{id}-horizon.zip\"")),
                    cached,
                ],
                body.to_vec(),
            )
                .into_response()
        }
    }
}

/// What Modrinth says `slug` is: kept for a while, so a kept result costs no request at all.
async fn lookup(app: &Arc<App>, slug: &str, game: Option<String>) -> Result<Arc<Resolved>, Response> {
    let key = format!("{}|{}", slug.to_ascii_lowercase(), game.as_deref().unwrap_or(""));
    {
        let mut lookups = app.lookups.lock().unwrap_or_else(|e| e.into_inner());
        lookups.retain(|_, (at, _)| at.elapsed() < cache::LOOKUP);
        if let Some((_, r)) = lookups.get(&key) {
            return Ok(r.clone());
        }
    }
    let target = slug.to_string();
    let done = tokio::task::spawn_blocking(move || modrinth::resolve(&target, game.as_deref(), MAX_JAR)).await;
    match done {
        Ok(Ok(r)) => {
            let r = Arc::new(r);
            app.lookups.lock().unwrap_or_else(|e| e.into_inner()).insert(key, (Instant::now(), r.clone()));
            Ok(r)
        }
        Ok(Err(message)) => Err(error(StatusCode::BAD_GATEWAY, message)),
        Err(_) => Err(error(StatusCode::INTERNAL_SERVER_ERROR, "The lookup failed.")),
    }
}

/// `/api/check` and `/api/port`: the kept result for the mod's newest version, or made now.
async fn result(app: Arc<App>, q: CheckQuery, kind: Kind) -> Response {
    let Some(slug) = modrinth::slug(&q.target) else {
        return error(StatusCode::BAD_REQUEST, "That isn't a Modrinth link or project name.");
    };
    let game = q.game.filter(|g| !g.trim().is_empty());
    let resolved = match lookup(&app, slug, game).await {
        Ok(r) => r,
        Err(response) => return response,
    };
    let (project, version) = match &*resolved {
        Resolved::Stopped(checked, _) => {
            return match kind {
                Kind::Check => answer(kind, &serde_json::to_vec(checked).unwrap_or_default(), true),
                Kind::Port => error(StatusCode::UNPROCESSABLE_ENTITY, checked.report.summary.clone()),
            };
        }
        Resolved::Ready(project, version) => (project.clone(), version.clone()),
    };
    let (check_key, port_key) = (
        format!("{}/{}/check.json", project.slug, version.id),
        format!("{}/{}/port.zip", project.slug, version.id),
    );
    let key = if kind == Kind::Check { check_key.clone() } else { port_key.clone() };
    if let Some(body) = app.cache.get(&key) {
        return answer(kind, &body, true);
    }
    // One request does the work; the others wait for it, then find it kept.
    let lock = app.cache.once(&key);
    let _working = lock.lock().await;
    if let Some(body) = app.cache.get(&key) {
        return answer(kind, &body, true);
    }
    let Ok(_turn) = app.turns.acquire().await else {
        return error(StatusCode::SERVICE_UNAVAILABLE, "Hopper is shutting down.");
    };
    let worker = app.clone();
    let done = tokio::task::spawn_blocking(move || -> Result<Arc<Vec<u8>>, String> {
        let jar = modrinth::download(&version, MAX_JAR)?;
        // The report is kept either way: a port's download also answers later checks.
        let checked = modrinth::report(&worker.library, &project, &version, &jar)?;
        let check = worker.cache.put(&check_key, serde_json::to_vec(&checked).map_err(|e| e.to_string())?);
        if kind == Kind::Check {
            return Ok(check);
        }
        let (generated, _note) = hopper_core::port::generate_for(&worker.library, &project, &jar)?;
        let zip = zip_project(&generated).map_err(|e| e.to_string())?;
        Ok(worker.cache.put(&port_key, zip))
    })
    .await;
    match done {
        Ok(Ok(body)) => answer(kind, &body, false),
        Ok(Err(message)) => error(StatusCode::BAD_GATEWAY, message),
        Err(_) => error(StatusCode::INTERNAL_SERVER_ERROR, "The check failed."),
    }
}

async fn check(State(app): State<Arc<App>>, Query(q): Query<CheckQuery>) -> Response {
    result(app, q, Kind::Check).await
}

async fn port(State(app): State<Arc<App>>, Query(q): Query<CheckQuery>) -> Response {
    result(app, q, Kind::Port).await
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
    /// Where results are kept; None keeps them in memory only.
    cache: Option<PathBuf>,
}

fn args() -> Result<Args, String> {
    let mut a = Args { listen: ([127, 0, 0, 1], 8080).into(), library: PathBuf::from("library"), cache: Some("cache".into()) };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut value = || it.next().ok_or(format!("{arg} needs a value"));
        match arg.as_str() {
            "--listen" => a.listen = value()?.parse().map_err(|e| format!("--listen: {e}"))?,
            "--library" => a.library = value()?.into(),
            "--cache" => a.cache = Some(value()?.into()),
            "--no-cache" => a.cache = None,
            _ => {
                return Err(
                    "Usage: hopper-server [--listen 127.0.0.1:8080] [--library ./library] [--cache ./cache | --no-cache]".into()
                );
            }
        }
    }
    Ok(a)
}

#[tokio::main]
async fn main() -> Result<(), String> {
    let a = args()?;
    let library = Library::load(&a.library)?;
    let stamp = cache::stamp(&a.library);
    let kept = a.cache.as_ref().map_or("memory only".to_string(), |d| d.display().to_string());
    println!(
        "hopper-server: {} library entries, results kept in {kept} (stamp {stamp}), listening on http://{}",
        library.entries.len(),
        a.listen
    );
    let app = Arc::new(App {
        library,
        cache: Cache::new(a.cache, stamp),
        lookups: Mutex::default(),
        turns: Semaphore::new(AT_ONCE),
    });
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
