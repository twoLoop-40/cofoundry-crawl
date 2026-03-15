//! Headless Chrome browser pool for SPA rendering + screenshots
//!
//! Uses chromiumoxide (Chrome DevTools Protocol) with tokio.
//! Supports parallel tab crawling and Tor proxy for .onion sites.

use anyhow::{Context, Result};
use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::cdp::browser_protocol::network::CookieParam;
use chromiumoxide::page::ScreenshotParams;
use chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotFormat;
use futures::StreamExt;
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::OnceCell;
use tracing::info;

use std::path::PathBuf;

use crate::config::CrawlResult;
use crate::crawler::extractor;

/// Cookie to inject into the browser before navigation
#[derive(Debug, Clone, Deserialize)]
pub struct CrawlCookie {
    pub name: String,
    pub value: String,
    #[serde(default)]
    pub domain: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
}

impl CrawlCookie {
    fn to_cookie_param(&self, url: &str) -> CookieParam {
        let mut param = CookieParam::new(self.name.clone(), self.value.clone());
        if let Some(ref domain) = self.domain {
            param.domain = Some(domain.clone());
        }
        if let Some(ref path) = self.path {
            param.path = Some(path.clone());
        }
        // Set url so Chrome can infer domain/path if not provided
        param.url = Some(url.to_string());
        param
    }
}

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

/// Open a new page with auth injection (localStorage + cookies) and navigate to URL
async fn inject_auth_and_navigate(browser: &Arc<Browser>, url: &str, cookies: &[CrawlCookie]) -> Result<chromiumoxide::Page> {
    let has_ls = cookies.iter().any(|c| c.domain.as_deref() == Some("localStorage"));

    if cookies.is_empty() {
        return browser.new_page(url).await
            .context("Failed to open new tab");
    }

    if has_ls {
        // For localStorage-based SPA auth (React/Zustand):
        // 1. Open a setup tab on the same origin → set localStorage
        // 2. Close setup tab
        // 3. Open a NEW tab at the target URL — React reads localStorage on mount
        // This works because localStorage is shared per-origin within the browser context.

        // Step 1: Setup tab — navigate to origin to get correct localStorage scope
        let origin = url::Url::parse(url)
            .map(|u| {
                let port = u.port().map(|p| format!(":{p}")).unwrap_or_default();
                format!("{}://{}{}", u.scheme(), u.host_str().unwrap_or("localhost"), port)
            })
            .unwrap_or_else(|_| url.to_string());

        let setup_page = browser.new_page(&origin).await
            .context("Failed to open setup tab")?;
        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;

        // Inject localStorage items
        for cookie in cookies {
            if cookie.domain.as_deref() == Some("localStorage") {
                let key_json = serde_json::to_string(&cookie.name).unwrap_or_default();
                let val_json = serde_json::to_string(&cookie.value).unwrap_or_default();
                let js = format!("localStorage.setItem({key_json}, {val_json})");
                setup_page.evaluate(js).await.ok();
            }
        }

        // Verify injection
        let verify = setup_page.evaluate("localStorage.getItem('safeintelligence-auth')?.length || 0").await
            .ok().and_then(|v| v.into_value::<i64>().ok()).unwrap_or(0);
        info!(ls_length = verify, "localStorage injected in setup tab");

        // Close setup tab
        let _ = setup_page.close().await;

        // Step 2: Open fresh tab at target URL — React will read localStorage on mount
        let page = browser.new_page(url).await
            .context("Failed to open target tab after localStorage injection")?;

        // Set real cookies on this tab too
        let real_cookies: Vec<CookieParam> = cookies.iter()
            .filter(|c| c.domain.as_deref() != Some("localStorage"))
            .map(|c| c.to_cookie_param(url))
            .collect();
        if !real_cookies.is_empty() {
            page.set_cookies(real_cookies).await.ok();
        }

        Ok(page)
    } else {
        let page = browser.new_page("about:blank").await
            .context("Failed to open new tab")?;
        let params: Vec<CookieParam> = cookies.iter().map(|c| c.to_cookie_param(url)).collect();
        page.set_cookies(params).await.ok();
        page.goto(url).await.context("Failed to navigate after setting cookies")?;
        Ok(page)
    }
}

