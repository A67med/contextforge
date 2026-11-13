//! Reading and writing JSON configuration files without losing unrelated settings.
//!
//! Key order is preserved (serde_json `preserve_order`). Files that are not valid JSON are never
//! overwritten — the caller gets an error and the user is asked to fix the file.

use std::path::Path;

use serde_json::{Map, Value};

use crate::{IntegrationError, Result, plan::read_opt};

/// Current raw content (None if the file is missing) and the parsed object (empty if missing/blank).
pub fn read_object(path: &Path) -> Result<(Option<String>, Map<String, Value>)> {
    let raw = read_opt(path)?;
    let text = raw.as_deref().unwrap_or("").trim_start_matches('\u{feff}');
    if text.trim().is_empty() {
        return Ok((raw, Map::new()));
    }
    let value: Value = serde_json::from_str(text).map_err(|e| {
        IntegrationError::Config(format!(
            "{} is not valid JSON ({e}); fix or move it before installing — ContextForge will not overwrite it",
            path.display()
        ))
    })?;
    match value {
        Value::Object(m) => Ok((raw, m)),
        _ => Err(IntegrationError::Config(format!("{} must contain a JSON object", path.display()))),
    }
}

/// Pretty JSON (2-space indent) with a trailing newline.
pub fn to_pretty(map: &Map<String, Value>) -> String {
    let mut s = serde_json::to_string_pretty(&Value::Object(map.clone())).unwrap_or_else(|_| "{}".into());
    s.push('\n');
    s
}

/// Get (creating if needed) a nested object at `key`. Errors if `key` holds a non-object.
pub fn object_entry<'a>(map: &'a mut Map<String, Value>, key: &str, file: &Path) -> Result<&'a mut Map<String, Value>> {
    let entry = map.entry(key.to_string()).or_insert_with(|| Value::Object(Map::new()));
    match entry {
        Value::Object(m) => Ok(m),
        _ => Err(IntegrationError::Config(format!("'{key}' in {} is not an object; refusing to modify it", file.display()))),
    }
}

pub fn array_entry<'a>(map: &'a mut Map<String, Value>, key: &str, file: &Path) -> Result<&'a mut Vec<Value>> {
    let entry = map.entry(key.to_string()).or_insert_with(|| Value::Array(Vec::new()));
    match entry {
        Value::Array(a) => Ok(a),
        _ => Err(IntegrationError::Config(format!("'{key}' in {} is not an array; refusing to modify it", file.display()))),
    }
}

/// Only produce a change when the serialised content actually differs.
pub fn change_if_different(
    path: &Path,
    before: Option<String>,
    after_map: &Map<String, Value>,
    summary: &str,
) -> Option<crate::plan::FileChange> {
    let after = to_pretty(after_map);
    let unchanged = before.as_deref().map(|b| normalize(b) == normalize(&after)).unwrap_or(false);
    if unchanged {
        return None;
    }
    Some(crate::plan::FileChange { path: path.to_path_buf(), before, after: Some(after), summary: summary.into() })
}

fn normalize(s: &str) -> Option<Value> {
    serde_json::from_str(s.trim_start_matches('\u{feff}')).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_json_is_never_overwritten() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("settings.json");
        std::fs::write(&p, "{ \"a\": 1, // comment\n }").unwrap();
        assert!(read_object(&p).is_err());
        std::fs::write(&p, "[1,2]").unwrap();
        assert!(read_object(&p).is_err());
    }

    #[test]
    fn bom_and_missing_files() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("x.json");
        let (raw, m) = read_object(&p).unwrap();
        assert!(raw.is_none() && m.is_empty());
        std::fs::write(&p, "\u{feff}{\"k\":true}").unwrap();
        let (_, m) = read_object(&p).unwrap();
        assert_eq!(m["k"], Value::Bool(true));
    }

    #[test]
    fn preserves_key_order() {
        let mut m = Map::new();
        m.insert("zeta".into(), Value::from(1));
        m.insert("alpha".into(), Value::from(2));
        let s = to_pretty(&m);
        assert!(s.find("zeta").unwrap() < s.find("alpha").unwrap());
    }
}
