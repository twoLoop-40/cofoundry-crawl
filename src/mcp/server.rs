// MCP server integration — placeholder for rmcp v1 API
// TODO: Wire up rmcp macros once API is stable

use anyhow::Result;

pub async fn run_mcp_server() -> Result<()> {
    tracing::info!("Starting cofoundry-crawl MCP server (stdio)");
    eprintln!("MCP server not yet implemented. Use CLI commands instead:");
    eprintln!("  cofoundry-crawl crawl <url>");
    eprintln!("  cofoundry-crawl site <url>");
    Ok(())
}
