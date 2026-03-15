use anyhow::Result;
use std::collections::HashSet;
use url::Url;

/// URL deduplication via normalization + HashSet (CrawlerPipelineSpec §5: dedup)
pub struct Dedup {
    seen: HashSet<String>,
}

impl Dedup {
    pub fn new() -> Self {
        Self {
            seen: HashSet::new(),
        }
    }

    pub fn is_seen(&self, url: &str) -> bool {
        self.seen.contains(url)
    }

    pub fn mark_seen(&mut self, url: &str) {
        self.seen.insert(url.to_string());
    }

    pub fn count(&self) -> usize {
        self.seen.len()
    }
}

/// Normalize URL: lowercase scheme+host, remove fragment, sort query params
pub fn normalize_url(raw: &str) -> Result<String> {
    let mut parsed = Url::parse(raw)?;

    // Remove fragment
    parsed.set_fragment(None);

    // Sort query params for consistent dedup
    if parsed.query().is_some() {
        let mut params: Vec<(String, String)> = parsed
            .query_pairs()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        params.sort();
        if params.is_empty() {
            parsed.set_query(None);
        } else {
            let sorted: Vec<String> = params
                .iter()
                .map(|(k, v)| {
                    if v.is_empty() {
                        k.clone()
                    } else {
                        format!("{k}={v}")
                    }
                })
                .collect();
            parsed.set_query(Some(&sorted.join("&")));
        }
    }

    // Strip trailing slash for bare hosts (url crate always adds "/" for scheme://host)
    let mut out = parsed.to_string();
    if out.ends_with('/') && (parsed.path() == "/" || parsed.path().is_empty()) && parsed.query().is_none() {
        out.pop();
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_removes_fragment() {
        let n = normalize_url("https://example.com/page#section").unwrap();
        assert_eq!(n, "https://example.com/page");
    }

    #[test]
    fn test_normalize_sorts_params() {
        let n = normalize_url("https://example.com/search?z=1&a=2").unwrap();
        assert_eq!(n, "https://example.com/search?a=2&z=1");
    }

    #[test]
    fn test_normalize_trailing_slash() {
        let n = normalize_url("https://example.com/").unwrap();
        assert_eq!(n, "https://example.com");
    }

    #[test]
    fn test_dedup_tracks_seen() {
        let mut d = Dedup::new();
        assert!(!d.is_seen("https://example.com"));
        d.mark_seen("https://example.com");
        assert!(d.is_seen("https://example.com"));
        assert_eq!(d.count(), 1);
    }
}
