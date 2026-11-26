//! Checking a mod straight from Modrinth: its license first, then its newest Fabric jar
//! when the license allows porting. Behind the `modrinth` feature (it needs the network).

use crate::Report;
use crate::library::Library;
use serde::Serialize;
use std::io::Read;

const USER_AGENT: &str = concat!("hopper-rust/hopper/", env!("CARGO_PKG_VERSION"), " (github.com/hopper-rust/hopper)");
const API: &str = "https://api.modrinth.com/v2";

/// What was checked, and the report.
#[derive(Debug, Serialize)]
pub struct Checked {
    pub slug: String,
    pub title: String,
    pub icon_url: Option<String>,
    /// The version and file checked; missing when the license stopped the check.
    pub version: Option<String>,
    pub file: Option<String>,
    pub report: Report,
}

/// `https://modrinth.com/mod/sodium/versions` or `sodium`: the project's slug or id. None when
/// it isn't one (only letters, digits, `-`, `_` and `.` can be).
pub fn slug(target: &str) -> Option<&str> {
    let t = target.trim().trim_end_matches('/');
    let s = match t.split_once("modrinth.com/") {
        Some((_, rest)) => rest.split('/').nth(1)?,
        None => t,
    };
    let s = s.split(['?', '#']).next()?;
    let ok = !s.is_empty() && s.len() <= 64 && s.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c));
    ok.then_some(s)
}

/// A Minecraft version as Modrinth names them (`26.3`, `26.3-rc-2`).
fn game_ok(g: &str) -> bool {
    !g.is_empty() && g.len() <= 32 && g.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
}

fn get(url: &str) -> Result<ureq::Response, String> {
    static AGENT: std::sync::LazyLock<ureq::Agent> = std::sync::LazyLock::new(|| {
        ureq::AgentBuilder::new()
            .user_agent(USER_AGENT)
            .timeout_connect(std::time::Duration::from_secs(10))
            .timeout(std::time::Duration::from_secs(60))
            .build()
    });
    AGENT.get(url).call().map_err(|e| match e {
        ureq::Error::Status(404, _) => "not found on Modrinth".to_string(),
        ureq::Error::Status(code, _) => format!("Modrinth answered {code}"),
        _ => "couldn't reach Modrinth".to_string(),
    })
}

fn json(url: &str) -> Result<serde_json::Value, String> {
    let body = get(url)?.into_string().map_err(|e| e.to_string())?;
    serde_json::from_str(&body).map_err(|e| e.to_string())
}

/// Checks `target` (a Modrinth link, slug or id), for Minecraft `game` when given. Jars larger
/// than `max_jar` bytes aren't downloaded.
pub fn check(library: &Library, target: &str, game: Option<&str>, max_jar: u64) -> Result<Checked, String> {
    fetch(library, target, game, max_jar).map(|f| f.checked)
}

/// What [`fetch`] got: the check, the project's license, and the jar when it was downloaded.
pub struct Fetched {
    pub checked: Checked,
    pub license: String,
    pub jar: Option<Vec<u8>>,
    /// The project's source code link, when it gives one.
    pub source_url: Option<String>,
}

/// [`check`], keeping the jar (for the generator).
pub fn fetch(library: &Library, target: &str, game: Option<&str>, max_jar: u64) -> Result<Fetched, String> {
    match resolve(target, game, max_jar)? {
        Resolved::Stopped(checked, project) => {
            Ok(Fetched { checked, license: project.license, jar: None, source_url: project.source_url })
        }
        Resolved::Ready(project, version) => {
            let jar = download(&version, max_jar)?;
            let checked = report(library, &project, &version, &jar)?;
            Ok(Fetched { checked, license: project.license, jar: Some(jar), source_url: project.source_url })
        }
    }
}

/// A Modrinth project, as far as Hopper needs it.
#[derive(Clone, Debug)]
pub struct Project {
    pub slug: String,
    pub title: String,
    pub icon_url: Option<String>,
    pub license: String,
    pub source_url: Option<String>,
}

/// The Fabric file of one version of a project.
#[derive(Clone, Debug)]
pub struct Version {
    /// Modrinth's id of the version: letters and digits, the same for as long as it exists.
    pub id: String,
    pub number: String,
    pub file_name: String,
    pub file_url: String,
}

/// What the network says before anything is downloaded.
#[derive(Debug)]
pub enum Resolved {
    /// The license stops the check: the report says why, and nothing is downloaded.
    Stopped(Checked, Project),
    /// The license allows porting: the newest Fabric version to check.
    Ready(Project, Version),
}

