//! Multi-dimensional recall query types and keyword extraction.
//!
//! Indices are backed by `SQLite` columns/tables (see [`crate::memory::store::Store`]).
//! This module defines the query vocabulary and pure helpers used to populate
//! the keyword inverted index.

use std::time::SystemTime;

/// A single dimension along which the archive can be queried.
///
/// Each variant maps 1:1 to a `SQLite` query in [`crate::memory::store::Store`].
/// `Semantic` is a placeholder that falls back to keyword matching until a
/// real embedding-based retriever is wired in.
#[derive(Debug, Clone)]
#[allow(clippy::module_name_repetitions)]
pub enum RecallQuery {
    /// Full-text keyword search against the inverted index.
    /// Multiple words are treated as a logical OR.
    Keyword(String),
    /// Inclusive time-range query on the storage timestamp.
    TimeRange { from: SystemTime, to: SystemTime },
    /// Equality query on the `task_id` column.
    TaskId(String),
    /// Equality query on the tag column.
    Tag(String),
    /// Placeholder for future embedding similarity search.
    /// Currently resolves to the same plan as [`RecallQuery::Keyword`].
    Semantic(String),
}

impl RecallQuery {
    /// Returns the raw string of a [`RecallQuery::Keyword`] or
    /// [`RecallQuery::Semantic`] variant, or `None` for ranged/equality
    /// variants that do not carry free text.
    #[must_use]
    pub fn as_keyword(&self) -> Option<&str> {
        match self {
            Self::Keyword(s) | Self::Semantic(s) => Some(s.as_str()),
            Self::TimeRange { .. } | Self::TaskId(_) | Self::Tag(_) => None,
        }
    }
}

/// Extract normalized lowercase keywords from arbitrary text.
///
/// Tokens are split on non-alphanumeric characters, lowercased, and filtered
/// to drop empty tokens and one-character noise. This keeps the inverted
/// index small and query-time `LIKE`/join plans cheap.
///
/// # Examples
///
/// ```
/// # use opca_core::memory::extract_keywords;
/// let kws = extract_keywords("Auth refactored the OAuth2 flow!");
/// assert!(kws.contains(&"auth".to_string()));
/// assert!(kws.contains(&"refactored".to_string()));
/// assert!(kws.contains(&"oauth2".to_string()));
/// assert!(kws.contains(&"flow".to_string()));
/// ```
#[must_use]
pub fn extract_keywords(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|s| s.len() > 1)
        .map(str::to_ascii_lowercase)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_lowercase_alphanumeric_tokens() {
        let kws = extract_keywords("Hello, WORLD! foo_bar baz42");
        assert_eq!(
            kws,
            ["hello", "world", "foo", "bar", "baz42"]
                .map(String::from)
                .to_vec()
        );
    }

    #[test]
    fn drops_single_chars_and_empty() {
        let kws = extract_keywords("a b !!  ab");
        assert_eq!(kws, ["ab"].map(String::from).to_vec());
    }

    #[test]
    fn keyword_query_exposes_string() {
        assert_eq!(
            RecallQuery::Keyword("auth".into()).as_keyword(),
            Some("auth")
        );
        assert_eq!(
            RecallQuery::Semantic("auth".into()).as_keyword(),
            Some("auth")
        );
        assert_eq!(RecallQuery::TaskId("t1".into()).as_keyword(), None);
        assert_eq!(RecallQuery::Tag("sec".into()).as_keyword(), None);
        assert_eq!(
            RecallQuery::TimeRange {
                from: SystemTime::UNIX_EPOCH,
                to: SystemTime::UNIX_EPOCH,
            }
            .as_keyword(),
            None
        );
    }
}
