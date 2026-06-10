//! Cross-release upgrade smoke test.
//!
//! Boots a temp SQLite DB against the *previous release's* migration set
//! (path passed in via `CAIRN_UPGRADE_OLD_MIGRATIONS_DIR`), seeds a sentinel
//! row using only columns that existed at that release, then opens the same
//! file with HEAD's `cairn_lib::db::Db` — which runs every migration added
//! since.
//!
//! Asserts the sentinel survives unmodified and HEAD's `_sqlx_migrations` set
//! is a prefix-preserving superset of OLD's. A failure here means a future
//! schema change would drop or corrupt user data on upgrade — exactly the
//! class of bug the manual rename incident hit.
//!
//! The test is **env-gated**: with the env var unset (the local `cargo test`
//! default) it prints a skip and exits cleanly. CI sets it from a dedicated
//! step that `git archive`s the latest `v*` tag's migrations dir to a temp
//! path, scoped so the var never leaks into the workspace test step.

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use std::path::PathBuf;
use std::str::FromStr;

/// Exact string we round-trip through the upgrade. Stronger than `starts_with`:
/// a buggy migration that appends or rewrites note text would currently pass a
/// prefix check but fails an equality check.
const SENTINEL_TEXT: &str = "upgrade sentinel — must survive every future migration";

#[tokio::test]
async fn upgrade_from_prev_release_preserves_data() {
    let old_dir = match std::env::var("CAIRN_UPGRADE_OLD_MIGRATIONS_DIR") {
        Ok(v) if !v.is_empty() => PathBuf::from(v),
        _ => {
            eprintln!(
                "skip: CAIRN_UPGRADE_OLD_MIGRATIONS_DIR not set (this test is \
                 gated to CI, which extracts the previous release's migrations \
                 dir into a temp path and points the var at it)"
            );
            return;
        }
    };
    assert!(
        old_dir.is_dir(),
        "CAIRN_UPGRADE_OLD_MIGRATIONS_DIR={} is not a directory",
        old_dir.display()
    );

    // The sqlite-vec auto-extension must be registered before any pool opens,
    // otherwise the old `…_vector.sql` migration's `CREATE VIRTUAL TABLE …
    // USING vec0(…)` fails. Once-guarded inside the lib.
    cairn_lib::vec_init::register();

    let tmp = tempfile::tempdir().expect("create tempdir");
    let db_path = tmp.path().join("memory.db");
    let url = format!("sqlite://{}?mode=rwc", db_path.display());

    // ----- Phase A: apply the OLD migration set on a fresh DB --------------
    let old_migrator = sqlx::migrate::Migrator::new(old_dir.as_path())
        .await
        .expect("load old migrator");
    // Mirror Db::open's connect options. The initial migration runs
    // `PRAGMA journal_mode = WAL` inside sqlx's per-migration transaction;
    // that statement is a no-op (and so legal) when the connection is already
    // in WAL mode, but errors with "cannot change into wal mode from within a
    // transaction" if the DB is still in the default journal mode.
    let opts = SqliteConnectOptions::from_str(&url)
        .expect("parse connect url")
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .foreign_keys(true);
    // `max_connections(1)` keeps Phase A a single-writer pass — the WAL
    // pragma stays well-defined and there's no risk of an idle reader holding
    // a snapshot that blocks the later checkpoint. Don't "optimize" this up.
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(opts)
        .await
        .expect("connect for old-migration pass");
    old_migrator.run(&pool).await.expect("apply old migrations");

    // Seed a sentinel using only columns that existed at the baseline. Every
    // migration added since is `ALTER TABLE ADD COLUMN`, so this INSERT keeps
    // working — that's part of what we're proving.
    let note_id = "01TESTNOTEUPGRADESENTINEL".to_string();
    sqlx::query(
        "INSERT INTO notes (id, text, source, created_at, processing_status) \
         VALUES (?, ?, 'manual', 1700000000000, 'done')",
    )
    .bind(&note_id)
    .bind(SENTINEL_TEXT)
    .execute(&pool)
    .await
    .expect("insert sentinel note");

    let entity_id = "01TESTENTITYUPGRADESENTINEL".to_string();
    sqlx::query(
        "INSERT INTO entities \
             (id, type, name, properties, confidence, source_note_id, \
              created_at, updated_at, last_accessed) \
         VALUES (?, 'Person', 'Sentinel Sam', '{}', 0.9, ?, \
                 1700000000000, 1700000000000, 1700000000000)",
    )
    .bind(&entity_id)
    .bind(&note_id)
    .execute(&pool)
    .await
    .expect("insert sentinel entity");

    // Snapshot the version set the OLD migrator just applied. After Phase B
    // we'll compare against HEAD's set — that comparison is the *only* check
    // that proves Phase B actually ran new migrations; a hard-coded
    // "is column X present?" canary is vacuous because any column we hard-code
    // here might already exist in the OLD set.
    let old_versions: Vec<i64> =
        sqlx::query_scalar("SELECT version FROM _sqlx_migrations ORDER BY version")
            .fetch_all(&pool)
            .await
            .expect("snapshot old _sqlx_migrations versions");
    assert!(
        !old_versions.is_empty(),
        "OLD migrator should have recorded at least one applied version"
    );

    pool.close().await;

    // ----- Phase B: open the same file via HEAD's Db, running new migrations
    // This is exactly what a user's app does after an upgrade.
    let db = cairn_lib::db::Db::open(&db_path)
        .await
        .expect("Db::open after upgrade");

    // Sentinel rows survived the upgrade.
    let n_notes: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM notes WHERE id = ?")
        .bind(&note_id)
        .fetch_one(db.pool())
        .await
        .expect("count sentinel note");
    assert_eq!(n_notes, 1, "sentinel note must survive the upgrade");

    let n_entities: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM entities WHERE id = ?")
        .bind(&entity_id)
        .fetch_one(db.pool())
        .await
        .expect("count sentinel entity");
    assert_eq!(n_entities, 1, "sentinel entity must survive the upgrade");

    // Sentinel text wasn't garbled by, say, a column-rewrite migration.
    // Equality (not prefix match) — a migration that appended a suffix would
    // pass `starts_with` but corrupts the user's data.
    let text: String = sqlx::query_scalar("SELECT text FROM notes WHERE id = ?")
        .bind(&note_id)
        .fetch_one(db.pool())
        .await
        .expect("fetch sentinel note text");
    assert_eq!(text, SENTINEL_TEXT, "sentinel text was modified by upgrade");

    // The real upgrade-correctness check: HEAD's applied-versions set must be
    // a *prefix-preserving superset* of OLD's. This proves three things at
    // once:
    //   1. Every OLD version is still recorded (no migration was un-applied).
    //   2. No OLD version was reordered (sqlx applies strictly ascending).
    //   3. Any HEAD-only migrations appended past the OLD prefix actually ran.
    // It also stays correct when HEAD == OLD (today's reality): the slices
    // are equal, `starts_with` is trivially true.
    let new_versions: Vec<i64> =
        sqlx::query_scalar("SELECT version FROM _sqlx_migrations ORDER BY version")
            .fetch_all(db.pool())
            .await
            .expect("snapshot new _sqlx_migrations versions");
    assert!(
        new_versions.starts_with(&old_versions),
        "HEAD's applied-migration set must be a prefix-preserving superset of \
         OLD's. old={old_versions:?}, new={new_versions:?}"
    );
}
