//! Read-only Git inspection through the `git` CLI.
//!
//! Every call has a timeout, sets `GIT_OPTIONAL_LOCKS=0` so it never takes the index lock away from
//! the user's own git commands, and never modifies the repository (no commit/reset/clean/checkout).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::process;
use crate::{Error, Result};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_OUTPUT: usize = 8 * 1024 * 1024;

fn git_cmd(cwd: &Path, args: &[&str]) -> Command {
    let mut cmd = Command::new("git");
    cmd.current_dir(cwd)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_PAGER", "cat")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .args(["-c", "core.quotepath=false", "-c", "color.ui=false", "-c", "log.showSignature=false"])
        .args(args);
    cmd
}

fn run_git(cwd: &Path, args: &[&str], stdin: Option<&[u8]>, timeout: Duration) -> Result<process::Output> {
    process::run(git_cmd(cwd, args), stdin, timeout, MAX_OUTPUT).map_err(|e| Error::Git(e.to_string()))
}

fn git_ok(cwd: &Path, args: &[&str]) -> Result<String> {
    let out = run_git(cwd, args, None, DEFAULT_TIMEOUT)?;
    if !out.success() {
        return Err(Error::Git(format!("git {} failed: {}", args.join(" "), out.stderr_str().trim())));
    }
    Ok(out.stdout_str())
}

