//! Library root. The Tauri app and the standalone MCP binary both depend on
//! this crate, so all real logic lives here and `main.rs` / `bin/mcp_stdio.rs`
//! are thin entrypoints.

pub mod api;
pub mod api_server;
pub mod audit;
pub mod bundle;
pub mod capture;
pub mod commands;
pub mod consolidation;
pub mod db;
pub mod embed;
pub mod embed_pipeline;
pub mod extract;
pub mod global_shortcut;
pub mod import;
pub mod importance;
pub mod keystore;
pub mod mcp;
pub mod mcp_bridge;
pub mod providers;
pub mod retrieval;
pub mod schema;
pub mod selection_popover;
pub mod signals;
pub mod usage;
pub mod vec_init;

use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::Manager;

pub struct AppState {
    pub db: db::Db,
    pub audit: audit::AuditLogger,
    pub retriever: retrieval::Retriever,
    /// Hot-swappable extractor + embedder + embedding-model-id. Updated
    /// in-place when the user changes `/settings → AI providers`, so no
    /// restart is needed for provider changes to take effect.
    pub providers: Arc<providers::LiveProviders>,
    /// HTTP client + provider config used by the consolidation worker. Cloned
    /// when triggered manually from IPC.
    pub consolidation: Option<ConsolidationServices>,
    /// Phase 5 capture supervisor — owns the per-source poll tasks.
    pub capture: capture::worker::CaptureSupervisor,
    /// Phase 6a selection popover supervisor (macOS only at runtime, no-op
    /// elsewhere).
    pub selection: Arc<selection_popover::SelectionPopover>,
    /// Phase 6b in-app remote MCP (SSE) bridge supervisor.
    pub mcp_bridge: Arc<mcp_bridge::McpBridge>,
    /// Data dir — needed so IPC commands can resolve `providers.json` etc.
    pub data_dir: PathBuf,
    /// Snapshot of `(family, model)` used to build the live extractor at
    /// boot. Compared against the persisted config so the UI knows when a
    /// restart is needed for changes to take effect.
    pub boot_extract_summary: Option<(providers::ProviderFamily, String)>,
}

#[derive(Clone)]
pub struct ConsolidationServices {
    pub client: reqwest::Client,
    pub cfg: extract::openai_compat::OpenAiCompatConfig,
}

impl AppState {
    pub async fn init(data_dir: PathBuf) -> Result<Self> {
        // Must run before the first sqlx connection.
        vec_init::register();

        let db_path = data_dir.join("memory.db");
        let db = db::Db::open(&db_path).await?;
        usage::install(db.clone()); // global telemetry sink
        let audit = audit::AuditLogger::init(db.clone()).await?;

        // Phase 6d: load user-configured providers. Falls back to env vars
        // for users who haven't migrated yet, so existing `.env` setups keep
        // working without explicit migration.
        let providers_cfg = providers::config_or_env_default(&data_dir);
        let embedding_dim = std::env::var("CAIRN_EMBEDDING_DIM")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(embed::DEFAULT_DIM);
        let providers_live = providers::LiveProviders::new(&providers_cfg, embedding_dim);
        let retriever =
            retrieval::Retriever::with_live_providers(db.clone(), providers_live.clone());
        let embedding_model_id = providers_live.embedding_model_id();

        // Worker always spawns — it resolves the provider per tick (Settings
        // first, these env-built services as fallback), so users who add a
        // key in Settings get background consolidation without a restart.
        let consolidation = build_consolidation_services();
        consolidation::spawn_worker(db.clone(), providers_live.clone(), consolidation.clone());

        let capture = capture::worker::CaptureSupervisor::new(db.clone());
        capture.restart_all().await;

        let selection = selection_popover::SelectionPopover::new(data_dir.clone());
        let mcp_bridge = mcp_bridge::McpBridge::new(
            data_dir.clone(),
            db.clone(),
            audit.clone(),
            retriever.clone(),
            providers_live.clone(),
        );

        tracing::info!(
            ?db_path,
            embedding = %embedding_model_id,
            consolidation_enabled = consolidation.is_some(),
            "app state initialized"
        );
        Ok(Self {
            db,
            audit,
            retriever,
            providers: providers_live,
            consolidation,
            capture,
            selection,
            mcp_bridge,
            boot_extract_summary: providers_cfg
                .extract
                .as_ref()
                .map(|c| (c.family, c.model.clone())),
            data_dir,
        })
    }
}

