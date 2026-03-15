use crate::config::CrawlConfig;
use anyhow::Result;
use reqwest::Client;
use std::sync::atomic::{AtomicUsize, Ordering};

/// HTTP fetch response
pub struct FetchResponse {
    pub status: u16,
    pub body: String,
    pub content_type: Option<String>,
    pub content_length: Option<u64>,
    pub final_url: String,
}

impl FetchResponse {
    pub fn is_html(&self) -> bool {
        self.content_type
            .as_ref()
            .is_some_and(|ct| ct.contains("text/html"))
    }
}

/// HTTP fetcher with User-Agent rotation (CrawlerCapabilitySpec §3: anti-bot basics)
pub struct Fetcher {
    client: Client,
    user_agents: Vec<String>,
    ua_counter: AtomicUsize,
}

impl Fetcher {
    pub fn new(config: &CrawlConfig) -> Result<Self> {
        let client = Client::builder()
            .timeout(config.timeout)
            .redirect(if config.follow_redirects {
                reqwest::redirect::Policy::limited(config.max_redirects)
            } else {
                reqwest::redirect::Policy::none()
            })
            .gzip(true)
            .brotli(true)
            .build()?;

        Ok(Self {
            client,
            user_agents: config.user_agents.clone(),
            ua_counter: AtomicUsize::new(0),
        })
    }

    /// Fetch a URL with rotating User-Agent
    pub async fn fetch(&self, url: &str) -> Result<FetchResponse> {
        let ua = self.next_user_agent();

        let response = self
            .client
            .get(url)
            .header("User-Agent", ua)
            .header("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8")
            .header("Accept-Language", "en-US,en;q=0.5")
            .send()
            .await?;

        let status = response.status().as_u16();
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        let content_length = response.content_length();
        let final_url = response.url().to_string();
        let body = response.text().await?;

        Ok(FetchResponse {
            status,
            body,
            content_type,
            content_length,
            final_url,
        })
    }

    fn next_user_agent(&self) -> &str {
        let idx = self.ua_counter.fetch_add(1, Ordering::Relaxed) % self.user_agents.len();
        &self.user_agents[idx]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ua_rotation() {
        let config = CrawlConfig::default();
        let fetcher = Fetcher::new(&config).unwrap();
        let ua1 = fetcher.next_user_agent();
        let ua2 = fetcher.next_user_agent();
        let ua3 = fetcher.next_user_agent();
        assert_ne!(ua1, ua2);
        assert_ne!(ua2, ua3);
        let ua4 = fetcher.next_user_agent();
        assert_eq!(ua1, ua4);
    }

    #[test]
    fn test_fetch_response_is_html() {
        let resp = FetchResponse {
            status: 200,
            body: String::new(),
            content_type: Some("text/html; charset=utf-8".into()),
            content_length: None,
            final_url: "http://example.com".into(),
        };
        assert!(resp.is_html());

        let resp_json = FetchResponse {
            status: 200,
            body: String::new(),
            content_type: Some("application/json".into()),
            content_length: None,
            final_url: "http://example.com/api".into(),
        };
        assert!(!resp_json.is_html());
    }
}
