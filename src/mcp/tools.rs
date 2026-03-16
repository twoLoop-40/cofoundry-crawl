use crate::config::{CrawlConfig, CrawlResult};
use crate::crawler::Crawler;
use crate::crawler::browser::{self, CrawlCookie};
use anyhow::Result;
use serde::{Deserialize, Serialize};

/// MCP tool: crawl_url — fetch and extract a single URL
#[derive(Debug, Deserialize)]
pub struct CrawlUrlInput {
    pub url: String,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    /// If true, use headless Chrome for JavaScript rendering (SPA support)
    #[serde(default)]
    pub render: bool,
    /// SOCKS5 proxy URL (e.g., "socks5://127.0.0.1:9050" for Tor)
    #[serde(default)]
    pub proxy: Option<String>,
    /// Milliseconds to wait after page load for JS rendering (default: 1500)
    #[serde(default = "default_wait_ms")]
    pub wait_ms: u64,
    /// Cookies to inject before navigation (for authenticated crawling)
    #[serde(default)]
    pub cookies: Vec<CrawlCookie>,
}

fn default_timeout() -> u64 {
    30
}

fn default_wait_ms() -> u64 {
    1500
}

/// MCP tool: search_site — BFS crawl + search for keyword
#[derive(Debug, Deserialize)]
pub struct SearchSiteInput {
    pub url: String,
    pub query: String,
    #[serde(default = "default_depth")]
    pub max_depth: usize,
    #[serde(default = "default_max_pages")]
    pub max_pages: usize,
    /// If true, use headless Chrome for each page
    #[serde(default)]
    pub render: bool,
    #[serde(default)]
    pub proxy: Option<String>,
}

fn default_depth() -> usize {
    2
}

fn default_max_pages() -> usize {
    20
}

/// MCP tool: extract_content — extract structured content from HTML
#[derive(Debug, Deserialize)]
pub struct ExtractContentInput {
    pub url: String,
    #[serde(default)]
    pub render: bool,
    #[serde(default)]
    pub proxy: Option<String>,
    #[serde(default)]
    pub cookies: Vec<CrawlCookie>,
}

/// MCP tool: screenshot — take full-page screenshot
#[derive(Debug, Deserialize)]
pub struct ScreenshotInput {
    pub url: String,
    #[serde(default)]
    pub proxy: Option<String>,
    #[serde(default = "default_wait_ms")]
    pub wait_ms: u64,
    #[serde(default)]
    pub cookies: Vec<CrawlCookie>,
}

/// MCP tool: render_batch — render multiple URLs in parallel with headless Chrome
#[derive(Debug, Deserialize)]
pub struct RenderBatchInput {
    pub urls: Vec<String>,
    #[serde(default)]
    pub proxy: Option<String>,
    #[serde(default = "default_wait_ms")]
    pub wait_ms: u64,
    #[serde(default = "default_concurrent")]
    pub max_concurrent: usize,
    #[serde(default)]
    pub cookies: Vec<CrawlCookie>,
}