fn build_consolidation_services() -> Option<ConsolidationServices> {
    let model = std::env::var("CAIRN_CONSOLIDATION_MODEL")
        .unwrap_or_else(|_| "anthropic/claude-sonnet-4-7".to_string());
    let (base_url, api_key, label) = if let Ok(k) = std::env::var("AI_GATEWAY_API_KEY") {
        let base = std::env::var("AI_GATEWAY_BASE_URL")
            .unwrap_or_else(|_| extract::gateway::DEFAULT_BASE_URL.to_string());
        (base, k, "gateway")
    } else if let Ok(k) = std::env::var("AI_GATEWAY_TOKEN") {
        let base = std::env::var("AI_GATEWAY_BASE_URL")
            .unwrap_or_else(|_| extract::gateway::DEFAULT_BASE_URL.to_string());
        (base, k, "gateway")
    } else if let Ok(endpoint) = std::env::var("CAIRN_LOCAL_ENDPOINT") {
        let k = std::env::var("CAIRN_LOCAL_API_KEY").unwrap_or_default();
        (endpoint, k, "local")
    } else {
        tracing::warn!(
            "consolidation worker disabled: set AI_GATEWAY_API_KEY or CAIRN_LOCAL_ENDPOINT"
        );
        return None;
    };
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(?e, "build http client failed");
            return None;
        }
    };
    Some(ConsolidationServices {
        client,
        cfg: extract::openai_compat::OpenAiCompatConfig {
            base_url,
            api_key,
            model,
            label,
        },
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("CAIRN_LOG")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .compact()
        .init();

    let _ = dotenvy::dotenv();

    let data_dir = match resolve_data_dir() {
        Ok(d) => d,
        Err(e) => {
            fatal_dialog("Cairn can't find its data folder", &format!("{e:#}"));
            std::process::exit(1);
        }
    };

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("build tokio runtime");

    let state = match runtime.block_on(AppState::init(data_dir)) {
        Ok(s) => s,
        Err(e) => {
            // A migration checksum conflict is the one fatal-startup case the
            // user can actually act on, so give it a tailored message + the
            // backup path. Everything else falls back to the raw error.
            if let Some(conflict) = e.downcast_ref::<db::MigrationConflict>() {
                let backup_line = conflict
                    .backup
                    .as_ref()
                    .map(|p| format!("\n\nA pre-migration backup is at:\n{}", p.display()))
                    .unwrap_or_else(|| {
                        "\n\nNo backup was needed — the live database was not \
                         opened for writes."
                            .into()
                    });
                fatal_dialog(
                    "Cairn can't open your memory database",
                    &format!(
                        "A database migration mismatch was detected (version {}). \
                         If any migration ran before the mismatch was caught, the \
                         backup above has your data from before that point.{}\n\n\
                         This usually means Cairn was downgraded, or a migration \
                         file changed between versions. Install the matching (or \
                         newer) Cairn build and relaunch. To start fresh instead, \
                         move the live database (and the backup) aside before \
                         relaunching.",
                        conflict.version, backup_line,
                    ),
                );
            } else {
                fatal_dialog("Cairn failed to start", &format!("{e:#}"));
            }
            std::process::exit(1);
        }
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(global_shortcut::build_plugin())
        .setup(|app| {
            if let Err(e) = global_shortcut::register(app) {
                tracing::warn!(error = %e, "global shortcut registration failed (continuing without)");
            }
            // Boot the selection-popover supervisor if the user left it
            // enabled in a previous session and AX trust is still valid.
            let state = app.state::<AppState>();
            state.selection.boot(app.handle());
            state.mcp_bridge.boot();

            // Always-on local HTTP API server for the browser extension and
            // any other localhost caller. Independent of the MCP SSE bridge
            // so the user no longer has to flip a toggle to use the extension.
            {
                let data_dir = state.data_dir.clone();
                let db = state.db.clone();
                let audit = state.audit.clone();
                let retriever = state.retriever.clone();
                let providers = Some(state.providers.clone());
                tauri::async_runtime::spawn(async move {
                    if let Err(e) =
                        api_server::boot(data_dir, db, audit, retriever, providers).await
                    {
                        tracing::error!(error = %e, "API server boot failed");
                    }
                });
            }

            // Auto-import: watch ~/.claude memory files and re-run the import
            // pipeline ~2s after any relevant change. The imported_docs sha256
            // dedup makes re-runs near-free when nothing actually changed.
            if let Err(e) = import::watcher::spawn(
                app.handle().clone(),
                state.db.clone(),
                state.audit.clone(),
                state.providers.clone(),
            ) {
                tracing::warn!(error = %e, "import watcher boot failed (continuing without)");
            }

            // Make ⌘W "hide window" instead of "close window". The process
            // stays alive (selection detector / MCP bridge / capture workers
            // depend on it) and clicking the dock icon brings the same
            // window back. Without this Tauri's default close on macOS leaves
            // a menubar-only app with no way back to the UI short of force
            // quitting.
            if let Some(win) = app.get_webview_window("main") {
                let win_for_handler = win.clone();
                win.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = win_for_handler.hide();
                    }
                });
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // Belt-and-braces — same as the per-window handler above,
                // but covers any future window we forget to wire.
                if window.label() == "main" {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .manage(state)
        .manage(runtime)
        .invoke_handler(tauri::generate_handler![
            commands::capture_note,
            commands::delete_note,
            commands::update_note,
            commands::update_entity,
            commands::create_entity,
            commands::list_entity_edits,
            commands::entity_edit_counts,
            commands::list_notes,
            commands::list_entities,
            commands::get_entity,
            commands::search,
            commands::list_agents,
            commands::add_agent,
            commands::delete_agent,
            commands::list_permissions,
            commands::grant_permission,
            commands::revoke_permission,
            commands::list_audit,
            commands::verify_audit,
            commands::audit_public_key,
            commands::reprocess_note,
            commands::reembed_pending,
            commands::list_themes,
            commands::get_theme,
            commands::list_consolidation_runs,
            commands::trigger_consolidation,
            commands::export_bundle,
            commands::import_bundle,
            commands::log_retrieval,
            commands::record_signal_action,
            commands::list_retrieval_signals,
            commands::list_usage,
            commands::usage_summary,
            commands::list_capture_sources,
            commands::add_imap_source,
            commands::add_ics_source,
            commands::set_capture_source_enabled,
            commands::delete_capture_source,
            commands::selection_status,
            commands::selection_set_enabled,
            commands::selection_request_permission,
            commands::selection_get_current,
            commands::selection_dismiss,
            commands::selection_save_with_note,
            commands::mcp_bridge_status,
            commands::mcp_bridge_set_enabled,
            commands::providers_get,
            commands::providers_save,
            commands::providers_test,
            commands::import_claude_auto_run,
            commands::read_import_source_file,
            commands::import_resync_file,
            commands::import_set_skip,
            commands::export_preview,
            commands::export_run,
            commands::vacuum_orphan_vecs,
            commands::list_dedup_candidates,
            commands::merge_entities,
            commands::reject_dedup_pair,
            commands::list_entity_aliases,
            commands::remove_entity_alias,
            commands::list_entities_ranked,
            commands::set_entity_archived,
            commands::list_archived_entities,
            commands::auto_archive_now,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            // macOS dock click / Spotlight reopen → if main window is hidden
            // (we hide instead of closing on ⌘W), bring it back. Otherwise
            // the user gets a "ghost" Cairn in the menubar with no UI.
            // `RunEvent::Reopen` is macOS-only.
            #[cfg(target_os = "macos")]
            {
                if let tauri::RunEvent::Reopen { has_visible_windows, .. } = &event {
                    if !has_visible_windows {
                        if let Some(win) = app.get_webview_window("main") {
                            let _ = win.show();
                            let _ = win.set_focus();
                        }
                    }
                }
            }
            #[cfg(not(target_os = "macos"))]
            {
                let _ = (app, event);
            }
        });
}

/// Surface a fatal startup error to a GUI user. The app is launched from
/// Finder/Explorer where stderr goes nowhere visible, so we pop a native
/// modal in addition to logging. Best-effort: if the dialog can't show
/// (headless CI, no display) the eprintln still lands in the logs.
fn fatal_dialog(title: &str, body: &str) {
    eprintln!("fatal: {title}\n{body}");
    let _ = rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Error)
        .set_title(title)
        .set_description(body)
        .set_buttons(rfd::MessageButtons::Ok)
        .show();
}

fn resolve_data_dir() -> Result<PathBuf> {
    if let Ok(dir) = std::env::var("CAIRN_DATA_DIR") {
        return Ok(PathBuf::from(dir));
    }
    // macOS  → ~/Library/Application Support/Cairn
    // Linux  → ~/.local/share/Cairn
    // Win    → %APPDATA%\Cairn
    let proj = directories::ProjectDirs::from("", "", "Cairn")
        .ok_or_else(|| anyhow::anyhow!("cannot resolve project data dir"))?;
    Ok(proj.data_dir().to_path_buf())
}
