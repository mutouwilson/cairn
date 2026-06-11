//! One-shot consolidation repro against a COPY of a live DB, with full logs.
//!
//! Usage:
//!   CAIRN_REPRO_DB=/tmp/cairn_repro/memory.db \
//!   CAIRN_REPRO_PROVIDERS="$HOME/Library/Application Support/Cairn/providers.json" \
//!   RUST_BACKTRACE=1 RUST_LOG=debug \
//!   cargo run --example consolidate_repro
//!
//! Never point CAIRN_REPRO_DB at the live database — migrations/writes run.

use cairn_lib::extract::openai_compat::OpenAiCompatConfig;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,cairn_lib=debug".into()),
        )
        .init();

    let db_path = std::env::var("CAIRN_REPRO_DB").expect("set CAIRN_REPRO_DB");
    let providers_path = std::env::var("CAIRN_REPRO_PROVIDERS").expect("set CAIRN_REPRO_PROVIDERS");

    let db = cairn_lib::db::Db::open(std::path::Path::new(&db_path)).await?;

    let raw = std::fs::read_to_string(&providers_path)?;
    let cfg_json: serde_json::Value = serde_json::from_str(&raw)?;
    let ex = &cfg_json["extract"];
    let family = ex["family"].as_str().unwrap_or("vercel-gateway");
    let base_default = match family {
        "openrouter" => "https://openrouter.ai/api/v1",
        "openai" => "https://api.openai.com/v1",
        _ => "https://ai-gateway.vercel.sh/v1",
    };
    let cfg = OpenAiCompatConfig {
        base_url: ex["base_url_override"]
            .as_str()
            .unwrap_or(base_default)
            .to_string(),
        api_key: ex["api_key"].as_str().unwrap_or("").to_string(),
        model: ex["model"].as_str().unwrap_or("").to_string(),
        label: "repro",
    };
    eprintln!(
        "repro: db={db_path} family={family} model={} key_len={}",
        cfg.model,
        cfg.api_key.len()
    );

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()?;

    let stats = cairn_lib::consolidation::run_consolidation(&db, &client, &cfg, "repro").await?;
    println!("{}", serde_json::to_string_pretty(&stats)?);
    Ok(())
}
