use std::time::Duration;

/// Crawler configuration (CrawlerCapabilitySpec §2: rate control + anti-bot basics)
#[derive(Debug, Clone)]
pub struct CrawlConfig {
    pub max_depth: usize,
    pub max_pages: usize,
    pub timeout: Duration,
    pub rate_limit_per_second: u32,
    pub user_agents: Vec<String>,
    pub respect_robots_txt: bool,
    pub follow_redirects: bool,
    pub max_redirects: usize,
}

impl Default for CrawlConfig {
    fn default() -> Self {
        Self {
            max_depth: 3,
            max_pages: 100,
            timeout: Duration::from_secs(30),
            rate_limit_per_second: 2,
            user_agents: default_user_agents(),
            respect_robots_txt: true,
            follow_redirects: true,
            max_redirects: 5,
        }
    }
}

fn default_user_agents() -> Vec<String> {
    vec![
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36".into(),
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36".into(),
        "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36".into(),
    ]
}

/// Result of a single page crawl
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CrawlResult {
    pub url: String,
    pub status: u16,
    pub title: Option<String>,
    pub content_markdown: String,
    pub links: Vec<String>,
    pub metadata: PageMetadata,
    pub depth: usize,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PageMetadata {
    pub content_type: Option<String>,
    pub content_length: Option<u64>,
    pub description: Option<String>,
    pub keywords: Vec<String>,
    pub h1: Vec<String>,
    pub h2: Vec<String>,
}

/// Full site crawl output
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SiteCrawlResult {
    pub start_url: String,
    pub pages: Vec<CrawlResult>,
    pub total_pages: usize,
    pub elapsed_ms: u64,
}
