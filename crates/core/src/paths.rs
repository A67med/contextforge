//! Path normalisation and safe project-relative path handling.
//!
//! Agents report paths in many shapes (`C:\repo\src\a.rs`, `c:/repo/src/a.rs`, `/c/repo/src/a.rs`,
//! `src\a.rs`, `./src/../src/a.rs`). Everything is reduced to a project-relative, forward-slash path,
//! and anything that escapes the project root is rejected.

use std::path::{Component, Path, PathBuf};

use crate::{Error, Result};

/// Canonicalise without the Windows `\\?\` prefix; fall back to lexical normalisation if the path
/// does not exist (e.g. a deleted file).
pub fn canonical(path: &Path) -> PathBuf {
    let path = convert_msys_path(path);
    match dunce::canonicalize(&path) {
        Ok(p) => p,
        Err(_) => {
            let abs = if path.is_absolute() { path } else { std::env::current_dir().map(|c| c.join(&path)).unwrap_or(path) };
            lexical_normalize(&abs)
        }
    }
}

/// Resolve `.` and `..` components lexically.
pub fn lexical_normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Git-Bash / MSYS style `/c/Users/...` → `C:/Users/...` on Windows.
fn convert_msys_path(path: &Path) -> PathBuf {
    if cfg!(windows) {
        let s = path.to_string_lossy();
        let b = s.as_bytes();
        if b.len() >= 3 && b[0] == b'/' && b[1].is_ascii_alphabetic() && b[2] == b'/' {
            return PathBuf::from(format!("{}:{}", (b[1] as char).to_ascii_uppercase(), &s[2..]));
        }
    }
    path.to_path_buf()
}

/// Comparison key: forward slashes, no trailing slash, lower-case on Windows (case-insensitive FS).
pub fn path_key(path: &Path) -> String {
    let mut s = path.to_string_lossy().replace('\\', "/");
    while s.len() > 1 && s.ends_with('/') && !s.ends_with(":/") {
        s.pop();
    }
    if cfg!(windows) { s.to_lowercase() } else { s }
}

/// If `candidate` (absolute) lies within `root`, return the relative forward-slash path.
pub fn relative_to_root(root: &Path, candidate: &Path) -> Option<String> {
    let root_key = path_key(root);
    let cand = path_key(&lexical_normalize(&convert_msys_path(candidate)));
    if cand == root_key {
        return Some(String::new());
    }
    let prefix = if root_key.ends_with('/') { root_key.clone() } else { format!("{root_key}/") };
    if cand.starts_with(&prefix) {
        // Use the original-case remainder of the candidate path.
        let original = lexical_normalize(&convert_msys_path(candidate)).to_string_lossy().replace('\\', "/");
        let rest = &original[original.len() - (cand.len() - prefix.len())..];
        return Some(rest.to_string());
    }
    None
}

/// Normalise a file reference supplied by an agent or user into a project-relative path.
///
/// * absolute paths must lie inside `root` (or one of `extra_roots`, e.g. worktrees);
/// * relative paths are resolved against `base` (the session cwd) or the root;
/// * the result never contains `..`, never starts with `/`, and uses forward slashes.
pub fn normalize_file_ref(root: &Path, extra_roots: &[PathBuf], base: Option<&Path>, input: &str) -> Result<String> {
    let trimmed = input.trim().trim_matches('"').trim_matches('\'');
    let trimmed = trimmed.strip_prefix("file:///").map(|s| s.to_string()).unwrap_or_else(|| trimmed.to_string());
    if trimmed.is_empty() {
        return Err(Error::invalid("empty file path"));
    }
    if trimmed.len() > 1024 || trimmed.contains('\0') {
        return Err(Error::invalid("file path is too long or contains NUL"));
    }
    let raw = convert_msys_path(Path::new(&trimmed));
    let absolute = if is_absolute_like(&raw) { raw } else { base.unwrap_or(root).join(trimmed.replace('\\', "/")) };
    let normalized = lexical_normalize(&absolute);
    for r in std::iter::once(root).chain(extra_roots.iter().map(|p| p.as_path())) {
        if let Some(rel) = relative_to_root(r, &normalized) {
            if rel.is_empty() {
                return Err(Error::invalid(format!("'{input}' refers to the project root, not a file")));
            }
            if rel.split('/').any(|seg| seg == "..") {
                return Err(Error::invalid(format!("'{input}' escapes the project root")));
            }
            return Ok(rel);
        }
    }
    Err(Error::invalid(format!("'{input}' is outside the project root")))
}

fn is_absolute_like(p: &Path) -> bool {
    if p.is_absolute() {
        return true;
    }
    // On Windows `C:/x` is absolute but `/x` (root-relative) is not; treat a drive prefix as absolute.
    let s = p.to_string_lossy();
    let b = s.as_bytes();
    b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':'
}

/// Join a validated project-relative path onto a root.
pub fn join_rel(root: &Path, rel: &str) -> PathBuf {
    let mut p = root.to_path_buf();
    for seg in rel.split('/') {
        if !seg.is_empty() && seg != "." && seg != ".." {
            p.push(seg);
        }
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> PathBuf {
        if cfg!(windows) { PathBuf::from(r"C:\Work\Repo") } else { PathBuf::from("/work/repo") }
    }

    #[test]
    fn relative_inputs_resolve_against_base() {
        let r = root();
        assert_eq!(normalize_file_ref(&r, &[], None, "src/main.rs").unwrap(), "src/main.rs");
        assert_eq!(normalize_file_ref(&r, &[], None, "./src/../src/lib.rs").unwrap(), "src/lib.rs");
        assert_eq!(normalize_file_ref(&r, &[], Some(&r.join("src")), "lib.rs").unwrap(), "src/lib.rs");
        assert_eq!(normalize_file_ref(&r, &[], None, r"src\win\path.rs").unwrap(), "src/win/path.rs");
    }

    #[test]
    fn traversal_and_outside_paths_are_rejected() {
        let r = root();
        assert!(normalize_file_ref(&r, &[], None, "../secret.txt").is_err());
        assert!(normalize_file_ref(&r, &[], None, "src/../../x").is_err());
        let outside = if cfg!(windows) { r"C:\Other\file.rs" } else { "/other/file.rs" };
        assert!(normalize_file_ref(&r, &[], None, outside).is_err());
        assert!(normalize_file_ref(&r, &[], None, "").is_err());
    }

    #[test]
    fn absolute_inputs_inside_root_are_accepted() {
        let r = root();
        let abs = r.join("src").join("db.rs");
        assert_eq!(normalize_file_ref(&r, &[], None, &abs.to_string_lossy()).unwrap(), "src/db.rs");
    }

    #[cfg(windows)]
    #[test]
    fn windows_case_and_msys_forms() {
        let r = root();
        assert_eq!(normalize_file_ref(&r, &[], None, r"c:\work\repo\Src\A.rs").unwrap(), "Src/A.rs");
        assert_eq!(normalize_file_ref(&r, &[], None, "/c/Work/Repo/src/b.rs").unwrap(), "src/b.rs");
        assert_eq!(normalize_file_ref(&r, &[], None, "C:/Work/Repo/src/c.rs").unwrap(), "src/c.rs");
        assert_eq!(path_key(Path::new(r"C:\Work\Repo\")), "c:/work/repo");
    }

    #[test]
    fn worktree_roots_are_accepted() {
        let r = root();
        let wt = r.parent().unwrap().join("repo-feature");
        let file = wt.join("src").join("x.rs");
        assert_eq!(normalize_file_ref(&r, std::slice::from_ref(&wt), None, &file.to_string_lossy()).unwrap(), "src/x.rs");
    }
}
