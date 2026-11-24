//! Results kept between requests, so a mod version is downloaded and checked once, not once per
//! visitor.
//!
//! - What Modrinth says a link is (its project and newest version) is kept in memory for
//!   [`LOOKUP`]: that is the only request a cached result needs.
//! - Reports and project zips are kept on disk under `<dir>/<stamp>/<slug>/<version id>/`, and
//!   the most recent ones in memory too. `stamp` covers the library and this server's own
//!   program, so new mappings or a new Hopper give new results; older stamps are deleted at
//!   start, as are files nobody asked for in [`UNUSED`].
//! - [`Cache::once`] makes requests for the same result wait for the first one's work.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

/// How long a lookup (link → project and newest version) is kept.
pub const LOOKUP: Duration = Duration::from_secs(10 * 60);
/// Files not read for this long are deleted at start.
const UNUSED: Duration = Duration::from_secs(30 * 24 * 60 * 60);
/// At most this many results are kept in memory; the oldest go first.
const IN_MEMORY: usize = 500;

pub struct Cache {
    dir: Option<PathBuf>,
    stamp: String,
    memory: Mutex<HashMap<String, (Instant, Arc<Vec<u8>>)>>,
    working: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
}

/// FNV-1a: a stable 64-bit hash (std's isn't promised to stay the same between Rust versions).
fn fnv(hash: &mut u64, bytes: &[u8]) {
    for b in bytes {
        *hash ^= u64::from(*b);
        *hash = hash.wrapping_mul(0x100_0000_01b3);
    }
}

/// The library's files and this program, hashed: results made by another Hopper are not reused.
pub fn stamp(library: &Path) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325;
    let mut files = Vec::new();
    let mut stack = vec![library.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "toml") {
                files.push(p);
            }
        }
    }
    files.sort();
    for f in files {
        fnv(&mut hash, f.strip_prefix(library).unwrap_or(&f).to_string_lossy().as_bytes());
        fnv(&mut hash, &std::fs::read(&f).unwrap_or_default());
    }
    if let Ok(exe) = std::env::current_exe().and_then(std::fs::read) {
        fnv(&mut hash, &exe);
    }
    format!("{hash:016x}")
}

impl Cache {
    /// A cache for results made with `stamp`, on disk in `dir` when given (memory only without).
    pub fn new(dir: Option<PathBuf>, stamp: String) -> Self {
        if let Some(dir) = &dir {
            let _ = std::fs::create_dir_all(dir);
            prune(dir, &stamp);
        }
        Self { dir, stamp, memory: Mutex::default(), working: Mutex::default() }
    }

    fn path(&self, key: &str) -> Option<PathBuf> {
        self.dir.as_ref().map(|d| d.join(&self.stamp).join(key))
    }

    /// The result kept under `key` (`slug/version id/check.json`), from memory or disk.
    pub fn get(&self, key: &str) -> Option<Arc<Vec<u8>>> {
        if let Some((_, body)) = self.memory.lock().unwrap_or_else(|e| e.into_inner()).get(key) {
            return Some(body.clone());
        }
        let path = self.path(key)?;
        let body = Arc::new(std::fs::read(&path).ok()?);
        // Reading marks it as used, for the pruning.
        let _ = std::fs::File::options().append(true).open(&path).and_then(|f| f.set_modified(SystemTime::now()));
        self.remember(key, body.clone());
        Some(body)
    }

    pub fn put(&self, key: &str, body: Vec<u8>) -> Arc<Vec<u8>> {
        let body = Arc::new(body);
        if let Some(path) = self.path(key) {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            // Written aside, then moved: a reader never sees half a file.
            let part = path.with_extension("part");
            if std::fs::write(&part, body.as_slice()).is_ok() {
                let _ = std::fs::rename(&part, &path);
            }
        }
        self.remember(key, body.clone());
        body
    }

    fn remember(&self, key: &str, body: Arc<Vec<u8>>) {
        let mut memory = self.memory.lock().unwrap_or_else(|e| e.into_inner());
        if memory.len() >= IN_MEMORY && !memory.contains_key(key) {
            if let Some(oldest) = memory.iter().min_by_key(|(_, (at, _))| *at).map(|(k, _)| k.clone()) {
                memory.remove(&oldest);
            }
        }
        memory.insert(key.to_string(), (Instant::now(), body));
    }

    /// A lock for the work behind `key`: the first request does it, the others wait for it and
    /// then find the result kept.
    pub fn once(&self, key: &str) -> Arc<tokio::sync::Mutex<()>> {
        let mut working = self.working.lock().unwrap_or_else(|e| e.into_inner());
        // Locks nobody holds or waits for are dropped as new ones come.
        working.retain(|_, lock| Arc::strong_count(lock) > 1);
        working.entry(key.to_string()).or_default().clone()
    }
}

/// Deletes other stamps' results, and files unused for [`UNUSED`].
fn prune(dir: &Path, stamp: &str) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() && e.file_name().to_string_lossy() != stamp {
            let _ = std::fs::remove_dir_all(&p);
        }
    }
    let mut stack = vec![dir.join(stamp)];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if e.metadata().and_then(|m| m.modified()).is_ok_and(|t| t.elapsed().is_ok_and(|age| age > UNUSED)) {
                let _ = std::fs::remove_file(&p);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_on_disk_across_restarts_and_drops_other_stamps() {
        let dir = std::env::temp_dir().join(format!("hopper-cache-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let c = Cache::new(Some(dir.clone()), "aaa".into());
        assert!(c.get("mod/v1/check.json").is_none());
        c.put("mod/v1/check.json", b"{}".to_vec());
        // A new server with the same stamp finds it on disk.
        let c = Cache::new(Some(dir.clone()), "aaa".into());
        assert_eq!(c.get("mod/v1/check.json").as_deref().map(Vec::as_slice), Some(&b"{}"[..]));
        // Another stamp (new library or program) starts empty and removes the old results.
        let c = Cache::new(Some(dir.clone()), "bbb".into());
        assert!(c.get("mod/v1/check.json").is_none());
        assert!(!dir.join("aaa").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stamps_change_with_the_library() {
        let dir = std::env::temp_dir().join(format!("hopper-stamp-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("events")).unwrap();
        std::fs::write(dir.join("events/a.toml"), "x").unwrap();
        let first = stamp(&dir);
        assert_eq!(first, stamp(&dir));
        std::fs::write(dir.join("events/a.toml"), "y").unwrap();
        assert_ne!(first, stamp(&dir));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