/// Looks `target` up on Modrinth: its project, and (when the license allows porting) its newest
/// Fabric version for Minecraft `game`. Two small requests; nothing is downloaded.
pub fn resolve(target: &str, game: Option<&str>, max_jar: u64) -> Result<Resolved, String> {
    let slug = slug(target).ok_or("that isn't a Modrinth link or project name")?;
    if game.is_some_and(|g| !game_ok(g)) {
        return Err("that isn't a Minecraft version".into());
    }
    let p = json(&format!("{API}/project/{slug}"))?;
    if p["project_type"].as_str() != Some("mod") {
        return Err(format!("{slug} is a {}, not a mod", p["project_type"].as_str().unwrap_or("project")));
    }
    let slug = p["slug"].as_str().unwrap_or(slug).to_string();
    let project = Project {
        title: p["title"].as_str().unwrap_or(&slug).to_string(),
        icon_url: p["icon_url"].as_str().map(str::to_string),
        license: p["license"]["id"].as_str().unwrap_or_default().to_string(),
        source_url: p["source_url"].as_str().map(str::to_string),
        slug,
    };
    if let Ok(report) = crate::check(&Library::default(), &project.license, None) {
        let checked = Checked {
            slug: project.slug.clone(),
            title: project.title.clone(),
            icon_url: project.icon_url.clone(),
            version: None,
            file: None,
            report,
        };
        return Ok(Resolved::Stopped(checked, project));
    }
    let mut url = format!("{API}/project/{}/version?loaders=%5B%22fabric%22%5D", project.slug);
    if let Some(g) = game {
        url += &format!("&game_versions=%5B%22{g}%22%5D");
    }
    let versions = json(&url)?;
    let v = versions.as_array().and_then(|v| v.first()).ok_or(match game {
        Some(g) => format!("{} has no Fabric version for {g} on Modrinth", project.title),
        None => format!("{} has no Fabric version on Modrinth", project.title),
    })?;
    let files = v["files"].as_array().ok_or("the version has no files")?;
    let file = files.iter().find(|f| f["primary"].as_bool() == Some(true)).or(files.first()).ok_or("the version has no files")?;
    let file_name = file["filename"].as_str().unwrap_or("mod.jar").to_string();
    let size = file["size"].as_u64().unwrap_or(0);
    if size > max_jar {
        return Err(format!("{file_name} is {} MB, more than Hopper downloads ({} MB)", size >> 20, max_jar >> 20));
    }
    let file_url = file["url"].as_str().ok_or("no file url")?.to_string();
    if !file_url.starts_with("https://cdn.modrinth.com/") {
        return Err("the file isn't on Modrinth's CDN".into());
    }
    let id = v["id"].as_str().unwrap_or_default().to_string();
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err("Modrinth gave a version without a usable id".into());
    }
    let number = v["version_number"].as_str().unwrap_or_default().to_string();
    Ok(Resolved::Ready(project, Version { id, number, file_name, file_url }))
}

/// Downloads a version's jar: at most `max` bytes.
pub fn download(version: &Version, max: u64) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    get(&version.file_url)?.into_reader().take(max + 1).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > max {
        return Err(format!("{} is larger than Hopper downloads", version.file_name));
    }
    Ok(bytes)
}

/// The report for a downloaded jar.
pub fn report(library: &Library, project: &Project, version: &Version, jar: &[u8]) -> Result<Checked, String> {
    let report = crate::check(library, &project.license, Some(jar)).map_err(|e| e.0)?;
    Ok(Checked {
        slug: project.slug.clone(),
        title: project.title.clone(),
        icon_url: project.icon_url.clone(),
        version: Some(version.number.clone()),
        file: Some(version.file_name.clone()),
        report,
    })
}

/// `https://github.com/owner/repo` (maybe with `.git` or a `/tree/...` path): owner and repo.
pub fn github_repo(url: &str) -> Option<(&str, &str)> {
    let rest = url.trim().strip_prefix("https://github.com/")?;
    let mut parts = rest.split('/');
    let (owner, repo) = (parts.next()?, parts.next()?.trim_end_matches(".git"));
    let ok = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c));
    (ok(owner) && ok(repo)).then_some((owner, repo))
}

/// The Java files of a GitHub repository's default branch: path and text, without tests and
/// without Forge or NeoForge folders of a multi-loader project. At most `max` bytes are downloaded.
pub fn java_sources(source_url: &str, max: u64) -> Result<Vec<(String, String)>, String> {
    let (owner, repo) = github_repo(source_url).ok_or("Hopper reads source code from GitHub only, for now")?;
    let url = format!("https://codeload.github.com/{owner}/{repo}/zip/HEAD");
    let mut bytes = Vec::new();
    get(&url)
        .map_err(|e| format!("couldn't download the source: {e}"))?
        .into_reader()
        .take(max + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > max {
        return Err("the source is larger than Hopper downloads".into());
    }
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| format!("the source isn't a zip: {e}"))?;
    let mut out = Vec::new();
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).map_err(|e| e.to_string())?;
        // Paths start with the `<repo>-<branch>/` folder.
        let name = f.name().split_once('/').map_or("", |(_, rest)| rest).to_string();
        let skip = name.split('/').any(|seg| matches!(seg, "test" | "tests" | "forge" | "neoforge"));
        if !name.ends_with(".java") || skip || f.size() > 1 << 20 {
            continue;
        }
        let mut text = String::new();
        if f.read_to_string(&mut text).is_ok() {
            out.push((name, text));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{github_repo, slug};

    #[test]
    fn reads_github_links() {
        assert_eq!(github_repo("https://github.com/TerminalMC/CommandKeys/"), Some(("TerminalMC", "CommandKeys")));
        assert_eq!(github_repo("https://github.com/a/b.git"), Some(("a", "b")));
        assert_eq!(github_repo("https://github.com/a/b/tree/main/x"), Some(("a", "b")));
        assert_eq!(github_repo("https://codeberg.org/a/b"), None);
        assert_eq!(github_repo("https://github.com/a/b?x"), None);
    }

    #[test]
    fn reads_links_and_names() {
        assert_eq!(slug("https://modrinth.com/mod/commandkeys"), Some("commandkeys"));
        assert_eq!(slug("https://modrinth.com/mod/commandkeys/versions?l=fabric"), Some("commandkeys"));
        assert_eq!(slug("modrinth.com/mod/sodium/"), Some("sodium"));
        assert_eq!(slug("sodium"), Some("sodium"));
        assert_eq!(slug("a/../b"), None);
        assert_eq!(slug("x y"), None);
        assert_eq!(slug(""), None);
    }
}
