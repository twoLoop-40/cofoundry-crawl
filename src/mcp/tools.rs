use crate::config::{CrawlConfig, CrawlResult};
use crate::crawler::Crawler;
use anyhow::Result;
use serde::{Deserialize, Serialize};

/// MCP tool: crawl_url — fetch and extract a single URL
#[derive(Debug, Deserialize)]
pub struct CrawlUrlInput {
    pub url: String,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
}

fn default_timeout() -> u64 {
    30
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

/// Execute crawl_url tool
pub async fn exec_crawl_url(input: CrawlUrlInput) -> Result<CrawlResult> {
    let config = CrawlConfig {
        timeout: std::time::Duration::from_secs(input.timeout_secs),
        ..Default::default()
    };
    let crawler = Crawler::new(config)?;
    crawler.crawl_url(&input.url).await
}

/// Execute extract_content tool (alias for crawl_url with just content)
pub async fn exec_extract_content(input: ExtractContentInput) -> Result<CrawlResult> {
    exec_crawl_url(CrawlUrlInput {
        url: input.url,
        timeout_secs: 30,
    })
    .await
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
                // Find snippet around match
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
    // TF-style: frequency relative to document length
    (count as f32 / len) * 1000.0
}
