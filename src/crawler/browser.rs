//! Headless Chrome browser pool for SPA rendering + screenshots
//!
//! Uses chromiumoxide (Chrome DevTools Protocol) with tokio.
//! Supports parallel tab crawling and Tor proxy for .onion sites.

use anyhow::{Context, Result};
use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::page::ScreenshotParams;
use chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotFormat;
use futures::StreamExt;
use std::sync::Arc;
use tokio::sync::OnceCell;
use tracing::info;

use std::path::PathBuf;

use crate::config::CrawlResult;
use crate::crawler::extractor;

/// Find Chrome/Chromium binary on the system
fn find_chrome_binary() -> Option<PathBuf> {
    let candidates = [
        // macOS
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        "/Applications/Chromium.app/Contents/MacOS/Chromium",
        // Playwright chromium (macOS)
        // Linux
        "/usr/bin/google-chrome-stable",
        "/usr/bin/google-chrome",
        "/usr/bin/chromium-browser",
        "/usr/bin/chromium",
        // Homebrew
        "/opt/homebrew/bin/chromium",
    ];

    // Check CHROME_PATH env var first
    if let Ok(path) = std::env::var("CHROME_PATH") {
        let p = PathBuf::from(&path);
        if p.exists() {
            return Some(p);
        }
    }

    for candidate in &candidates {
        let p = PathBuf::from(candidate);
        if p.exists() {
            return Some(p);
        }
    }

    // Check Playwright's chromium
    if let Ok(home) = std::env::var("HOME") {
        let playwright_dir = PathBuf::from(&home).join("Library/Caches/ms-playwright");
        if let Ok(entries) = std::fs::read_dir(&playwright_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with("chromium-") {
                    let chrome = entry.path().join("chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing");
                    if chrome.exists() {
                        return Some(chrome);
                    }
                    // Linux variant
                    let chrome_linux = entry.path().join("chrome-linux64/chrome");
                    if chrome_linux.exists() {
                        return Some(chrome_linux);
                    }
                }
            }
        }
    }

    None
}

/// Global browser instance (lazy-initialized on first render request)
static BROWSER: OnceCell<Arc<Browser>> = OnceCell::const_new();

/// Initialize browser with optional proxy (e.g., "socks5://127.0.0.1:9050" for Tor)
async fn get_or_init_browser(proxy: Option<&str>) -> Result<Arc<Browser>> {
    // If already initialized, return existing
    if let Some(b) = BROWSER.get() {
        return Ok(b.clone());
    }

    // Find Chrome binary — check common locations
    let chrome_path = find_chrome_binary();

    // Create unique user-data-dir to avoid SingletonLock conflicts
    let user_data_dir = std::env::temp_dir()
        .join(format!("cofoundry-crawl-{}", std::process::id()));

    let mut builder = BrowserConfig::builder()
        .no_sandbox()
        .disable_default_args()
        .user_data_dir(&user_data_dir)
        .arg("--headless=new")
        .arg("--disable-gpu")
        .arg("--disable-dev-shm-usage")
        .arg("--no-first-run")
        .arg("--disable-extensions");

    if let Some(ref path) = chrome_path {
        builder = builder.chrome_executable(path);
        info!(path = %path.display(), "Using Chrome binary");
    }

    if let Some(proxy_url) = proxy {
        builder = builder.arg(format!("--proxy-server={proxy_url}"));
        info!(proxy = proxy_url, "Browser configured with proxy");
    }

    let config = builder.build().map_err(|e| anyhow::anyhow!("Browser config error: {e}"))?;
    let (browser, mut handler) = Browser::launch(config)
        .await
        .map_err(|e| anyhow::anyhow!("Chrome launch failed: {e:?}"))?;

    // Spawn handler loop (required for CDP event processing)
    tokio::spawn(async move {
        while let Some(event) = handler.next().await {
            if event.is_err() {
                break;
            }
        }
    });

    let browser = Arc::new(browser);
    let _ = BROWSER.set(browser.clone());
    info!("Headless Chrome browser initialized");
    Ok(browser)
}