/// Is a usable `git` executable available?
pub fn available() -> bool {
    let mut cmd = Command::new("git");
    cmd.arg("--version");
    process::run(cmd, None, Duration::from_secs(5), 1024).map(|o| o.success()).unwrap_or(false)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RepoInfo {
    pub toplevel: PathBuf,
    pub common_dir: PathBuf,
    pub git_dir: PathBuf,
    /// True when `toplevel` is a linked worktree (its git dir differs from the common dir).
    pub is_worktree: bool,
}

/// Discover the repository containing `cwd`, if any.
pub fn discover(cwd: &Path) -> Option<RepoInfo> {
    let out = git_ok(cwd, &["rev-parse", "--path-format=absolute", "--show-toplevel", "--git-common-dir", "--git-dir"]).ok()?;
    let mut lines = out.lines().map(str::trim).filter(|l| !l.is_empty());
    let toplevel = PathBuf::from(lines.next()?);
    let common_dir = crate::paths::canonical(Path::new(lines.next()?));
    let git_dir = crate::paths::canonical(Path::new(lines.next()?));
    let is_worktree = crate::paths::path_key(&common_dir) != crate::paths::path_key(&git_dir);
    Some(RepoInfo { toplevel: crate::paths::canonical(&toplevel), common_dir, git_dir, is_worktree })
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Change {
    /// Two-letter porcelain status (e.g. ".M", "A.", "R.", "??").
    pub code: String,
    pub path: String,
    pub orig_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct WorkingTree {
    pub head: Option<String>,
    pub branch: Option<String>,
    pub upstream: Option<String>,
    pub ahead: i64,
    pub behind: i64,
    pub changes: Vec<Change>,
}

impl WorkingTree {
    pub fn dirty_count(&self) -> usize {
        self.changes.len()
    }
    pub fn changed_paths(&self) -> HashSet<String> {
        self.changes.iter().map(|c| c.path.clone()).collect()
    }
    pub fn short_head(&self) -> Option<String> {
        self.head.as_ref().map(|h| h.chars().take(10).collect())
    }
}

/// `git status --porcelain=v2 --branch -z` parsed into branch/head/changes.
pub fn status(root: &Path) -> Result<WorkingTree> {
    let out = run_git(root, &["status", "--porcelain=v2", "--branch", "-z", "--untracked-files=normal"], None, DEFAULT_TIMEOUT)?;
    if !out.success() {
        return Err(Error::Git(format!("git status failed: {}", out.stderr_str().trim())));
    }
    Ok(parse_status_v2(&out.stdout_str()))
}

pub fn parse_status_v2(raw: &str) -> WorkingTree {
    let mut wt = WorkingTree::default();
    let mut tokens = raw.split('\0').peekable();
    while let Some(tok) = tokens.next() {
        if tok.is_empty() {
            continue;
        }
        if let Some(rest) = tok.strip_prefix("# ") {
            let mut parts = rest.splitn(2, ' ');
            let key = parts.next().unwrap_or("");
            let val = parts.next().unwrap_or("").trim();
            match key {
                "branch.oid" if val != "(initial)" => wt.head = Some(val.to_string()),
                "branch.head" if val != "(detached)" => wt.branch = Some(val.to_string()),
                "branch.upstream" => wt.upstream = Some(val.to_string()),
                "branch.ab" => {
                    for part in val.split_whitespace() {
                        if let Some(a) = part.strip_prefix('+') {
                            wt.ahead = a.parse().unwrap_or(0);
                        } else if let Some(b) = part.strip_prefix('-') {
                            wt.behind = b.parse().unwrap_or(0);
                        }
                    }
                }
                _ => {}
            }
            continue;
        }
        let kind = tok.chars().next().unwrap_or(' ');
        match kind {
            '1' => {
                let fields: Vec<&str> = tok.splitn(9, ' ').collect();
                if fields.len() == 9 {
                    wt.changes.push(Change { code: fields[1].to_string(), path: fields[8].to_string(), orig_path: None });
                }
            }
            '2' => {
                let fields: Vec<&str> = tok.splitn(10, ' ').collect();
                if fields.len() == 10 {
                    let orig = tokens.next().map(|s| s.to_string());
                    wt.changes.push(Change { code: fields[1].to_string(), path: fields[9].to_string(), orig_path: orig });
                }
            }
            'u' => {
                let fields: Vec<&str> = tok.splitn(11, ' ').collect();
                if fields.len() == 11 {
                    wt.changes.push(Change { code: fields[1].to_string(), path: fields[10].to_string(), orig_path: None });
                }
            }
            '?' => wt.changes.push(Change { code: "??".into(), path: tok[2..].to_string(), orig_path: None }),
            _ => {}
        }
    }
    wt
}

/// HEAD commit and branch without scanning the working tree (much cheaper than `status`).
pub fn head_info(root: &Path) -> (Option<String>, Option<String>) {
    if let Ok(out) = git_ok(root, &["rev-parse", "HEAD", "--abbrev-ref", "HEAD"]) {
        let mut lines = out.lines().map(str::trim);
        let sha = lines.next().filter(|s| s.len() >= 7).map(str::to_string);
        let branch = lines.next().filter(|b| !b.is_empty() && *b != "HEAD").map(str::to_string);
        return (sha, branch);
    }
    // Unborn branch (no commits yet): only the symbolic ref exists.
    let branch = git_ok(root, &["symbolic-ref", "--short", "-q", "HEAD"]).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    (None, branch)
}

/// Root commit(s) of HEAD — a stable identity for a repository across clones and worktrees.
pub fn root_commits(root: &Path) -> Vec<String> {
    git_ok(root, &["rev-list", "--max-parents=0", "HEAD"])
        .map(|s| s.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect())
        .unwrap_or_default()
}

/// `remote.origin.url` with any embedded credentials removed.
pub fn remote_url(root: &Path) -> Option<String> {
    let url = git_ok(root, &["config", "--get", "remote.origin.url"]).ok()?;
    let url = url.trim();
    if url.is_empty() {
        return None;
    }
    Some(strip_url_credentials(url))
}

pub fn strip_url_credentials(url: &str) -> String {
    if let Some(scheme_end) = url.find("://") {
        let (scheme, rest) = url.split_at(scheme_end + 3);
        if let Some(at) = rest.find('@') {
            let slash = rest.find('/').unwrap_or(rest.len());
            if at < slash {
                return format!("{scheme}{}", &rest[at + 1..]);
            }
        }
    }
    url.to_string()
}

pub fn default_branch(root: &Path) -> Option<String> {
    if let Ok(s) = git_ok(root, &["symbolic-ref", "--short", "-q", "refs/remotes/origin/HEAD"]) {
        let s = s.trim();
        if let Some(b) = s.strip_prefix("origin/") {
            return Some(b.to_string());
        }
    }
    None
}

/// Which of `shas` are *not* present as commits in the repository.
pub fn missing_commits(root: &Path, shas: &[String]) -> Result<HashSet<String>> {
    if shas.is_empty() {
        return Ok(HashSet::new());
    }
    let input: String = shas.iter().map(|s| format!("{s}^{{commit}}\n")).collect();
    let out = run_git(root, &["cat-file", "--batch-check"], Some(input.as_bytes()), DEFAULT_TIMEOUT)?;
    if !out.success() {
        return Err(Error::Git(out.stderr_str()));
    }
    let mut missing = HashSet::new();
    for (sha, line) in shas.iter().zip(out.stdout_str().lines()) {
        if line.trim_end().ends_with(" missing") {
            missing.insert(sha.clone());
        }
    }
    Ok(missing)
}

pub fn is_ancestor(root: &Path, ancestor: &str, descendant: &str) -> bool {
    run_git(root, &["merge-base", "--is-ancestor", ancestor, descendant], None, DEFAULT_TIMEOUT).map(|o| o.success()).unwrap_or(false)
}

/// Renames between `commit` and the working tree: (old path, new path).
pub fn renames_since(root: &Path, commit: &str) -> Result<Vec<(String, String)>> {
    let out = run_git(root, &["diff", "-M50%", "--name-status", "-z", "--diff-filter=R", commit], None, DEFAULT_TIMEOUT)?;
    if !out.success() {
        return Err(Error::Git(out.stderr_str()));
    }
    let raw = out.stdout_str();
    let mut toks = raw.split('\0').filter(|t| !t.is_empty());
    let mut renames = Vec::new();
    while let Some(status) = toks.next() {
        if status.starts_with('R') {
            if let (Some(old), Some(new)) = (toks.next(), toks.next()) {
                renames.push((old.to_string(), new.to_string()));
            }
        }
    }
    Ok(renames)
}

/// Lines added/removed for `path` between `commit` and the working tree.
pub fn numstat_since(root: &Path, commit: &str, path: &str) -> Option<(u64, u64)> {
    let out = git_ok(root, &["diff", "--numstat", commit, "--", path]).ok()?;
    let line = out.lines().next()?;
    let mut parts = line.split_whitespace();
    let added = parts.next()?.parse().ok()?;
    let removed = parts.next()?.parse().ok()?;
    Some((added, removed))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CommitInfo {
    pub sha: String,
    pub subject: String,
    pub time: String,
}

/// Commits in `range` (e.g. `abc..def` or `HEAD`), newest first.
pub fn log(root: &Path, range: &str, max: usize) -> Vec<CommitInfo> {
    let n = format!("-n{max}");
    let Ok(out) = git_ok(root, &["log", &n, "--no-merges", "--format=%H%x1f%s%x1f%aI%x1e", range, "--"]) else {
        return Vec::new();
    };
    out.split('\u{1e}')
        .filter_map(|rec| {
            let rec = rec.trim_matches(['\n', '\r']);
            let mut f = rec.split('\u{1f}');
            let sha = f.next()?.trim().to_string();
            if sha.is_empty() {
                return None;
            }
            Some(CommitInfo { sha, subject: crate::privacy::redact_str(f.next().unwrap_or("")), time: f.next().unwrap_or("").to_string() })
        })
        .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Worktree {
    pub path: PathBuf,
    pub head: Option<String>,
    pub branch: Option<String>,
}

pub fn worktrees(root: &Path) -> Vec<Worktree> {
    let Ok(out) = git_ok(root, &["worktree", "list", "--porcelain"]) else {
        return Vec::new();
    };
    let mut list = Vec::new();
    let mut cur: Option<Worktree> = None;
    for line in out.lines() {
        if let Some(p) = line.strip_prefix("worktree ") {
            if let Some(w) = cur.take() {
                list.push(w);
            }
            cur = Some(Worktree { path: crate::paths::canonical(Path::new(p)), head: None, branch: None });
        } else if let Some(h) = line.strip_prefix("HEAD ") {
            if let Some(w) = cur.as_mut() {
                w.head = Some(h.to_string());
            }
        } else if let Some(b) = line.strip_prefix("branch ") {
            if let Some(w) = cur.as_mut() {
                w.branch = Some(b.trim_start_matches("refs/heads/").to_string());
            }
        }
    }
    if let Some(w) = cur {
        list.push(w);
    }
    list
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_porcelain_v2() {
        let raw = "# branch.oid 1234567890abcdef1234567890abcdef12345678\0# branch.head main\0# branch.upstream origin/main\0# branch.ab +2 -1\0\
1 .M N... 100644 100644 100644 aaaa bbbb src/lib.rs\0\
2 R. N... 100644 100644 100644 aaaa bbbb R100 src/new name.rs\0src/old.rs\0\
? notes.txt\0";
        let wt = parse_status_v2(raw);
        assert_eq!(wt.branch.as_deref(), Some("main"));
        assert_eq!(wt.ahead, 2);
        assert_eq!(wt.behind, 1);
        assert_eq!(wt.changes.len(), 3);
        assert_eq!(wt.changes[1].path, "src/new name.rs");
        assert_eq!(wt.changes[1].orig_path.as_deref(), Some("src/old.rs"));
        assert_eq!(wt.changes[2].code, "??");
    }

    #[test]
    fn detached_and_initial_states() {
        let wt = parse_status_v2("# branch.oid (initial)\0# branch.head (detached)\0");
        assert!(wt.head.is_none());
        assert!(wt.branch.is_none());
    }

    #[test]
    fn strips_credentials_from_urls() {
        assert_eq!(strip_url_credentials("https://user:tok@github.com/o/r.git"), "https://github.com/o/r.git");
        assert_eq!(strip_url_credentials("git@github.com:o/r.git"), "git@github.com:o/r.git");
    }
}
