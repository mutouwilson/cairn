//! `cairn-migrate` — one-shot plaintext → SQLCipher migration.
//!
//! Usage:
//!
//! ```text
//! cairn-migrate --from /path/to/plaintext.db --to /path/to/encrypted.db
//! ```
//!
//! Steps it takes:
//!
//! 1. Sanity-check both paths (`from` exists, `to` does not).
//! 2. Open `to` as a fresh SQLCipher DB. The 32-byte page key is read or
//!    created in the OS keychain (account `db-key`, service `so.cairn.app`).
//! 3. Run all bundled `sqlx::migrate!` migrations on `to` so its schema is
//!    identical to a freshly-installed Cairn.
//! 4. `ATTACH DATABASE 'plaintext.db' AS plaintext KEY ''` and `INSERT INTO
//!    main.<t> SELECT * FROM plaintext.<t>` for every concrete data table.
//! 5. Verify row counts per table match between source and destination.
//! 6. Leave `entity_vecs` empty — virtual tables can't be SELECT-INTO'd; the
//!    user runs `reembed_pending` (or just opens the app) to rebuild them.
//!
//! Real migration logic is gated behind `--features encrypted` (SQLCipher
//! adds ~10 minutes to the first build). Without the feature the binary
//! still compiles as a stub that prints a usage hint and exits non-zero —
//! this keeps Tauri's bundler happy in release builds without forcing
//! SQLCipher onto every CI run.

#[cfg(not(feature = "encrypted"))]
fn main() {
    eprintln!(
        "cairn-migrate was built without the `encrypted` feature.\n\
         \n\
         Re-build with SQLCipher support to use this command:\n\
         \n\
         \x20   cd memory/src-tauri\n\
         \x20   cargo build --release --features encrypted --bin cairn-migrate\n\
         \n\
         Note: the first encrypted build takes ~10 min because it vendors\n\
         SQLCipher + OpenSSL."
    );
    std::process::exit(2);
}

#[cfg(feature = "encrypted")]
use anyhow::{anyhow, bail, Context, Result};
#[cfg(feature = "encrypted")]
use cairn_lib::db::Db;
#[cfg(feature = "encrypted")]
use cairn_lib::vec_init;
#[cfg(feature = "encrypted")]
use sqlx::{Connection, Executor, Row, SqliteConnection};
#[cfg(feature = "encrypted")]
use std::path::{Path, PathBuf};

/// Tables that hold concrete rows we want to carry across. Order matters
/// because of foreign keys (parents before children).
#[cfg(feature = "encrypted")]
const TABLES: &[&str] = &[
    "notes",
    "entities",
    "entity_aliases",
    "relations",
    "agents",
    "permissions",
    "audit_log",
    "audit_key",
    "entity_embedding_meta",
    "semantic_memories",
    "consolidation_runs",
    "sync_log",
    "retrieval_signals",
    "usage_log",
];

