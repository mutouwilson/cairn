//! Database layer. Owns the SQLite pool and exposes typed queries.
//!
//! Migrations live in `../migrations/` and are run at startup via `sqlx::migrate!`.
//!
//! Phase 3b adds optional at-rest encryption via SQLCipher, gated by the
//! `encrypted` cargo feature **and** the runtime env `CAIRN_ENCRYPT=1`. The
//! cipher key is derived from a 32-byte random secret held in the OS keychain;
//! we *never* fall back to storing the DB-encryption key inside the DB itself.

pub mod queries;

use anyhow::{Context, Result};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Row, SqlitePool};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;

/// How many timestamped pre-migration backups to keep per database file.
const MAX_BACKUPS: usize = 10;

/// Raised when `sqlx` finds that an already-applied migration's checksum no
/// longer matches the bundled file — i.e. a shipped migration was edited or
/// the app was downgraded. We attach the path of the safety backup we took
/// (a snapshot from *before* any migration ran) so the caller can tell the
/// user exactly where a clean copy of their data is.
#[derive(Debug, thiserror::Error)]
#[error(
    "database migration conflict at version {version}: a migration that was \
     already applied has since changed. A pre-migration backup is at: {}",
    backup.as_ref().map(|p| p.display().to_string())
        .unwrap_or_else(|| "(none taken — original DB was not opened for writes)".into())
)]
pub struct MigrationConflict {
    pub version: i64,
    pub backup: Option<PathBuf>,
}

#[cfg(feature = "encrypted")]
mod encryption {
    use crate::keystore::keychain::KeychainKeyStore;
    use crate::keystore::KeyStore;
    use anyhow::{Context, Result};
    use rand::RngCore;

    pub const DB_KEY_ACCOUNT: &str = "db-key";

    /// Load or create the at-rest DB key from the OS keychain.
    ///
    /// Fails closed if the keychain is unreachable — we never persist the DB
    /// encryption key inside the DB it is supposed to be protecting.
    pub async fn passphrase_pragma() -> Result<String> {
        let kc = KeychainKeyStore::default();
        let seed = match kc.load_seed(DB_KEY_ACCOUNT).await? {
            Some(s) => s,
            None => {
                let mut seed = [0u8; 32];
                rand::rngs::OsRng.fill_bytes(&mut seed);
                kc.save_seed(DB_KEY_ACCOUNT, &seed)
                    .await
                    .context("save db key to keychain")?;
                tracing::info!("generated fresh DB encryption key in keychain");
                seed
            }
        };
        // SQLCipher raw-key format: PRAGMA key = "x'HEX'"
        Ok(format!("\"x'{}'\"", hex::encode(seed)))
    }
}

#[derive(Clone)]
pub struct Db {
    pool: Arc<SqlitePool>,
}

impl Db {
    /// Open or create the database at the given path.
    /// Runs all pending migrations.
    pub async fn open(path: &Path) -> Result<Self> {
        // Auto-extension must be registered before the first connection.
        // Once-guarded, so safe to call repeatedly.
        crate::vec_init::register();

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).context("create db dir")?;
        }

        let want_encrypt = std::env::var("CAIRN_ENCRYPT")
            .map(|v| matches!(v.as_str(), "1" | "true" | "TRUE" | "yes"))
            .unwrap_or(false);

        #[cfg(not(feature = "encrypted"))]
        if want_encrypt {
            anyhow::bail!(
                "CAIRN_ENCRYPT=1 set but binary was built without `--features encrypted`. \
                 Rebuild with: cargo build --release --features encrypted"
            );
        }

        let url = format!("sqlite://{}?mode=rwc", path.display());
        let opts = SqliteConnectOptions::from_str(&url)?
            .create_if_missing(true)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
            .synchronous(sqlx::sqlite::SqliteSynchronous::Normal)
            .foreign_keys(true)
            .busy_timeout(std::time::Duration::from_secs(5));

        let pool_opts = SqlitePoolOptions::new().max_connections(8);

        #[cfg(feature = "encrypted")]
        let pool_opts = if want_encrypt {
            let pragma = encryption::passphrase_pragma().await?;
            tracing::info!("opening encrypted SQLCipher database");
            // PRAGMA key MUST be the first statement on a fresh connection.
            pool_opts.after_connect(move |conn, _| {
                let stmt = format!("PRAGMA key = {}", pragma);
                Box::pin(async move {
                    sqlx::query(&stmt).execute(&mut *conn).await?;
                    // Force SQLCipher v4 format so the on-disk layout doesn't
                    // shift if the bundled library version drifts.
                    sqlx::query("PRAGMA cipher_compatibility = 4")
                        .execute(&mut *conn)
                        .await?;
                    Ok(())
                })
            })
        } else {
            pool_opts
        };

        // Did a real DB exist before this open? `create_if_missing` will make
        // an empty file during connect, so capture this *first* — it decides
        // whether a pre-migration backup is worth taking.
        let db_preexisted = path.exists();

        let pool = pool_opts
            .connect_with(opts)
            .await
            .context("connect sqlite")?;

        let migrator = sqlx::migrate!("./migrations");

        // Safety net: if we're about to mutate a user's existing DB (pending
        // migrations to apply), snapshot it first so a botched migration can
        // be rolled back by hand. Fresh installs and no-op startups skip this.
        let mut backup: Option<PathBuf> = None;
        if db_preexisted && has_pending_migrations(&pool, &migrator).await {
            match backup_database(&pool, path).await {
                Ok(p) => backup = Some(p),
                // A failed backup must not block startup on a healthy DB, but
                // we log loudly — the user is now migrating without a net.
                Err(e) => tracing::warn!(error = %e, "pre-migration backup failed; proceeding"),
            }
        }

        if let Err(e) = migrator.run(&pool).await {
            return match e {
                // The dangerous case: a previously-applied migration's checksum
                // changed. NOTE: sqlx 0.8 does *not* validate every checksum up
                // front — it walks versions ascending and a lower-version
                // *pending* migration can be applied+committed before a
                // higher-version mismatch trips. So the live file may already be
                // partially migrated here. That's fine: when there were pending
                // migrations we snapshotted above (before any apply), so `backup`
                // already holds a clean pre-migration copy. The re-attempt below
                // only fires for a pure mismatch (no pending → nothing applied →
                // file still untouched).
                sqlx::migrate::MigrateError::VersionMismatch(version) => {
                    if backup.is_none() && db_preexisted {
                        backup = backup_database(&pool, path).await.ok();
                    }
                    Err(anyhow::Error::new(MigrationConflict { version, backup }))
                }
                other => Err(anyhow::Error::new(other).context("run migrations")),
            };
        }

        tracing::info!(
            path = %path.display(),
            encrypted = want_encrypt,
            "database opened"
        );

        Ok(Self {
            pool: Arc::new(pool),
        })
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }
}