/// Render a single URL with headless Chrome and extract content
pub async fn render_page(url: &str, proxy: Option<&str>, wait_ms: u64, cookies: &[CrawlCookie]) -> Result<CrawlResult> {
    let start = std::time::Instant::now();
    let browser = get_or_init_browser(proxy).await?;

    // Open page with auth injection (localStorage + cookies)
    let page = inject_auth_and_navigate(&browser, url, cookies).await?;

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
pub async fn screenshot(url: &str, proxy: Option<&str>, wait_ms: u64, cookies: &[CrawlCookie]) -> Result<Vec<u8>> {
    let browser = get_or_init_browser(proxy).await?;

    let page = inject_auth_and_navigate(&browser, url, cookies).await
        .context("Failed to open page for screenshot")?;

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
    cookies: &[CrawlCookie],
) -> Vec<Result<CrawlResult>> {
    let browser = match get_or_init_browser(proxy).await {
        Ok(b) => b,
        Err(e) => return vec![Err(e)],
    };

    let semaphore = Arc::new(tokio::sync::Semaphore::new(max_concurrent));
    let cookies = Arc::new(cookies.to_vec());
    let mut handles = Vec::with_capacity(urls.len());

    for url in urls {
        let browser = browser.clone();
        let url = url.clone();
        let sem = semaphore.clone();
        let cookies = cookies.clone();

        handles.push(tokio::spawn(async move {
            let _permit = sem.acquire().await
                .map_err(|e| anyhow::anyhow!("Semaphore error: {e}"))?;

            let start = std::time::Instant::now();
            let page = inject_auth_and_navigate(&browser, &url, &cookies).await?;

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

/// Login via API call (OAuth2 form-encoded) and return session tokens.
/// Uses HTTP POST to the auth endpoint, then returns cookies/localStorage entries
/// that can be injected into subsequent browser sessions.
pub async fn login_and_get_cookies(
    login_url: &str,
    email: &str,
    password: &str,
    proxy: Option<&str>,
    _wait_ms: u64,
    api_url_override: Option<&str>,
) -> Result<(Vec<crate::mcp::tools::CookieInfo>, String)> {
    // Parse the login URL to determine the API base
    let parsed = url::Url::parse(login_url)
        .context("Invalid login URL")?;
    let origin = format!("{}://{}", parsed.scheme(), parsed.host_str().unwrap_or("localhost"));
    let port = parsed.port().map(|p| format!(":{p}")).unwrap_or_default();

    // Determine API endpoint:
    // 1. Use explicit api_url if provided
    // 2. If login_url contains /api/, use it directly
    // 3. Otherwise, try the same origin with /api/v1/auth/login
    let api_url = if let Some(override_url) = api_url_override {
        override_url.to_string()
    } else if login_url.contains("/api/") {
        login_url.to_string()
    } else {
        format!("{origin}{port}/api/v1/auth/login")
    };

    info!(api_url = %api_url, "Attempting API login");

    // Build reqwest client with optional proxy
    let mut client_builder = reqwest::Client::builder();
    if let Some(proxy_url) = proxy {
        client_builder = client_builder.proxy(
            reqwest::Proxy::all(proxy_url)
                .context("Invalid proxy URL")?
        );
    }
    let client = client_builder.build().context("Failed to build HTTP client")?;

    // OAuth2 form-encoded login (matching FastAPI's OAuth2PasswordRequestForm)
    let response = client.post(&api_url)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(format!(
            "username={}&password={}",
            urlencoding::encode(email),
            urlencoding::encode(password)
        ))
        .send()
        .await
        .context("Login API request failed")?;

    let status = response.status();
    let body = response.text().await.context("Failed to read login response")?;

    if !status.is_success() {
        anyhow::bail!("Login failed (HTTP {status}): {body}");
    }

    // Parse the JWT response
    let token_data: serde_json::Value = serde_json::from_str(&body)
        .context("Failed to parse login response as JSON")?;

    let access_token = token_data.get("access_token")
        .and_then(|v| v.as_str())
        .context("No access_token in login response")?;

    let refresh_token = token_data.get("refresh_token")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    // Build Zustand-compatible auth state for localStorage injection
    let user_data = token_data.get("user").cloned().unwrap_or(serde_json::Value::Null);
    let workspaces = token_data.get("workspaces").cloned().unwrap_or(serde_json::json!([]));
    let active_workspace = token_data.get("active_workspace").cloned();

    let zustand_state = serde_json::json!({
        "state": {
            "user": user_data,
            "accessToken": access_token,
            "refreshToken": refresh_token,
            "isAuthenticated": true,
            "isLoading": false,
            "error": null,
            "workspaces": workspaces,
            "activeWorkspace": active_workspace
        },
        "version": 0
    });

    let cookies = vec![
        crate::mcp::tools::CookieInfo {
            name: "safeintelligence-auth".to_string(),
            value: serde_json::to_string(&zustand_state).unwrap_or_default(),
            domain: "localStorage".to_string(),
            path: String::new(),
        },
    ];

    info!(token_len = access_token.len(), "API login successful");
    Ok((cookies, format!("{origin}{port}/")))
}
