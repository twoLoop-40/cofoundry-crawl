//! MCP (Model Context Protocol) server — JSON-RPC 2.0 over stdio (NDJSON)
//!
//! Protocol: newline-delimited JSON. Each message is a single compact JSON line.
//! Server reads from stdin, writes to stdout, logs to stderr.
//!
//! Tools: crawl_url, extract_content, search_site, screenshot, render_batch, login

use super::tools;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader};

// ─── JSON-RPC types ───

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct JsonRpcRequest {
    jsonrpc: String,
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: &'static str,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
struct JsonRpcError {
    code: i32,
    message: String,
}

impl JsonRpcResponse {
    fn success(id: Value, result: Value) -> Self {
        Self { jsonrpc: "2.0", id, result: Some(result), error: None }
    }

    fn error(id: Value, code: i32, message: String) -> Self {
        Self { jsonrpc: "2.0", id, result: None, error: Some(JsonRpcError { code, message }) }
    }
}

// ─── Shared schema fragments ───

fn cookie_prop() -> Value {
    serde_json::json!({
        "cookies": {
            "type": "array",
            "description": "Cookies to inject before navigation (for authenticated crawling). Get cookies via the login tool first.",
            "items": {
                "type": "object",
                "properties": {
                    "name": { "type": "string" },
                    "value": { "type": "string" },
                    "domain": { "type": "string" },
                    "path": { "type": "string" }
                },
                "required": ["name", "value"]
            }
        }
    })
}

fn render_props() -> Value {
    let mut props = serde_json::json!({
        "render": {
            "type": "boolean",
            "description": "Use headless Chrome for JavaScript rendering (SPA support). Default: false",
            "default": false
        },
        "proxy": {
            "type": "string",
            "description": "SOCKS5 proxy URL (e.g., socks5://127.0.0.1:9050 for Tor/.onion sites)"
        },
        "wait_ms": {
            "type": "integer",
            "description": "Milliseconds to wait after page load for JS rendering (default: 1500)",
            "default": 1500
        }
    });
    // Merge cookie prop
    if let (Some(p), Some(c)) = (props.as_object_mut(), cookie_prop().as_object()) {
        p.extend(c.iter().map(|(k, v)| (k.clone(), v.clone())));
    }
    props
}

// ─── Tool schemas ───

fn tool_schema_crawl_url() -> Value {
    let mut props = serde_json::json!({
        "url": { "type": "string", "description": "The URL to crawl" },
        "timeout_secs": { "type": "integer", "description": "Timeout in seconds (default: 30)", "default": 30 }
    });
    // Merge render props
    if let (Some(p), Some(r)) = (props.as_object_mut(), render_props().as_object()) {
        p.extend(r.iter().map(|(k, v)| (k.clone(), v.clone())));
    }
    serde_json::json!({
        "name": "crawl_url",
        "description": "Fetch a URL and extract content as Markdown + links + metadata. Set render=true for SPA/JavaScript sites. Set proxy for Tor/.onion crawling.",
        "inputSchema": { "type": "object", "properties": props, "required": ["url"] }
    })
}

fn tool_schema_extract_content() -> Value {
    let mut props = serde_json::json!({
        "url": { "type": "string", "description": "The URL to extract content from" }
    });
    if let (Some(p), Some(r)) = (props.as_object_mut(), render_props().as_object()) {
        p.extend(r.iter().map(|(k, v)| (k.clone(), v.clone())));
    }
    serde_json::json!({
        "name": "extract_content",
        "description": "Extract structured content (title, markdown, links, metadata) from a URL. Supports SPA rendering and Tor proxy.",
        "inputSchema": { "type": "object", "properties": props, "required": ["url"] }
    })
}

fn tool_schema_search_site() -> Value {
    serde_json::json!({
        "name": "search_site",
        "description": "BFS crawl a website and search for a keyword across all discovered pages. Returns ranked results with snippets.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "url": { "type": "string", "description": "Starting URL for the site crawl" },
                "query": { "type": "string", "description": "Search keyword to find across pages" },
                "max_depth": { "type": "integer", "description": "Maximum crawl depth (default: 2)", "default": 2 },
                "max_pages": { "type": "integer", "description": "Maximum pages to crawl (default: 20)", "default": 20 },
                "render": { "type": "boolean", "description": "Use headless Chrome for SPA sites", "default": false },
                "proxy": { "type": "string", "description": "SOCKS5 proxy for Tor/.onion" }
            },
            "required": ["url", "query"]
        }
    })
}