/// True if any bundled migration hasn't been recorded in `_sqlx_migrations`
/// yet. Best-effort: a missing table (fresh DB) reads as "everything pending",
/// but `Db::open` only calls this when the file pre-existed, so a fresh DB
/// never reaches here.
async fn has_pending_migrations(pool: &SqlitePool, migrator: &sqlx::migrate::Migrator) -> bool {
    let applied: HashSet<i64> =
        sqlx::query_scalar::<_, i64>("SELECT version FROM _sqlx_migrations")
            .fetch_all(pool)
            .await
            .unwrap_or_default()
            .into_iter()
            .collect();
    migrator.iter().any(|m| !applied.contains(&m.version))
}

/// Checkpoint the WAL into the main file, then copy it to
/// `<db_dir>/backups/<stem>-<YYYYMMDD-HHMMSS-mmm>.db`. Returns the backup
/// path. Keeps only the most recent [`MAX_BACKUPS`].
///
/// If the checkpoint can't fully fold the WAL (another connection holds a
/// lock — e.g. a second Cairn instance racing this one), the sidecar
/// `-wal`/`-shm` files are copied alongside the backup so the snapshot
/// stays restorable (SQLite re-attaches sidecars by name automatically).
async fn backup_database(pool: &SqlitePool, db_path: &Path) -> Result<PathBuf> {
    let fully_checkpointed = match sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
        .fetch_one(pool)
        .await
    {
        // PRAGMA wal_checkpoint returns (busy, log, checkpointed) as i64s.
        // `busy == 0 && log == checkpointed` means every WAL frame is now in
        // the main file — i.e. a `.db` copy on its own is complete.
        Ok(row) => {
            let busy: i64 = row.try_get(0).unwrap_or(1);
            let log: i64 = row.try_get(1).unwrap_or(-1);
            let checkpointed: i64 = row.try_get(2).unwrap_or(-1);
            let ok = busy == 0 && log >= 0 && checkpointed >= 0 && log == checkpointed;
            if !ok {
                tracing::warn!(
                    busy,
                    log,
                    checkpointed,
                    "WAL not fully checkpointed; will back up sidecars too"
                );
            }
            ok
        }
        Err(e) => {
            tracing::warn!(error = %e, "WAL checkpoint failed; will back up sidecars too");
            false
        }
    };

    let dir = db_path.parent().unwrap_or_else(|| Path::new("."));
    let backup_dir = dir.join("backups");
    std::fs::create_dir_all(&backup_dir).context("create backups dir")?;

    let stem = db_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("memory");
    // Millisecond resolution makes same-second collisions vanishingly rare
    // even when two app instances race. (Cross-process migration races are
    // a separate concern tracked elsewhere — see DEVELOPMENT.md.)
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S-%3f");
    let dest = backup_dir.join(format!("{stem}-{stamp}.db"));

    std::fs::copy(db_path, &dest)
        .with_context(|| format!("copy {} -> {}", db_path.display(), dest.display()))?;

    if !fully_checkpointed {
        copy_sidecar_if_present(db_path, &dest, "-wal");
        copy_sidecar_if_present(db_path, &dest, "-shm");
    }

    prune_backups(&backup_dir, stem, MAX_BACKUPS);
    tracing::info!(backup = %dest.display(), "backed up database before migration");
    Ok(dest)
}

