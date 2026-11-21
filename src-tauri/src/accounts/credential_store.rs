//! Credential storage (Phase 3).
//!
//! Credentials are stored separately from account configuration, behind
//! the `CredentialStore` trait. Two backends are planned behind this
//! trait: the operating system's credential store (default) and a
//! master-password encrypted vault under `data/credentials/` for
//! portable mode. Plaintext credential files are never a backend.
//!
//! This phase defines the interface and an in-memory backend used by
//! tests and development. The OS and vault backends land with the
//! account UI wiring; until then no real credential is persisted, which
//! is the safe default.
//!
//! `Credentials` values must never appear in logs, error messages or the
//! frontend. Its `Debug` implementation prints field names only.

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::sync::Mutex;

/// A set of named credential fields for one account, for example
/// `agent_private_key` for a Hyperliquid agent wallet. Field names are
/// venue-defined; values are secrets.
#[derive(Clone, PartialEq, Eq)]
pub struct Credentials {
    fields: BTreeMap<String, String>,
}

impl Credentials {
    pub fn new() -> Self {
        Self {
            fields: BTreeMap::new(),
        }
    }

    pub fn set(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.fields.insert(name.into(), value.into());
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.fields.get(name).map(String::as_str)
    }

    /// Names of the stored fields — safe to log or display, unlike values.
    pub fn field_names(&self) -> impl Iterator<Item = &str> {
        self.fields.keys().map(String::as_str)
    }

    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }
}

impl Default for Credentials {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Redacted on purpose: secret values must never reach a log line
        // or a panic message through Debug formatting.
        f.debug_struct("Credentials")
            .field("fields", &self.fields.keys().collect::<Vec<_>>())
            .field("values", &"***")
            .finish()
    }
}

#[derive(Debug)]
pub enum CredentialError {
    /// The backend failed (OS store error, vault I/O, ...).
    Backend(String),
    /// No usable backend is available on this system yet.
    Unavailable(String),
}

impl fmt::Display for CredentialError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Backend(m) => write!(f, "credential store error: {m}"),
            Self::Unavailable(m) => write!(f, "credential store unavailable: {m}"),
        }
    }
}

impl std::error::Error for CredentialError {}

/// Storage for account credentials, strictly separated from account
/// configuration files. Implementations must not log values.
pub trait CredentialStore: Send + Sync {
    /// Short backend identifier for diagnostics, e.g. "os-keyring".
    fn backend_name(&self) -> &str;

    fn put(&self, account_id: &str, credentials: &Credentials) -> Result<(), CredentialError>;

    fn get(&self, account_id: &str) -> Result<Option<Credentials>, CredentialError>;

    /// Removes the account's credentials; deleting a missing entry is
    /// not an error (account deletion must be able to finish).
    fn delete(&self, account_id: &str) -> Result<(), CredentialError>;
}

/// In-memory backend for tests and development sessions. Credentials
/// vanish when the process exits; nothing is written to disk.
#[derive(Debug, Default)]
pub struct InMemoryCredentialStore {
    entries: Mutex<HashMap<String, Credentials>>,
}

impl CredentialStore for InMemoryCredentialStore {
    fn backend_name(&self) -> &str {
        "in-memory"
    }

    fn put(&self, account_id: &str, credentials: &Credentials) -> Result<(), CredentialError> {
        self.entries
            .lock()
            .map_err(|_| CredentialError::Backend("credential store lock poisoned".into()))?
            .insert(account_id.to_string(), credentials.clone());
        Ok(())
    }

    fn get(&self, account_id: &str) -> Result<Option<Credentials>, CredentialError> {
        Ok(self
            .entries
            .lock()
            .map_err(|_| CredentialError::Backend("credential store lock poisoned".into()))?
            .get(account_id)
            .cloned())
    }

    fn delete(&self, account_id: &str) -> Result<(), CredentialError> {
        self.entries
            .lock()
            .map_err(|_| CredentialError::Backend("credential store lock poisoned".into()))?
            .remove(account_id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Credentials {
        let mut credentials = Credentials::new();
        credentials.set("agent_private_key", "0xdeadbeef");
        credentials
    }

    #[test]
    fn put_get_delete_roundtrip() {
        let store = InMemoryCredentialStore::default();
        assert_eq!(store.get("hl1").expect("get"), None);
        store.put("hl1", &sample()).expect("put");
        let loaded = store.get("hl1").expect("get").expect("present");
        assert_eq!(loaded.get("agent_private_key"), Some("0xdeadbeef"));
        store.delete("hl1").expect("delete");
        assert_eq!(store.get("hl1").expect("get"), None);
        // Deleting again is fine.
        store.delete("hl1").expect("delete missing");
    }

    #[test]
    fn accounts_are_isolated_in_the_store() {
        let store = InMemoryCredentialStore::default();
        store.put("hl1", &sample()).expect("put");
        assert_eq!(store.get("hl2").expect("get"), None);
    }

    #[test]
    fn debug_output_never_contains_secret_values() {
        let rendered = format!("{:?}", sample());
        assert!(!rendered.contains("0xdeadbeef"));
        assert!(rendered.contains("agent_private_key"));
    }
}
