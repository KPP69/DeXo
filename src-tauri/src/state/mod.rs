//! Application state shared with Tauri commands.

use std::sync::Mutex;

use crate::settings::paths::DataPaths;
use crate::trading::database::Database;

/// State created once at startup and managed by Tauri.
pub struct AppState {
    pub paths: DataPaths,
    pub database: Mutex<Database>,
}

impl AppState {
    pub fn new(paths: DataPaths, database: Database) -> Self {
        Self {
            paths,
            database: Mutex::new(database),
        }
    }
}