/// MCP tool: login — perform API-based login and return session tokens
#[derive(Debug, Deserialize)]
pub struct LoginInput {
    /// Login page URL (used to determine origin) or direct API endpoint URL
    pub url: String,
    pub email: String,
    pub password: String,
    /// Direct API login endpoint URL (overrides auto-detection from url)
    #[serde(default)]
    pub api_url: Option<String>,
    #[serde(default)]
    pub proxy: Option<String>,
    #[serde(default = "default_wait_ms")]
    pub wait_ms: u64,
    /// localStorage key name for SPA auth injection (e.g., "myapp-auth").
    /// If provided, the JWT response is stored in localStorage under this key.
    /// If omitted, only HTTP cookies are returned (no localStorage injection).
    #[serde(default)]
    pub ls_key: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct LoginOutput {
    pub success: bool,
    pub cookies: Vec<CookieInfo>,
    pub final_url: String,
    pub elapsed_ms: u64,
}

#[derive(Debug, Serialize)]
pub struct CookieInfo {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub path: String,
}

fn default_concurrent() -> usize {
    5
}

#[derive(Debug, Serialize)]
pub struct SearchResult {
    pub url: String,
    pub title: Option<String>,
    pub snippet: String,
    pub score: f32,
}

#[derive(Debug, Serialize)]
pub struct SearchSiteOutput {
    pub results: Vec<SearchResult>,
    pub total_pages_crawled: usize,
    pub elapsed_ms: u64,
}

#[derive(Debug, Serialize)]
pub struct ScreenshotOutput {
    pub url: String,
    pub png_base64: String,
    pub size_bytes: usize,
}

#[derive(Debug, Serialize)]
pub struct RenderBatchOutput {
    pub results: Vec<RenderBatchItem>,
    pub total: usize,
    pub success: usize,
    pub elapsed_ms: u64,
}

#[derive(Debug, Serialize)]
pub struct RenderBatchItem {
    pub url: String,
    pub success: bool,
    pub title: Option<String>,
    pub content_length: usize,
    pub elapsed_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Execute crawl_url tool
pub async fn exec_crawl_url(input: CrawlUrlInput) -> Result<CrawlResult> {
    if input.render || !input.cookies.is_empty() {
        browser::render_page(&input.url, input.proxy.as_deref(), input.wait_ms, &input.cookies).await
    } else {
        let config = CrawlConfig {
            timeout: std::time::Duration::from_secs(input.timeout_secs),
            ..Default::default()
        };
        let crawler = Crawler::new(config)?;
        crawler.crawl_url(&input.url).await
    }
}

/// Execute extract_content tool (alias for crawl_url with just content)
pub async fn exec_extract_content(input: ExtractContentInput) -> Result<CrawlResult> {
    exec_crawl_url(CrawlUrlInput {
        url: input.url,
        timeout_secs: 30,
        render: input.render,
        proxy: input.proxy,
        wait_ms: default_wait_ms(),
        cookies: input.cookies,
    })
    .await
}

/// Execute screenshot tool
pub async fn exec_screenshot(input: ScreenshotInput) -> Result<ScreenshotOutput> {
    let png_bytes = browser::screenshot(&input.url, input.proxy.as_deref(), input.wait_ms, &input.cookies).await?;
    let png_base64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &png_bytes);
    Ok(ScreenshotOutput {
        url: input.url,
        size_bytes: png_bytes.len(),
        png_base64,
    })
}

/// Execute login tool — API-based login, returns session tokens
pub async fn exec_login(input: LoginInput) -> Result<LoginOutput> {
    let start = std::time::Instant::now();
    let (cookies, final_url) = browser::login_and_get_cookies(
        &input.url,
        &input.email,
        &input.password,
        input.proxy.as_deref(),
        input.wait_ms,
        input.api_url.as_deref(),
        input.ls_key.as_deref(),
    ).await?;

    Ok(LoginOutput {
        success: true,
        cookies,
        final_url,
        elapsed_ms: start.elapsed().as_millis() as u64,
    })
}

/// Execute render_batch tool — parallel SPA rendering
pub async fn exec_render_batch(input: RenderBatchInput) -> Result<RenderBatchOutput> {
    let start = std::time::Instant::now();
    let results = browser::render_batch(
        &input.urls,
        input.proxy.as_deref(),
        input.wait_ms,
        input.max_concurrent,
        &input.cookies,
    )
    .await;

    let mut items = Vec::with_capacity(results.len());
    let mut success_count = 0;

    for result in results {
        match result {
            Ok(crawl_result) => {
                success_count += 1;
                items.push(RenderBatchItem {
                    url: crawl_result.url,
                    success: true,
                    title: crawl_result.title,
                    content_length: crawl_result.content_markdown.len(),
                    elapsed_ms: crawl_result.elapsed_ms,
                    error: None,
                });
            }
            Err(e) => {
                items.push(RenderBatchItem {
                    url: String::new(),
                    success: false,
                    title: None,
                    content_length: 0,
                    elapsed_ms: 0,
                    error: Some(e.to_string()),
                });
            }
        }
    }

    Ok(RenderBatchOutput {
        total: items.len(),
        success: success_count,
        results: items,
        elapsed_ms: start.elapsed().as_millis() as u64,
    })
}

/// Execute search_site tool
pub async fn exec_search_site(input: SearchSiteInput) -> Result<SearchSiteOutput> {
    let config = CrawlConfig {
        max_depth: input.max_depth,
        max_pages: input.max_pages,
        ..Default::default()
    };
    let mut crawler = Crawler::new(config)?;
    let site_result = crawler.crawl_site(&input.url).await?;

    let query_lower = input.query.to_lowercase();
    let mut results: Vec<SearchResult> = site_result
        .pages
        .iter()
        .filter_map(|page| {
            let content_lower = page.content_markdown.to_lowercase();
            if content_lower.contains(&query_lower) {
                let snippet = find_snippet(&page.content_markdown, &input.query, 200);
                let score = compute_relevance(&content_lower, &query_lower);
                Some(SearchResult {
                    url: page.url.clone(),
                    title: page.title.clone(),
                    snippet,
                    score,
                })
            } else {
                None
            }
        })
        .collect();

    results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

    Ok(SearchSiteOutput {
        total_pages_crawled: site_result.total_pages,
        elapsed_ms: site_result.elapsed_ms,
        results,
    })
}

fn find_snippet(content: &str, query: &str, max_len: usize) -> String {
    let lower = content.to_lowercase();
    let query_lower = query.to_lowercase();
    if let Some(pos) = lower.find(&query_lower) {
        let start = pos.saturating_sub(max_len / 2);
        let end = (pos + query.len() + max_len / 2).min(content.len());
        let snippet = &content[start..end];
        format!("...{}...", snippet.trim())
    } else {
        content.chars().take(max_len).collect()
    }
}

fn compute_relevance(content: &str, query: &str) -> f32 {
    let count = content.matches(query).count();
    let len = content.len().max(1) as f32;
    (count as f32 / len) * 1000.0
}