/// Copy `<live_db><suffix>` to `<backup_db><suffix>` if the source exists.
/// Best-effort: failure logs a warning but does not abort the backup —
/// the main `.db` is already saved.
fn copy_sidecar_if_present(live_db: &Path, backup_db: &Path, suffix: &str) {
    let mut from = live_db.as_os_str().to_owned();
    from.push(suffix);
    let from = PathBuf::from(from);
    if !from.exists() {
        return;
    }
    let mut to = backup_db.as_os_str().to_owned();
    to.push(suffix);
    let to = PathBuf::from(to);
    if let Err(e) = std::fs::copy(&from, &to) {
        tracing::warn!(error = %e, from = %from.display(), "copy sidecar failed");
    }
}

/// Delete all but the newest `keep` backups for `stem`. Timestamped names sort
/// chronologically as plain strings, so an ascending sort puts oldest first.
/// Each removed `.db` drags its `-wal`/`-shm` sidecars with it (if any),
/// otherwise we'd leak orphan sidecars over time.
fn prune_backups(backup_dir: &Path, stem: &str, keep: usize) {
    let prefix = format!("{stem}-");
    let mut backups: Vec<PathBuf> = std::fs::read_dir(backup_dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension().and_then(|e| e.to_str()) == Some("db")
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with(&prefix))
        })
        .collect();
    backups.sort();
    if backups.len() > keep {
        for old in &backups[..backups.len() - keep] {
            if let Err(e) = std::fs::remove_file(old) {
                tracing::warn!(error = %e, path = %old.display(), "failed to prune old backup");
            }
            for suffix in ["-wal", "-shm"] {
                let mut sc = old.as_os_str().to_owned();
                sc.push(suffix);
                let _ = std::fs::remove_file(PathBuf::from(sc));
            }
        }
    }
}

pub fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prune_keeps_newest_n_and_ignores_others() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        // 13 timestamped backups for stem "memory" (names sort chronologically).
        for day in 1..=13 {
            std::fs::write(p.join(format!("memory-202605{day:02}-000000.db")), b"x").unwrap();
        }
        // Files that must be left alone: different stem + non-.db sidecar.
        std::fs::write(p.join("other-20260501-000000.db"), b"keep").unwrap();
        std::fs::write(p.join("memory-20260513-000000.db-wal"), b"keep").unwrap();

        prune_backups(p, "memory", 10);

        let mut kept: Vec<String> = std::fs::read_dir(p)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with("memory-") && n.ends_with(".db"))
            .collect();
        kept.sort();
        assert_eq!(
            kept.len(),
            10,
            "exactly the 10 newest memory backups survive"
        );
        assert_eq!(kept.first().unwrap(), "memory-20260504-000000.db"); // oldest kept
        assert_eq!(kept.last().unwrap(), "memory-20260513-000000.db"); // newest kept
        assert!(
            !p.join("memory-20260501-000000.db").exists(),
            "oldest 3 pruned"
        );
        // Unrelated entries untouched.
        assert!(p.join("other-20260501-000000.db").exists());
        assert!(p.join("memory-20260513-000000.db-wal").exists());
    }

    #[test]
    fn prune_drops_sidecars_with_their_db() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        // 12 timestamped backups; the 2 oldest have full sidecar sets.
        for day in 1..=12 {
            std::fs::write(p.join(format!("memory-202605{day:02}-000000-000.db")), b"x").unwrap();
        }
        for day in 1..=2 {
            std::fs::write(
                p.join(format!("memory-202605{day:02}-000000-000.db-wal")),
                b"x",
            )
            .unwrap();
            std::fs::write(
                p.join(format!("memory-202605{day:02}-000000-000.db-shm")),
                b"x",
            )
            .unwrap();
        }

        prune_backups(p, "memory", 10);

        // The 2 oldest .db files plus their sidecars must all be gone.
        for day in 1..=2 {
            let base = format!("memory-202605{day:02}-000000-000.db");
            assert!(!p.join(&base).exists(), "{base} should be pruned");
            assert!(
                !p.join(format!("{base}-wal")).exists(),
                "-wal for {base} should be pruned alongside it"
            );
            assert!(
                !p.join(format!("{base}-shm")).exists(),
                "-shm for {base} should be pruned alongside it"
            );
        }
        // The 10 newest are kept.
        for day in 3..=12 {
            let base = format!("memory-202605{day:02}-000000-000.db");
            assert!(p.join(&base).exists(), "{base} should survive");
        }
    }

    #[tokio::test]
    async fn fresh_open_takes_no_backup() {
        let dir = tempfile::tempdir().unwrap();
        let _db = Db::open(&dir.path().join("memory.db")).await.unwrap();
        assert!(
            !dir.path().join("backups").exists(),
            "a brand-new DB has nothing to back up"
        );
    }

    #[tokio::test]
    async fn noop_reopen_takes_no_backup() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("memory.db");
        drop(Db::open(&db_path).await.unwrap());
        // Second open: every migration already applied, nothing pending.
        drop(Db::open(&db_path).await.unwrap());
        assert!(
            !dir.path().join("backups").exists(),
            "a no-op reopen must not spam backups"
        );
    }
}
