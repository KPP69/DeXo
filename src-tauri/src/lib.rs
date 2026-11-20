//! DeXo backend library: startup, portable data directory, database and
//! logging foundation, and the Tauri command surface for the frontend.
//!
//! Trading logic lives entirely in this Rust backend. The frontend only
//! renders UI and calls commands; it never touches exchange APIs, account
//! credentials, or the database directly.

pub mod accounts;
pub mod data;
pub mod error_report;
pub mod exchanges;
pub mod execution;
pub mod models;
pub mod risk;
pub mod settings;
pub mod state;
pub mod strategies;
pub mod trading;

use serde::Serialize;
use tauri::{WebviewUrl, WebviewWindowBuilder};
use tracing::{error, info};

use settings::logging::init_logging;
use settings::paths::{data_dir, ensure_data_dirs, DataPaths};
use state::AppState;
use trading::database;

/// Startup status reported to the frontend overview page.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStatus {
    pub version: String,
    pub data_dir: String,
    pub database_path: String,
    pub database_ready: bool,
}

#[tauri::command]
fn app_status(state: tauri::State<'_, AppState>) -> AppStatus {
    let database_ready = state
        .database
        .lock()
        .map(|db| db.schema_version().ok().flatten().is_some())
        .unwrap_or(false);
    AppStatus {
        version: env!("CARGO_PKG_VERSION").to_string(),
        data_dir: state.paths.root.display().to_string(),
        database_path: state.paths.database_file.display().to_string(),
        database_ready,
    }
}

/// Runs one startup stage; on failure shows a native error dialog naming
/// the stage and exits without falling back to any system directory.
fn stage<T, E: std::fmt::Display>(name: &str, result: Result<T, E>) -> T {
    match result {
        Ok(value) => value,
        Err(e) => {
            error!(stage = name, error = %e, "startup stage failed");
            error_report::show_fatal_error(name, &e.to_string());
            std::process::exit(1);
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let root = stage("数据目录解析", data_dir());
    let paths = DataPaths::from_root(root);
    stage("数据目录初始化", ensure_data_dirs(&paths));
    init_logging(&paths);
    info!(version = env!("CARGO_PKG_VERSION"), data_dir = %paths.root.display(), "DeXo starting");

    let database = stage("数据库初始化", database::initialise(&paths.database_file));
    info!(database = %paths.database_file.display(), "database ready");

    let webview_dir = paths.webview.clone();
    let app_state = AppState::new(paths, database);

    let build_result = tauri::Builder::default()
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![app_status])
        .setup(move |app| {
            // The main window is created in Rust so the WebView data
            // directory can be pinned to an absolute path inside the
            // portable data/ directory instead of LocalAppData.
            let mut builder =
                WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                    .title("DeXo")
                    .inner_size(1280.0, 800.0)
                    .min_inner_size(960.0, 600.0);
            #[cfg(target_os = "windows")]
            {
                builder = builder.data_directory(webview_dir.clone());
            }
            builder.build()?;
            Ok(())
        })
        .run(tauri::generate_context!());

    if let Err(e) = build_result {
        error!(error = %e, "failed to run DeXo");
        error_report::show_fatal_error("应用启动", &e.to_string());
        std::process::exit(1);
    }
}
