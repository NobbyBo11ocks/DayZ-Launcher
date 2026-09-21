//! Minimal reader for the `key = value;` files DayZ mods ship:
//! `meta.cpp` (Workshop-generated: `protocol`, `publishedid`, `name`, `timestamp`)
//! and `mod.cpp` (author-written: `name`, `author`, `version`, `overview`, …).
//! Layout evidence: docs/02-dayz-launch-mechanics.md §3 (live files, S-41).
//!
//! The format is a flat list of assignments; values are either quoted strings
//! or bare numbers, terminated by `;`. Nothing else is needed here, so this
//! stays a 40-line scanner rather than a real C-preprocessor grammar.

use std::path::Path;

/// Parsed assignments in file order. Keys are trimmed; string values are
/// unquoted; numeric values are kept as their literal text.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct CppValues(Vec<(String, String)>);

impl CppValues {
    pub fn parse(text: &str) -> Self {
        let mut out = Vec::new();
        for raw in text.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with("//") {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            let key = k.trim();
            if key.is_empty() {
                continue;
            }
            let mut val = v.trim();
            if let Some(stripped) = val.strip_suffix(';') {
                val = stripped.trim();
            }
            let val = val
                .strip_prefix('"')
                .and_then(|s| s.strip_suffix('"'))
                .unwrap_or(val);
            out.push((key.to_string(), val.to_string()));
        }
        Self(out)
    }

    /// Reads and parses a file; `None` when it does not exist or is not UTF-8.
    pub fn read(path: &Path) -> Option<Self> {
        let bytes = std::fs::read(path).ok()?;
        let text = String::from_utf8_lossy(&bytes);
        Some(Self::parse(&text))
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v.as_str())
    }

    pub fn get_u64(&self, key: &str) -> Option<u64> {
        self.get(key)?.parse().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const META_CF: &str = include_str!("../../tests/fixtures/meta_cf.cpp");
    const MOD_CF: &str = include_str!("../../tests/fixtures/mod_cf.cpp");

    #[test]
    fn meta_cpp_from_live_fixture() {
        let m = CppValues::parse(META_CF);
        assert_eq!(m.get("protocol"), Some("1"));
        assert_eq!(m.get_u64("publishedid"), Some(1_559_212_036));
        assert_eq!(m.get("name"), Some("CF"));
        assert!(m.get_u64("timestamp").is_some());
    }

    #[test]
    fn mod_cpp_from_live_fixture() {
        let m = CppValues::parse(MOD_CF);
        assert_eq!(m.get("name"), Some("Community Framework"));
        assert_eq!(m.get("version"), Some("1.5.8"));
        assert_eq!(m.get("author"), Some("CF Mod Team"));
        assert_eq!(m.get("authorID"), Some("76561198103677868"));
        assert!(m
            .get("overview")
            .unwrap()
            .starts_with("This is a Community Framework"));
    }

    #[test]
    fn tolerates_noise() {
        let m = CppValues::parse("// comment\n\nname=\"X\"\nbogus line\nn = 5 ;\n");
        assert_eq!(m.get("NAME"), Some("X"));
        assert_eq!(m.get_u64("n"), Some(5));
        assert_eq!(m.get("bogus"), None);
    }
}
