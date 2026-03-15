mod config;
mod crawler;
mod mcp;

use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "cofoundry-crawl", version, about = "High-performance web crawler with MCP server")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start MCP server (stdio transport)
    Serve,
    /// Crawl a single URL
    Crawl {
        /// URL to crawl
        url: String,
        /// Output format: json or markdown
        #[arg(short, long, default_value = "json")]
        format: String,
    },
    /// BFS crawl a site
    Site {
        /// Starting URL
        url: String,
        /// Maximum depth
        #[arg(short, long, default_value = "3")]
        depth: usize,
        /// Maximum pages
        #[arg(short, long, default_value = "100")]
        max_pages: usize,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("cofoundry_crawl=info".parse()?))
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Serve => {
            mcp::server::run_mcp_server().await?;
        }
        Commands::Crawl { url, format } => {
            let config = config::CrawlConfig::default();
            let crawler = crawler::Crawler::new(config)?;
            let result = crawler.crawl_url(&url).await?;

            match format.as_str() {
                "markdown" | "md" => println!("{}", result.content_markdown),
                _ => println!("{}", serde_json::to_string_pretty(&result)?),
            }
        }
        Commands::Site { url, depth, max_pages } => {
            let config = config::CrawlConfig {
                max_depth: depth,
                max_pages,
                ..Default::default()
            };
            let mut crawler = crawler::Crawler::new(config)?;
            let result = crawler.crawl_site(&url).await?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
    }

    Ok(())
}