#[cfg(feature = "encrypted")]
#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("CAIRN_LOG")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .compact()
        .init();

    let _ = dotenvy::dotenv();
    let (from, to) = parse_args()?;

    if !from.exists() {
        bail!("source DB does not exist: {}", from.display());
    }
    if to.exists() {
        bail!(
            "destination already exists: {} — refuse to clobber, move or rename it first",
            to.display()
        );
    }

    tracing::info!(
        from = %from.display(),
        to = %to.display(),
        "starting plaintext → encrypted migration"
    );

    // Force CAIRN_ENCRYPT for the destination open path.
    std::env::set_var("CAIRN_ENCRYPT", "1");
    vec_init::register();

    let dest = Db::open(&to)
        .await
        .context("open encrypted destination DB (keychain access required)")?;

    // Use a fresh raw connection for the copy so we can ATTACH cleanly.
    let dest_url = format!("sqlite://{}?mode=rwc", to.display());
    let mut conn = SqliteConnection::connect(&dest_url)
        .await
        .context("re-open destination for ATTACH")?;
    // Re-apply the same key on this raw connection.
    apply_key(&mut conn).await?;

    let from_str = from.to_string_lossy().replace('\'', "''");
    conn.execute(format!("ATTACH DATABASE '{from_str}' AS plaintext KEY ''").as_str())
        .await
        .context("ATTACH plaintext source")?;

    let mut totals: Vec<(String, i64, i64)> = Vec::new();
    for table in TABLES {
        let n_src = count(&mut conn, "plaintext", table).await?;
        if n_src == 0 {
            totals.push((table.to_string(), 0, 0));
            continue;
        }
        conn.execute(format!("INSERT INTO main.{table} SELECT * FROM plaintext.{table}").as_str())
            .await
            .with_context(|| format!("copy table {table}"))?;
        let n_dst = count(&mut conn, "main", table).await?;
        if n_dst < n_src {
            bail!("row count mismatch on {table}: source={n_src} dest={n_dst}");
        }
        totals.push((table.to_string(), n_src, n_dst));
        tracing::info!(table, src = n_src, dst = n_dst, "copied");
    }

    conn.execute("DETACH DATABASE plaintext").await.ok();
    drop(conn);
    drop(dest);

    println!("\n=== migration summary ===");
    println!("source: {}", from.display());
    println!("dest:   {}", to.display());
    println!("        (vec table `entity_vecs` is empty — rebuild via the app's");
    println!("         reembed_pending IPC or just open the app once)\n");
    println!("{:<30} {:>10} {:>10}", "table", "source", "dest");
    for (t, s, d) in totals {
        println!("{:<30} {:>10} {:>10}", t, s, d);
    }
    println!("\nNext:");
    println!("  1. point CAIRN_DATA_DIR (or move the encrypted DB to the data dir)");
    println!("  2. set CAIRN_ENCRYPT=1 in your env / Claude Desktop config");
    println!("  3. open the app — it will re-embed entities on demand");

    Ok(())
}

#[cfg(feature = "encrypted")]
fn parse_args() -> Result<(PathBuf, PathBuf)> {
    let args: Vec<String> = std::env::args().collect();
    let mut from: Option<PathBuf> = None;
    let mut to: Option<PathBuf> = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--from" => {
                from = args.get(i + 1).map(PathBuf::from);
                i += 2;
            }
            "--to" => {
                to = args.get(i + 1).map(PathBuf::from);
                i += 2;
            }
            "-h" | "--help" => {
                eprintln!(
                    "usage: cairn-migrate --from <plaintext.db> --to <encrypted.db>\n\n\
                     Copies all data from a plaintext Cairn DB into a fresh\n\
                     SQLCipher-encrypted DB. The encryption key is taken from\n\
                     the OS keychain (account `db-key`, service `so.cairn.app`).\n\
                     Refuses to clobber an existing destination."
                );
                std::process::exit(0);
            }
            other => return Err(anyhow!("unknown arg: {other}")),
        }
    }
    Ok((
        from.ok_or_else(|| anyhow!("--from is required"))?,
        to.ok_or_else(|| anyhow!("--to is required"))?,
    ))
}

#[cfg(feature = "encrypted")]
async fn apply_key(conn: &mut SqliteConnection) -> Result<()> {
    use cairn_lib::keystore::keychain::KeychainKeyStore;
    use cairn_lib::keystore::KeyStore;
    let kc = KeychainKeyStore::default();
    let seed = kc.load_seed("db-key").await?.ok_or_else(|| {
        anyhow!("no db-key in keychain — open the encrypted app once first to generate it")
    })?;
    let pragma = format!("PRAGMA key = \"x'{}'\"", hex::encode(seed));
    conn.execute(pragma.as_str()).await.context("PRAGMA key")?;
    conn.execute("PRAGMA cipher_compatibility = 4").await.ok();
    Ok(())
}

#[cfg(feature = "encrypted")]
async fn count(conn: &mut SqliteConnection, schema: &str, table: &str) -> Result<i64> {
    let row = sqlx::query(&format!("SELECT COUNT(*) AS n FROM {schema}.{table}"))
        .fetch_one(&mut *conn)
        .await
        .with_context(|| format!("count {schema}.{table}"))?;
    Ok(row.try_get::<i64, _>("n").unwrap_or(0))
}

#[cfg(feature = "encrypted")]
#[allow(dead_code)]
fn _path_marker(p: &Path) -> &str {
    p.to_str().unwrap_or("<non-utf8>")
}