/// Render a single URL with headless Chrome and extract content
pub async fn render_page(url: &str, proxy: Option<&str>, wait_ms: u64) -> Result<CrawlResult> {
    let start = std::time::Instant::now();
    let browser = get_or_init_browser(proxy).await?;

    let page = browser.new_page(url).await
        .context("Failed to open new tab")?;

    // Wait for JS rendering
    if wait_ms > 0 {
        tokio::time::sleep(std::time::Duration::from_millis(wait_ms)).await;
    }

    // Get rendered HTML
    let html = page.content().await
        .context("Failed to get page content")?;

    // Extract title via JS (more reliable for SPAs)
    let title = page.evaluate("document.title")
        .await
        .ok()
        .and_then(|v| v.into_value::<String>().ok())
        .filter(|s| !s.is_empty());

    // Extract content using existing extractor
    let extracted = extractor::extract(&html, url);

    let status = 200u16; // Chrome doesn't expose HTTP status directly

    let mut meta = extracted.metadata;
    meta.content_type = Some("text/html".to_string());

    // Close tab
    let _ = page.close().await;

    Ok(CrawlResult {
        url: url.to_string(),
        status,
        title: title.or(extracted.title),
        content_markdown: extracted.markdown,
        links: extracted.links,
        metadata: meta,
        depth: 0,
        elapsed_ms: start.elapsed().as_millis() as u64,
    })
}

/// Take a full-page screenshot and return PNG bytes
pub async fn screenshot(url: &str, proxy: Option<&str>, wait_ms: u64) -> Result<Vec<u8>> {
    let browser = get_or_init_browser(proxy).await?;

    let page = browser.new_page(url).await
        .context("Failed to open new tab for screenshot")?;

    if wait_ms > 0 {
        tokio::time::sleep(std::time::Duration::from_millis(wait_ms)).await;
    }

    let png_bytes = page.screenshot(
        ScreenshotParams::builder()
            .format(CaptureScreenshotFormat::Png)
            .full_page(true)
            .build(),
    )
    .await
    .context("Failed to take screenshot")?;

    let _ = page.close().await;

    Ok(png_bytes)
}

/// Render multiple URLs in parallel (N tabs simultaneously)
pub async fn render_batch(
    urls: &[String],
    proxy: Option<&str>,
    wait_ms: u64,
    max_concurrent: usize,
) -> Vec<Result<CrawlResult>> {
    let browser = match get_or_init_browser(proxy).await {
        Ok(b) => b,
        Err(e) => return vec![Err(e)],
    };

    let semaphore = Arc::new(tokio::sync::Semaphore::new(max_concurrent));
    let mut handles = Vec::with_capacity(urls.len());

    for url in urls {
        let browser = browser.clone();
        let url = url.clone();
        let sem = semaphore.clone();

        handles.push(tokio::spawn(async move {
            let _permit = sem.acquire().await
                .map_err(|e| anyhow::anyhow!("Semaphore error: {e}"))?;

            let start = std::time::Instant::now();
            let page = browser.new_page(&url).await
                .context("Failed to open tab")?;

            if wait_ms > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(wait_ms)).await;
            }

            let html = page.content().await
                .context("Failed to get content")?;

            let title = page.evaluate("document.title")
                .await
                .ok()
                .and_then(|v| v.into_value::<String>().ok())
                .filter(|s| !s.is_empty());

            let extracted = extractor::extract(&html, &url);
            let mut meta = extracted.metadata;
            meta.content_type = Some("text/html".to_string());

            let _ = page.close().await;

            Ok(CrawlResult {
                url,
                status: 200,
                title: title.or(extracted.title),
                content_markdown: extracted.markdown,
                links: extracted.links,
                metadata: meta,
                depth: 0,
                elapsed_ms: start.elapsed().as_millis() as u64,
            })
        }));
    }

    let mut results = Vec::with_capacity(handles.len());
    for handle in handles {
        results.push(handle.await.unwrap_or_else(|e| Err(anyhow::anyhow!("Task join error: {e}"))));
    }
    results
}