fn tool_schema_screenshot() -> Value {
    let mut props = serde_json::json!({
        "url": { "type": "string", "description": "The URL to screenshot" },
        "proxy": { "type": "string", "description": "SOCKS5 proxy for Tor/.onion" },
        "wait_ms": { "type": "integer", "description": "Wait time for JS rendering (default: 1500ms)", "default": 1500 }
    });
    if let (Some(p), Some(c)) = (props.as_object_mut(), cookie_prop().as_object()) {
        p.extend(c.iter().map(|(k, v)| (k.clone(), v.clone())));
    }
    serde_json::json!({
        "name": "screenshot",
        "description": "Take a full-page screenshot of a URL using headless Chrome. Returns base64-encoded PNG. Supports cookies for authenticated pages.",
        "inputSchema": { "type": "object", "properties": props, "required": ["url"] }
    })
}

fn tool_schema_login() -> Value {
    serde_json::json!({
        "name": "login",
        "description": "Login via API (OAuth2 form-encoded) and return session tokens as localStorage/cookie entries. Use the returned cookies with other tools to crawl authenticated pages. Workflow: 1) call login → get cookies, 2) pass cookies to crawl_url/screenshot/render_batch.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "url": { "type": "string", "description": "Login page URL (used to determine origin)" },
                "email": { "type": "string", "description": "Email or username" },
                "password": { "type": "string", "description": "Password" },
                "api_url": { "type": "string", "description": "Direct API login endpoint URL (overrides auto-detection). E.g., http://localhost:8002/api/v1/auth/login" },
                "proxy": { "type": "string", "description": "SOCKS5 proxy for Tor/.onion" },
                "wait_ms": { "type": "integer", "description": "Wait time (default: 1500ms)", "default": 1500 }
            },
            "required": ["url", "email", "password"]
        }
    })
}

fn tool_schema_render_batch() -> Value {
    let mut props = serde_json::json!({
        "urls": {
            "type": "array",
            "items": { "type": "string" },
            "description": "List of URLs to render in parallel"
        },
        "proxy": { "type": "string", "description": "SOCKS5 proxy for Tor/.onion" },
        "wait_ms": { "type": "integer", "description": "Wait time per page (default: 1500ms)", "default": 1500 },
        "max_concurrent": { "type": "integer", "description": "Max parallel tabs (default: 5)", "default": 5 }
    });
    if let (Some(p), Some(c)) = (props.as_object_mut(), cookie_prop().as_object()) {
        p.extend(c.iter().map(|(k, v)| (k.clone(), v.clone())));
    }
    serde_json::json!({
        "name": "render_batch",
        "description": "Render multiple URLs in parallel using headless Chrome. Returns content for all pages. Supports cookies for authenticated crawling.",
        "inputSchema": { "type": "object", "properties": props, "required": ["urls"] }
    })
}

// ─── Tool dispatch ───

async fn handle_tool_call(name: &str, args: Value) -> Result<Value, String> {
    match name {
        "crawl_url" => {
            let input: tools::CrawlUrlInput = serde_json::from_value(args)
                .map_err(|e| format!("Invalid arguments for crawl_url: {e}"))?;
            let result = tools::exec_crawl_url(input).await
                .map_err(|e| format!("crawl_url failed: {e}"))?;
            serde_json::to_value(&result)
                .map_err(|e| format!("Serialization error: {e}"))
        }
        "extract_content" => {
            let input: tools::ExtractContentInput = serde_json::from_value(args)
                .map_err(|e| format!("Invalid arguments for extract_content: {e}"))?;
            let result = tools::exec_extract_content(input).await
                .map_err(|e| format!("extract_content failed: {e}"))?;
            serde_json::to_value(&result)
                .map_err(|e| format!("Serialization error: {e}"))
        }
        "search_site" => {
            let input: tools::SearchSiteInput = serde_json::from_value(args)
                .map_err(|e| format!("Invalid arguments for search_site: {e}"))?;
            let result = tools::exec_search_site(input).await
                .map_err(|e| format!("search_site failed: {e}"))?;
            serde_json::to_value(&result)
                .map_err(|e| format!("Serialization error: {e}"))
        }
        "screenshot" => {
            let input: tools::ScreenshotInput = serde_json::from_value(args)
                .map_err(|e| format!("Invalid arguments for screenshot: {e}"))?;
            let result = tools::exec_screenshot(input).await
                .map_err(|e| format!("screenshot failed: {e}"))?;
            serde_json::to_value(&result)
                .map_err(|e| format!("Serialization error: {e}"))
        }
        "render_batch" => {
            let input: tools::RenderBatchInput = serde_json::from_value(args)
                .map_err(|e| format!("Invalid arguments for render_batch: {e}"))?;
            let result = tools::exec_render_batch(input).await
                .map_err(|e| format!("render_batch failed: {e}"))?;
            serde_json::to_value(&result)
                .map_err(|e| format!("Serialization error: {e}"))
        }
        "login" => {
            let input: tools::LoginInput = serde_json::from_value(args)
                .map_err(|e| format!("Invalid arguments for login: {e}"))?;
            let result = tools::exec_login(input).await
                .map_err(|e| format!("login failed: {e}"))?;
            serde_json::to_value(&result)
                .map_err(|e| format!("Serialization error: {e}"))
        }
        _ => Err(format!("Unknown tool: {name}")),
    }
}

