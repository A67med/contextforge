//! Small shared helpers: timestamps, identifiers, hashing, text utilities.

use chrono::{DateTime, SecondsFormat, Utc};
use sha2::{Digest, Sha256};

/// Current time as an RFC 3339 UTC string with millisecond precision.
pub fn now_ts() -> String {
    ts(Utc::now())
}

pub fn ts(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(SecondsFormat::Millis, true)
}

pub fn parse_ts(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s).ok().map(|d| d.with_timezone(&Utc))
}

/// Human-friendly relative time ("just now", "5m ago", "3h ago", "2d ago", "2026-01-04").
pub fn relative_time(then: &str, now: DateTime<Utc>) -> String {
    let Some(t) = parse_ts(then) else {
        return then.to_string();
    };
    let secs = (now - t).num_seconds();
    if secs < 0 {
        return "just now".into();
    }
    match secs {
        0..=59 => "just now".into(),
        60..=3599 => format!("{}m ago", secs / 60),
        3600..=86_399 => format!("{}h ago", secs / 3600),
        86_400..=1_209_599 => format!("{}d ago", secs / 86_400),
        _ => t.format("%Y-%m-%d").to_string(),
    }
}

/// New prefixed, time-sortable identifier, e.g. `mem_01jd2k6v4n0c1v1t5w2r7q3x9z`.
pub fn new_id(prefix: &str) -> String {
    format!("{prefix}_{}", ulid::Ulid::generate().to_string().to_ascii_lowercase())
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for b in digest {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// Hash file content with line endings normalised so `core.autocrlf` checkouts
/// do not look like modifications.
pub fn content_hash_normalized(bytes: &[u8]) -> String {
    if bytes.contains(&b'\r') {
        let mut normalized = Vec::with_capacity(bytes.len());
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'\r' && bytes.get(i + 1) == Some(&b'\n') {
                i += 1;
                continue;
            }
            normalized.push(bytes[i]);
            i += 1;
        }
        sha256_hex(&normalized)
    } else {
        sha256_hex(bytes)
    }
}

/// Collapse whitespace runs to single spaces and trim.
pub fn collapse_ws(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last_space = true;
    for ch in s.chars() {
        if ch.is_whitespace() {
            if !last_space {
                out.push(' ');
                last_space = true;
            }
        } else {
            out.push(ch);
            last_space = false;
        }
    }
    if out.ends_with(' ') {
        out.pop();
    }
    out
}

/// Truncate to at most `max_chars` characters (not bytes), appending an ellipsis when cut.
pub fn truncate_chars(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let keep = max_chars.saturating_sub(1);
    let mut out: String = s.chars().take(keep).collect();
    out.push('…');
    out
}

/// Deterministic token estimate used for context budgets (≈4 characters per token).
pub fn estimate_tokens(s: &str) -> usize {
    s.chars().count().div_ceil(4)
}

/// Lower-case alphanumeric word tokens (length ≥ 2), used for similarity and keyword extraction.
pub fn word_tokens(s: &str) -> Vec<String> {
    s.split(|c: char| !(c.is_alphanumeric() || c == '_')).filter(|w| w.chars().count() >= 2).map(|w| w.to_lowercase()).collect()
}

pub const STOPWORDS: &[&str] = &[
    "a", "an", "and", "are", "as", "at", "be", "but", "by", "can", "could", "do", "does", "for", "from", "has", "have", "how", "i", "if",
    "in", "into", "is", "it", "its", "me", "my", "no", "not", "of", "on", "or", "our", "please", "should", "so", "that", "the", "their",
    "them", "then", "there", "these", "this", "to", "us", "was", "we", "were", "what", "when", "where", "which", "while", "who", "why",
    "will", "with", "would", "you", "your", "let", "lets", "make", "now", "also", "just", "some", "any", "all", "get", "use", "using",
    "want", "need", "like", "about", "again", "still", "help", "fix", "add", "update", "change", "look",
];

/// Keywords for retrieval: word tokens minus stopwords and pure numbers, de-duplicated, order kept.
pub fn keywords(s: &str, max: usize) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    word_tokens(s)
        .into_iter()
        .filter(|w| !STOPWORDS.contains(&w.as_str()) && !w.chars().all(|c| c.is_ascii_digit()))
        .filter(|w| seen.insert(w.clone()))
        .take(max)
        .collect()
}

/// Jaccard similarity of two token sets.
pub fn jaccard(a: &[String], b: &[String]) -> f64 {
    use std::collections::HashSet;
    let sa: HashSet<&String> = a.iter().collect();
    let sb: HashSet<&String> = b.iter().collect();
    if sa.is_empty() && sb.is_empty() {
        return 1.0;
    }
    let inter = sa.intersection(&sb).count() as f64;
    let union = sa.union(&sb).count() as f64;
    if union == 0.0 { 0.0 } else { inter / union }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crlf_normalisation_makes_hashes_equal() {
        assert_eq!(content_hash_normalized(b"a\r\nb\r\n"), content_hash_normalized(b"a\nb\n"));
        assert_ne!(content_hash_normalized(b"a\nb\n"), content_hash_normalized(b"a\nc\n"));
    }

    #[test]
    fn truncation_counts_chars() {
        assert_eq!(truncate_chars("héllo wörld", 5), "héll…");
        assert_eq!(truncate_chars("short", 10), "short");
    }

    #[test]
    fn keywords_drop_stopwords() {
        assert_eq!(keywords("Please fix the SQLite WAL locking bug in the store", 10), vec!["sqlite", "wal", "locking", "bug", "store"]);
    }

    #[test]
    fn ids_are_prefixed_and_unique() {
        let a = new_id("mem");
        let b = new_id("mem");
        assert!(a.starts_with("mem_"));
        assert_ne!(a, b);
        assert_eq!(a.len(), 4 + 26);
    }

    #[test]
    fn relative_time_formats() {
        let now = parse_ts("2026-11-12T12:00:00.000Z").unwrap();
        assert_eq!(relative_time("2026-11-12T11:59:30.000Z", now), "just now");
        assert_eq!(relative_time("2026-11-12T11:00:00.000Z", now), "1h ago");
        assert_eq!(relative_time("2026-11-10T12:00:00.000Z", now), "2d ago");
    }

    #[test]
    fn token_estimate_is_deterministic() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("abcde"), 2);
    }
}
