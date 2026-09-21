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
/// backslash surviving either way.
pub fn path_value(s: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(s.replace("\\\\", "\\"))
}
