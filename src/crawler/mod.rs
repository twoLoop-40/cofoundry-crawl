pub mod fetcher;
pub mod frontier;
pub mod extractor;
pub mod dedup;

use crate::config::{CrawlConfig, CrawlResult, SiteCrawlResult};
use anyhow::Result;
use std::time::Instant;
use tracing::info;

/// Main crawler engine — BFS crawl with rate limiting
pub struct Crawler {
    config: CrawlConfig,
    fetcher: fetcher::Fetcher,
    frontier: frontier::Frontier,
    dedup: dedup::Dedup,
}

impl Crawler {
    pub fn new(config: CrawlConfig) -> Result<Self> {
        let fetcher = fetcher::Fetcher::new(&config)?;
        let frontier = frontier::Frontier::new();
        let dedup = dedup::Dedup::new();
        Ok(Self { config, fetcher, frontier, dedup })
    }

    /// Crawl a single URL and return structured result
    pub async fn crawl_url(&self, url: &str) -> Result<CrawlResult> {
        let start = Instant::now();
        let response = self.fetcher.fetch(url).await?;
        let status = response.status;
        let content_type = response.content_type.clone();
        let content_length = response.content_length;

        let (title, markdown, links, metadata) = if response.is_html() {
            let extracted = extractor::extract(&response.body, url);
            (extracted.title, extracted.markdown, extracted.links, extracted.metadata)
        } else {
            (None, response.body.clone(), vec![], extractor::empty_metadata())
        };

        let mut meta = metadata;
        meta.content_type = content_type;
        meta.content_length = content_length;

        Ok(CrawlResult {
            url: url.to_string(),
            status,
            title,
            content_markdown: markdown,
            links,
            metadata: meta,
            depth: 0,
            elapsed_ms: start.elapsed().as_millis() as u64,
        })
    }

    /// BFS crawl starting from seed URL
    pub async fn crawl_site(&mut self, seed_url: &str) -> Result<SiteCrawlResult> {
        let start = Instant::now();
        let mut results: Vec<CrawlResult> = Vec::new();

        // Normalize and add seed
        let seed = dedup::normalize_url(seed_url)?;
        self.frontier.push(seed.clone(), 0);
        self.dedup.mark_seen(&seed);

        while let Some((url, depth)) = self.frontier.pop() {
            if results.len() >= self.config.max_pages {
                info!("Reached max_pages limit: {}", self.config.max_pages);
                break;
            }
            if depth > self.config.max_depth {
                continue;
            }

            info!(url = %url, depth, "Crawling");

            match self.crawl_url(&url).await {
                Ok(mut result) => {
                    result.depth = depth;

                    // Enqueue discovered links
                    for link in &result.links {
                        if let Ok(normalized) = dedup::normalize_url(link) {
                            if !self.dedup.is_seen(&normalized) {
                                self.dedup.mark_seen(&normalized);
                                self.frontier.push(normalized, depth + 1);
                            }
                        }
                    }

                    results.push(result);
                }
                Err(e) => {
                    info!(url = %url, error = %e, "Failed to crawl");
                }
            }

            // Rate limiting via simple delay
            tokio::time::sleep(std::time::Duration::from_millis(
                1000 / self.config.rate_limit_per_second as u64,
            ))
            .await;
        }

        let total_pages = results.len();
        Ok(SiteCrawlResult {
            start_url: seed_url.to_string(),
            pages: results,
            total_pages,
            elapsed_ms: start.elapsed().as_millis() as u64,
        })
    }
}
