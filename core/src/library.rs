//! The library: one TOML file per mapping (see `library/README.md`).

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Entry {
    /// The file's path inside `library/`, without `.toml`.
    #[serde(skip_deserializing)]
    pub id: String,
    pub from: From,
    pub to: To,
    pub info: Info,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct From {
    pub loader: String,
    pub class: String,
    pub member: Option<String>,
    #[serde(default)]
    pub aliases: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct To {
    pub hook: Option<String>,
    pub host: Option<String>,
    /// A mod API type or function that isn't a host call (`mod_api::Text::literal`).
    pub api: Option<String>,
    /// Plain Rust: nothing from Horizon is needed (`f32::clamp`).
    pub rust: Option<String>,
    pub missing: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Direct,
    Partial,
    Missing,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Info {
    pub status: Status,
    pub notes: String,
    pub since_api: String,
    #[serde(default)]
    pub seen_in: Vec<String>,
}

impl Entry {
    /// The names this entry answers for: its class and member, then each alias (`Class` or `Class#member`).
    pub fn keys(&self) -> impl Iterator<Item = (&str, Option<&str>)> {
        std::iter::once((self.from.class.as_str(), self.from.member.as_deref())).chain(
            self.from.aliases.iter().map(|a| match a.split_once('#') {
                Some((c, m)) => (c, Some(m)),
                None => (a.as_str(), None),
            }),
        )
    }

    /// What it becomes on Horizon, in a few words.
    pub fn target(&self) -> String {
        if let Some(h) = &self.to.hook {
            format!("hook {h}")
        } else if let Some(h) = self.to.host.as_ref().or(self.to.api.as_ref()) {
            h.clone()
        } else if let Some(r) = &self.to.rust {
            format!("plain Rust: {r}")
        } else {
            match self.to.missing.as_deref() {
                Some("unrequested") | None => "no Horizon API yet".to_string(),
                Some(url) => format!("no Horizon API yet ({url})"),
            }
        }
    }
}

#[derive(Debug, Default)]
pub struct Library {
    pub entries: Vec<Entry>,
}

impl Library {
    /// Reads every `*.toml` under `dir`.
    pub fn load(dir: &Path) -> Result<Self, String> {
        let mut entries = Vec::new();
        let mut stack = vec![dir.to_path_buf()];
        while let Some(d) = stack.pop() {
            for item in std::fs::read_dir(&d).map_err(|e| format!("{}: {e}", d.display()))? {
                let path = item.map_err(|e| e.to_string())?.path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|e| e == "toml") {
                    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
                    let mut entry: Entry = toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
                    let rel = path.strip_prefix(dir).unwrap_or(&path).with_extension("");
                    entry.id = rel.to_string_lossy().replace('\\', "/");
                    entries.push(entry);
                }
            }
        }
        entries.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(Self { entries })
    }

    /// Every entry for `class` (and `member`): an entry with no member answers for the whole class.
    pub fn find(&self, class: &str, member: Option<&str>) -> Vec<&Entry> {
        self.entries
            .iter()
            .filter(|e| e.keys().any(|(c, m)| c == class && (m.is_none() || m == member)))
            .collect()
    }
}