// ─── Request handler ───

async fn handle_request(req: JsonRpcRequest) -> Option<JsonRpcResponse> {
    let id = req.id.clone();

    match req.method.as_str() {
        "initialize" => {
            let result = serde_json::json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {
                    "tools": { "listChanged": false }
                },
                "serverInfo": {
                    "name": "cofoundry-crawl",
                    "version": env!("CARGO_PKG_VERSION")
                }
            });
            Some(JsonRpcResponse::success(id.unwrap_or(Value::Null), result))
        }

        "notifications/initialized" => {
            eprintln!("[mcp] Client initialized");
            None
        }

        "tools/list" => {
            let result = serde_json::json!({
                "tools": [
                    tool_schema_login(),
                    tool_schema_crawl_url(),
                    tool_schema_extract_content(),
                    tool_schema_search_site(),
                    tool_schema_screenshot(),
                    tool_schema_render_batch()
                ]
            });
            Some(JsonRpcResponse::success(id.unwrap_or(Value::Null), result))
        }

        "tools/call" => {
            let id = id.unwrap_or(Value::Null);
            let tool_name = req.params.get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let arguments = req.params.get("arguments")
                .cloned()
                .unwrap_or(Value::Object(serde_json::Map::new()));

            eprintln!("[mcp] tools/call: {tool_name}");

            match handle_tool_call(tool_name, arguments).await {
                Ok(value) => {
                    let text = serde_json::to_string_pretty(&value).unwrap_or_default();
                    let result = serde_json::json!({
                        "content": [{ "type": "text", "text": text }],
                        "isError": false
                    });
                    Some(JsonRpcResponse::success(id, result))
                }
                Err(err_msg) => {
                    if err_msg.starts_with("Unknown tool:") {
                        Some(JsonRpcResponse::error(id, -32602, err_msg))
                    } else {
                        let result = serde_json::json!({
                            "content": [{ "type": "text", "text": err_msg }],
                            "isError": true
                        });
                        Some(JsonRpcResponse::success(id, result))
                    }
                }
            }
        }

        method if method.starts_with("notifications/") => {
            eprintln!("[mcp] Ignoring notification: {method}");
            None
        }

        _ => {
            let id = id.unwrap_or(Value::Null);
            Some(JsonRpcResponse::error(id, -32601, format!("Method not found: {}", req.method)))
        }
    }
}

// ─── Main loop ───

pub async fn run_mcp_server() -> Result<()> {
    eprintln!("[mcp] cofoundry-crawl v{} MCP server starting (stdio)", env!("CARGO_PKG_VERSION"));
    eprintln!("[mcp] Tools: login, crawl_url, extract_content, search_site, screenshot, render_batch");

    let stdin = io::stdin();
    let mut stdout = io::stdout();
    let reader = BufReader::new(stdin);
    let mut lines = reader.lines();

    while let Some(line) = lines.next_line().await? {
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }

        let req: JsonRpcRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[mcp] Parse error: {e}");
                let resp = JsonRpcResponse::error(
                    Value::Null,
                    -32700,
                    format!("Parse error: {e}"),
                );
                let out = serde_json::to_string(&resp)? + "\n";
                stdout.write_all(out.as_bytes()).await?;
                stdout.flush().await?;
                continue;
            }
        };

        if let Some(resp) = handle_request(req).await {
            let out = serde_json::to_string(&resp)? + "\n";
            stdout.write_all(out.as_bytes()).await?;
            stdout.flush().await?;
        }
    }

    eprintln!("[mcp] stdin closed, shutting down");
    Ok(())
}
