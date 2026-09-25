//! Thin helpers over `keyvalues-parser` for Valve's text KeyValues (VDF/ACF).
//! `Obj` derefs to `BTreeMap<Cow<str>, Vec<Value>>`; Steam files never repeat a
//! key, so the first value is the value.

use std::path::Path;

use keyvalues_parser::{Obj, Vdf};

use crate::error::{AppError, AppResult};

pub fn parse(text: &str, path: &Path) -> AppResult<Vdf<'static>> {
    keyvalues_parser::parse(text)
        .map(|p| p.into_vdf().into_owned())
        .map_err(|e| AppError::parse(path, e.to_string()))
}

pub fn get_str<'a>(obj: &'a Obj<'_>, key: &str) -> Option<&'a str> {
    obj.get(key)?.first()?.get_str()
}

pub fn get_u64(obj: &Obj<'_>, key: &str) -> Option<u64> {
    get_str(obj, key)?.parse().ok()
}

pub fn get_u32(obj: &Obj<'_>, key: &str) -> Option<u32> {
    get_str(obj, key)?.parse().ok()
}

pub fn get_flag(obj: &Obj<'_>, key: &str) -> bool {
    matches!(get_str(obj, key), Some("1"))
}

pub fn get_obj<'a>(obj: &'a Obj<'_>, key: &str) -> Option<&'a Obj<'a>> {
    obj.get(key)?.first()?.get_obj()
}

/// Steam writes Windows paths with escaped backslashes (`C:\\Program Files`).
/// The parser already unescapes quoted strings; this guards against a double
/// backslash surviving either way — except at the start, where two backslashes are a
/// UNC share (`\\nas\games`) or a verbatim prefix, and collapsing them made the path
/// relative to the current drive (D-239).
pub fn path_value(s: &str) -> std::path::PathBuf {
    match s.strip_prefix(r"\\") {
        Some(rest) => std::path::PathBuf::from(format!(r"\\{}", rest.replace(r"\\", r"\"))),
        None => std::path::PathBuf::from(s.replace(r"\\", r"\")),
    }
}

#[cfg(test)]
mod tests {
    use super::path_value;
    use std::path::PathBuf;

    /// A UNC or verbatim library keeps its prefix; an escaped drive path is unescaped (D-239).
    #[test]
    fn path_value_keeps_unc_and_verbatim_prefixes() {
        for (raw, want) in [
            (r"\\nas\games\SteamLibrary", r"\\nas\games\SteamLibrary"),
            (r"\\?\G:\SteamLibrary", r"\\?\G:\SteamLibrary"),
            (r"G:\\SteamLibrary", r"G:\SteamLibrary"),
            (r"G:\SteamLibrary", r"G:\SteamLibrary"),
        ] {
            assert_eq!(path_value(raw), PathBuf::from(want), "{raw}");
        }
    }
}
