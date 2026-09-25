//! `!Workshop` junction management, compatible with the official launcher (docs/02 §4, D-009):
//! one NTFS junction per mod named `@<meta.cpp name>` pointing at the Workshop item folder.
//! Existing junctions are reused when they already point at the right folder; a
//! name clash with a different target gets a `@<name> (<id>)` sibling. Nothing is
//! ever deleted.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModLink {
    pub id: u64,
    pub name: String,
    pub source: PathBuf,
    pub junction: PathBuf,
    pub created: bool,
}

/// Folder name for a mod: `@` + meta.cpp name with characters Windows forbids removed.
pub fn junction_name(id: u64, meta_name: Option<&str>) -> String {
    let cleaned: String = meta_name
        .unwrap_or("")
        .chars()
        .filter(|c| {
            // `;` is not forbidden by Windows but it separates -mod= entries
            // (launch/args.rs), so a mod name carrying one would split its own
            // path into two arguments and the launch would fail (D-160).
            !matches!(
                c,
                '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' | ';'
            ) && !c.is_control()
        })
        .collect::<String>()
        .trim()
        .trim_end_matches('.')
        .to_string();
    if cleaned.is_empty() {
        format!("@{id}")
    } else {
        format!("@{cleaned}")
    }
}

/// Ensures a junction exists for each `(id, item folder, meta name)`; returns them in input order.
pub fn ensure_junctions(
    game_dir: &Path,
    items: &[(u64, PathBuf, Option<String>)],
) -> AppResult<Vec<ModLink>> {
    let workshop = game_dir.join("!Workshop");
    std::fs::create_dir_all(&workshop).map_err(|e| AppError::io(&workshop, e))?;
    let mut out = Vec::with_capacity(items.len());
    for (id, source, meta_name) in items {
        if !source.is_dir() {
            return Err(AppError::Internal(format!(
                "Workshop item {id} has no folder at {}",
                source.display()
            )));
        }
        let base = junction_name(*id, meta_name.as_deref());
        let candidates = [
            workshop.join(&base),
            workshop.join(format!("{base} ({id})")),
        ];
        let mut chosen: Option<(PathBuf, bool)> = None;
        for path in &candidates {
            match std::fs::symlink_metadata(path) {
                Ok(_) => {
                    if let Ok(target) = junction::get_target(path) {
                        if same_dir(&target, source) {
                            chosen = Some((path.clone(), false));
                            break;
                        }
                    }
                    // exists but is not our junction: try the next candidate
                }
                Err(_) => {
                    if let Err(e) = junction::create(source, path) {
                        // `junction::create` makes the folder first and turns it into a
                        // mount point second. When the second step failed — a FAT or
                        // exFAT library, a permission — the empty folder stayed, every
                        // later launch found the name taken, and the one after that the
                        // sibling too. This call made it and nothing is in it, so it is
                        // ours to take back; the rule about junctions we did not create
                        // does not reach it (D-093, D-239).
                        if e.kind() != std::io::ErrorKind::AlreadyExists && is_empty_plain_dir(path)
                        {
                            let _ = std::fs::remove_dir(path);
                        }
                        return Err(AppError::io(path, e));
                    }
                    chosen = Some((path.clone(), true));
                    break;
                }
            }
        }
        let (junction, created) = chosen.ok_or_else(|| {
            AppError::Internal(format!(
                "cannot create a junction for mod {id}: {} and its sibling are taken",
                candidates[0].display()
            ))
        })?;
        out.push(ModLink {
            id: *id,
            name: base,
            source: source.clone(),
            junction,
            created,
        });
    }
    Ok(out)
}

/// A real, empty directory: not a junction or any other reparse point, nothing inside.
fn is_empty_plain_dir(p: &Path) -> bool {
    std::fs::symlink_metadata(p).is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink())
        && std::fs::read_dir(p).is_ok_and(|mut d| d.next().is_none())
}

fn same_dir(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| {
        let s = p.to_string_lossy().replace('/', "\\");
        let s = s.strip_prefix(r"\\?\").map(str::to_string).unwrap_or(s);
        s.trim_end_matches('\\').to_ascii_lowercase()
    };
    norm(a) == norm(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_follow_official_convention() {
        assert_eq!(junction_name(1_559_212_036, Some("CF")), "@CF");
        assert_eq!(
            junction_name(2_545_327_648, Some("Dabs Framework")),
            "@Dabs Framework"
        );
        assert_eq!(junction_name(7, Some("Bad:Name?/")), "@BadName");
        assert_eq!(junction_name(7, Some("   ")), "@7");
        assert_eq!(junction_name(7, None), "@7");
    }

    #[test]
    fn same_dir_ignores_case_prefix_and_trailing_slash() {
        assert!(same_dir(
            Path::new(r"\\?\G:\SteamLibrary\steamapps\workshop\content\221100\1559212036"),
            Path::new(r"g:\steamlibrary\steamapps\workshop\content\221100\1559212036\")
        ));
        assert!(!same_dir(Path::new(r"G:\a\1"), Path::new(r"G:\a\2")));
    }

    #[test]
    fn creates_reuses_and_disambiguates() {
        let tmp = std::env::temp_dir().join(format!("dzl-junction-test-{}", std::process::id()));
        let game = tmp.join("game");
        let src_a = tmp.join("content").join("111");
        let src_b = tmp.join("content").join("222");
        std::fs::create_dir_all(&game).unwrap();
        std::fs::create_dir_all(&src_a).unwrap();
        std::fs::create_dir_all(&src_b).unwrap();

        let first = ensure_junctions(&game, &[(111, src_a.clone(), Some("Same".into()))]).unwrap();
        assert!(first[0].created);
        assert_eq!(first[0].junction, game.join("!Workshop").join("@Same"));
        assert!(junction::exists(&first[0].junction).unwrap());

        let again = ensure_junctions(&game, &[(111, src_a.clone(), Some("Same".into()))]).unwrap();
        assert!(
            !again[0].created,
            "existing junction with the right target is reused"
        );

        let clash = ensure_junctions(&game, &[(222, src_b.clone(), Some("Same".into()))]).unwrap();
        assert!(clash[0].created);
        assert_eq!(
            clash[0].junction,
            game.join("!Workshop").join("@Same (222)")
        );

        for l in [&first[0], &clash[0]] {
            let _ = junction::delete(&l.junction);
            let _ = std::fs::remove_dir(&l.junction);
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
