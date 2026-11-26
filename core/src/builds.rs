//! The Horizon builds a starter project can target (`data/builds.toml`, written by
//! `scripts/update_builds.py`). Horizon's mod API only grows, so each build is the newest one
//! minus what it `lacks`: the hooks and host functions Hopper writes that came later.

use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

const BUILDS: &str = include_str!("../data/builds.toml");

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Build {
    /// The Horizon commit, as pinned in `horizon-mod.toml`.
    pub horizon: String,
    /// When it was built (YYYY-MM-DD).
    pub date: String,
    pub game: String,
    pub mixins: String,
    #[serde(default)]
    pub lacks: Vec<String>,
}

impl Build {
    /// The short commit: `f1bb66f`.
    pub fn short(&self) -> &str {
        &self.horizon[..self.horizon.len().min(7)]
    }

    /// "Nov 22 (f1bb66f)".
    pub fn label(&self) -> String {
        const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
        let mut parts = self.date.split('-').skip(1);
        let month = parts.next().and_then(|m| m.parse::<usize>().ok()).and_then(|m| MONTHS.get(m.wrapping_sub(1)));
        let day = parts.next().and_then(|d| d.parse::<u32>().ok());
        match (month, day) {
            (Some(m), Some(d)) => format!("{m} {d} ({})", self.short()),
            _ => self.short().to_string(),
        }
    }

    /// Whether this build has the hook or host function `name`.
    pub fn has(&self, name: &str) -> bool {
        !self.lacks.iter().any(|l| l == name)
    }
}

#[derive(Deserialize)]
struct File {
    build: Vec<Build>,
}

/// Every build, oldest first.
pub fn all() -> &'static [Build] {
    static ALL: OnceLock<Vec<Build>> = OnceLock::new();
    ALL.get_or_init(|| toml::from_str::<File>(BUILDS).map(|f| f.build).unwrap_or_default())
}

pub fn newest() -> &'static Build {
    all().last().expect("data/builds.toml lists at least one build")
}

/// The build a commit names (7 or more characters of it); empty or `newest` is the newest.
pub fn find(name: &str) -> Option<&'static Build> {
    let name = name.trim().to_ascii_lowercase();
    if name.is_empty() || name == "newest" {
        return Some(newest());
    }
    if name.len() < 7 {
        return None;
    }
    all().iter().find(|b| b.horizon.starts_with(&name))
}

/// The oldest build that has every one of `names`.
pub fn oldest_with<'a>(names: impl IntoIterator<Item = &'a str> + Clone) -> &'static Build {
    all().iter().find(|b| names.clone().into_iter().all(|n| b.has(n))).unwrap_or_else(newest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_builds() {
        let newest = newest();
        assert!(newest.lacks.is_empty(), "the newest build lacks nothing");
        let sdk = include_str!("../data/sdk.toml");
        assert!(sdk.contains(&newest.horizon), "data/sdk.toml pins the newest build");
        assert_eq!(find("").map(|b| &b.horizon), Some(&newest.horizon));
        assert_eq!(find(newest.short()).map(|b| &b.horizon), Some(&newest.horizon));
        assert!(find("abc").is_none());
        assert!(all().windows(2).all(|w| w[0].date <= w[1].date));
    }

    #[test]
    fn labels() {
        let b = Build {
            horizon: "f1bb66f353".into(),
            date: "2026-11-22".into(),
            game: "26.3".into(),
            mixins: String::new(),
            lacks: vec![],
        };
        assert_eq!(b.label(), "Nov 22 (f1bb66f)");
    }
}
