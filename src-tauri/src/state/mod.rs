//! Application state shared with Tauri commands.

use std::sync::Mutex;

use crate::accounts::account_manager::AccountManager;
use crate::settings::paths::DataPaths;
use crate::trading::database::Database;

/// State created once at startup and managed by Tauri.
pub struct AppState {
    pub paths: DataPaths,
    pub database: Mutex<Database>,
    pub accounts: Mutex<AccountManager>,
}

impl AppState {
    pub fn new(paths: DataPaths, database: Database, accounts: AccountManager) -> Self {
        Self {
            paths,
            database: Mutex::new(database),
            accounts: Mutex::new(accounts),
        }
    }
}
