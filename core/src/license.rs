//! Can a mod be ported? Decided from its license's SPDX id, before anything is downloaded.

use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    /// An open license that allows copying and changing, with credit.
    Ok,
    /// Copyleft: a port that reuses the mod's code must keep the same license.
    Careful,
    /// All rights reserved, custom, or unknown: Hopper doesn't port it.
    Stop,
}

#[derive(Clone, Debug, Serialize)]
pub struct LicenseCheck {
    pub id: String,
    pub verdict: Verdict,
    pub reason: String,
}

const PERMISSIVE: &[&str] = &[
    "MIT",
    "MIT-0",
    "Apache-2.0",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "ISC",
    "Zlib",
    "Unlicense",
    "CC0-1.0",
    "0BSD",
    "BSL-1.0",
    "CC-BY-4.0",
    "CC-BY-3.0",
    "WTFPL",
];

/// Copyleft families, matched by prefix (`GPL-3.0-only`, `LGPL-2.1-or-later`...).
const COPYLEFT: &[&str] = &["GPL-", "LGPL-", "AGPL-", "MPL-", "EUPL-", "OSL-", "CC-BY-SA-", "EPL-"];

pub fn check(spdx: &str) -> LicenseCheck {
    let id = spdx.trim().to_string();
    let (verdict, reason) = if PERMISSIVE.iter().any(|p| p.eq_ignore_ascii_case(&id)) {
        (Verdict::Ok, "An open license: a port may copy and change the mod, crediting it.".to_string())
    } else if COPYLEFT.iter().any(|p| id.to_ascii_uppercase().starts_with(&p.to_ascii_uppercase())) {
        (
            Verdict::Careful,
            "A copyleft license: a port that reuses the mod's code must keep this license and credit the mod.".to_string(),
        )
    } else if id.contains("NC") || id.contains("ND") {
        (
            Verdict::Stop,
            "This license forbids commercial use or changes, so a port isn't allowed without asking the author.".to_string(),
        )
    } else {
        let what = match id.as_str() {
            "" => "This mod has no license".to_string(),
            "LicenseRef-All-Rights-Reserved" | "ARR" => "This mod is all rights reserved".to_string(),
            s if s.starts_with("LicenseRef-") => "This mod has a custom license".to_string(),
            s => format!("Hopper doesn't know the license {s}"),
        };
        (
            Verdict::Stop,
            format!(
                "{what}, so it can't be ported. You can still write your own mod with the same idea from scratch, without its code or assets, and credit it as \"inspired by\"."
            ),
        )
    };
    LicenseCheck { id, verdict, reason }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sorts_licenses() {
        assert_eq!(check("MIT").verdict, Verdict::Ok);
        assert_eq!(check("Apache-2.0").verdict, Verdict::Ok);
        assert_eq!(check("LGPL-3.0-only").verdict, Verdict::Careful);
        assert_eq!(check("MPL-2.0").verdict, Verdict::Careful);
        assert_eq!(check("LicenseRef-All-Rights-Reserved").verdict, Verdict::Stop);
        assert_eq!(check("LicenseRef-Custom").verdict, Verdict::Stop);
        assert_eq!(check("CC-BY-NC-SA-4.0").verdict, Verdict::Stop);
        assert_eq!(check("").verdict, Verdict::Stop);
    }
}
