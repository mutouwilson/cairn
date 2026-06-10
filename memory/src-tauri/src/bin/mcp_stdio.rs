//! Standalone MCP server binary.
//!
//! Two transports:
//!   * `--transport stdio` (default) — JSON-RPC on stdin/stdout. Launched as a
//!     subprocess by MCP-capable clients (Claude Desktop, Cursor, …).
//!   * `--transport sse --port 7717 [--host 127.0.0.1]` — HTTP server speaking
//!     the MCP "SSE" flavour. Designed to sit behind a Cloudflare Tunnel /
//!     Tailscale Funnel so web clients like ChatGPT's Connector UI can reach
//!     this user's local Cairn.
//!
//! Configure the data directory with `CAIRN_DATA_DIR`. Default is the OS
//! user app-data dir (e.g. `~/Library/Application Support/Cairn`).

use cairn_lib::{audit::AuditLogger, db::Db, embed, mcp, retrieval, vec_init};
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("CAIRN_LOG")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .compact()
        .init();

    let _ = dotenvy::dotenv();
    let cli = Cli::parse();

    let data_dir = resolve_data_dir()?;
    vec_init::register();
    let db_path = data_dir.join("memory.db");
    let db = Db::open(&db_path).await?;
    let audit = AuditLogger::init(db.clone()).await?;
    let embedder = embed::build_default();
    let retriever = retrieval::Retriever::new(db.clone(), embedder);

    tracing::info!(?db_path, public_key = %audit.public_key_b64(), "audit ready");

    let ctx = mcp::McpContext::new(db, audit, retriever);

    match cli.transport.as_str() {
        "stdio" => mcp::serve_stdio(ctx).await?,
        "sse" => {
            let addr = SocketAddr::new(cli.host, cli.port);
            mcp::serve_sse(ctx, addr).await?;
        }
        other => anyhow::bail!("unknown --transport `{}` (expected stdio or sse)", other),
    }
    Ok(())
}

struct Cli {
    transport: String,
    host: IpAddr,
    port: u16,
}

impl Cli {
    fn parse() -> Self {
        // Minimal arg parser — keep cairn-mcp dependency-free of clap.
        let mut transport = std::env::var("CAIRN_MCP_TRANSPORT").unwrap_or_else(|_| "stdio".into());
        let mut host: IpAddr = "127.0.0.1".parse().unwrap();
        let mut port: u16 = std::env::var("CAIRN_MCP_PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(7717);
        let mut args = std::env::args().skip(1).peekable();
        while let Some(a) = args.next() {
            match a.as_str() {
                "--transport" => {
                    if let Some(v) = args.next() {
                        transport = v;
                    }
                }
                "--port" => {
                    if let Some(v) = args.next() {
                        if let Ok(p) = v.parse::<u16>() {
                            port = p;
                        }
                    }
                }
                "--host" => {
                    if let Some(v) = args.next() {
                        if let Ok(h) = v.parse::<IpAddr>() {
                            host = h;
                        }
                    }
                }
                "--help" | "-h" => {
                    eprintln!(
                        "cairn-mcp — Cairn's MCP server\n\n\
                         USAGE:\n  \
                            cairn-mcp [--transport stdio|sse] [--port 7717] [--host 127.0.0.1]\n\n\
                         ENV:\n  \
                            CAIRN_DATA_DIR        override data dir\n  \
                            CAIRN_MCP_TRANSPORT   default transport when no flag passed\n  \
                            CAIRN_MCP_PORT        default port (sse mode)"
                    );
                    std::process::exit(0);
                }
                _ => {}
            }
        }
        Self {
            transport,
            host,
            port,
        }
    }
}

fn resolve_data_dir() -> anyhow::Result<PathBuf> {
    if let Ok(dir) = std::env::var("CAIRN_DATA_DIR") {
        return Ok(PathBuf::from(dir));
    }
    let proj = directories::ProjectDirs::from("", "", "Cairn")
        .ok_or_else(|| anyhow::anyhow!("cannot resolve project data dir"))?;
    Ok(proj.data_dir().to_path_buf())
}
