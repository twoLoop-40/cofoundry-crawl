//! MCP (Model Context Protocol) server — JSON-RPC 2.0 over stdio (NDJSON)
//!
//! Protocol: newline-delimited JSON. Each message is a single compact JSON line.
//! Server reads from stdin, writes to stdout, logs to stderr.

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

// ─── Tool schema helpers ───

fn tool_schema_crawl_url() -> Value {
    serde_json::json!({
        "name": "crawl_url",
        "description": "Fetch a single URL and extract its content as Markdown, links, and metadata. Fast single-page crawl.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "The URL to crawl"
                },
                "timeout_secs": {
                    "type": "integer",
                    "description": "Timeout in seconds (default: 30)",
                    "default": 30
                }
            },
            "required": ["url"]
        }
    })
}

fn tool_schema_extract_content() -> Value {
    serde_json::json!({
        "name": "extract_content",
        "description": "Extract structured content (title, markdown, links, metadata) from a URL. Alias for crawl_url focused on content extraction.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "The URL to extract content from"
                }
            },
            "required": ["url"]
        }
    })
}

fn tool_schema_search_site() -> Value {
    serde_json::json!({
        "name": "search_site",
        "description": "BFS crawl a website and search for a keyword across all discovered pages. Returns ranked results with snippets.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "Starting URL for the site crawl"
                },
                "query": {
                    "type": "string",
                    "description": "Search keyword to find across pages"
                },
                "max_depth": {
                    "type": "integer",
                    "description": "Maximum crawl depth (default: 2)",
                    "default": 2
                },
                "max_pages": {
                    "type": "integer",
                    "description": "Maximum pages to crawl (default: 20)",
                    "default": 20
                }
            },
            "required": ["url", "query"]
        }
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
            None // notifications don't get responses
        }

        "tools/list" => {
            let result = serde_json::json!({
                "tools": [
                    tool_schema_crawl_url(),
                    tool_schema_extract_content(),
                    tool_schema_search_site()
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

        // Ignore unknown notifications
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
    eprintln!("[mcp] cofoundry-crawl MCP server starting (stdio)");

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
