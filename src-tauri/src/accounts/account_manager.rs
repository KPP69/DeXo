//! Account manager (Phase 3): loads, validates and persists account
//! configurations from `data/accounts/<id>.json`.
//!
//! Only non-sensitive configuration lives in those files (the `Account`
//! model rejects unknown fields, so credentials cannot be smuggled in).
//! Loading is strict: a corrupt file, a duplicate id or an invalid id
//! fails the whole load with the offending path named, because silently
//! skipping an account could make a strategy trade the wrong account or
//! none at all. Credentials are managed separately through
//! `credential_store::CredentialStore`.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use crate::models::Account;

#[derive(Debug)]
pub enum AccountError {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    Parse {
        path: PathBuf,
        message: String,
    },
    DuplicateId(String),
    Invalid(String),
    NotFound(String),
}

impl fmt::Display for AccountError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(f, "account file {} failed: {source}", path.display())
            }
            Self::Parse { path, message } => {
                write!(f, "account file {} is invalid: {message}", path.display())
            }
            Self::DuplicateId(id) => write!(f, "duplicate account id: {id}"),
            Self::Invalid(m) => write!(f, "invalid account: {m}"),
            Self::NotFound(id) => write!(f, "account not found: {id}"),
        }
    }
}

impl std::error::Error for AccountError {}

/// Account ids double as file names, so they are restricted to a safe
/// alphabet: letters, digits, `-` and `_`, 1–64 characters.
pub fn validate_account(account: &Account) -> Result<(), AccountError> {
    let id = &account.id;
    let id_ok = !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if !id_ok {
        return Err(AccountError::Invalid(format!(
            "account id '{id}' must be 1-64 chars of [A-Za-z0-9_-]"
        )));
    }
    if account.exchange.is_empty() {
        return Err(AccountError::Invalid("exchange must not be empty".into()));
    }
    if account.label.is_empty() {
        return Err(AccountError::Invalid("label must not be empty".into()));
    }
    Ok(())
}

#[derive(Debug)]
pub struct AccountManager {
    dir: PathBuf,
    accounts: BTreeMap<String, Account>,
}

impl AccountManager {
    /// Loads every `*.json` account file in `dir`. A missing directory is
    /// an error: startup creates the layout first, so a missing directory
    /// means the portable layout is broken and must not be papered over.
    pub fn load(dir: &Path) -> Result<Self, AccountError> {
        let mut accounts = BTreeMap::new();
        let entries = fs::read_dir(dir).map_err(|source| AccountError::Io {
            path: dir.to_path_buf(),
            source,
        })?;
        let mut files: Vec<PathBuf> = entries
            .filter_map(|entry| entry.ok().map(|e| e.path()))
            .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
            .collect();
        files.sort();
        for path in files {
            let raw = fs::read_to_string(&path).map_err(|source| AccountError::Io {
                path: path.clone(),
                source,
            })?;
            let account: Account = serde_json::from_str(&raw).map_err(|e| AccountError::Parse {
                path: path.clone(),
                message: e.to_string(),
            })?;
            validate_account(&account).map_err(|e| AccountError::Parse {
                path: path.clone(),
                message: e.to_string(),
            })?;
            let id = account.id.clone();
            if accounts.insert(id.clone(), account).is_some() {
                return Err(AccountError::DuplicateId(id));
            }
        }
        Ok(Self {
            dir: dir.to_path_buf(),
            accounts,
        })
    }

    pub fn list(&self) -> Vec<Account> {
        self.accounts.values().cloned().collect()
    }

    pub fn get(&self, id: &str) -> Option<&Account> {
        self.accounts.get(id)
    }

    pub fn enabled_accounts(&self) -> Vec<Account> {
        self.accounts
            .values()
            .filter(|a| a.enabled)
            .cloned()
            .collect()
    }

    /// Accounts grouped by exchange id, for the account center UI and for
    /// the execution layer's per-exchange startup.
    pub fn grouped_by_exchange(&self) -> BTreeMap<String, Vec<Account>> {
        let mut groups: BTreeMap<String, Vec<Account>> = BTreeMap::new();
        for account in self.accounts.values() {
            groups
                .entry(account.exchange.clone())
                .or_default()
                .push(account.clone());
        }
        groups
    }

    /// Adds a new account and persists it before it becomes visible.
    pub fn add(&mut self, account: Account) -> Result<(), AccountError> {
        validate_account(&account)?;
        if self.accounts.contains_key(&account.id) {
            return Err(AccountError::DuplicateId(account.id.clone()));
        }
        self.persist(&account)?;
        self.accounts.insert(account.id.clone(), account);
        Ok(())
    }

    pub fn set_enabled(&mut self, id: &str, enabled: bool) -> Result<(), AccountError> {
        let mut account = self
            .accounts
            .get(id)
            .cloned()
            .ok_or_else(|| AccountError::NotFound(id.to_string()))?;
        account.enabled = enabled;
        self.persist(&account)?;
        self.accounts.insert(id.to_string(), account);
        Ok(())
    }

    /// Removes the account configuration and returns it. Deleting the
    /// account's credentials is a separate, explicit step through the
    /// credential store, so the caller decides the order.
    pub fn remove(&mut self, id: &str) -> Result<Account, AccountError> {
        let account = self
            .accounts
            .remove(id)
            .ok_or_else(|| AccountError::NotFound(id.to_string()))?;
        let path = self.file_for(id);
        fs::remove_file(&path).map_err(|source| AccountError::Io { path, source })?;
        Ok(account)
    }

    fn file_for(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.json"))
    }

    /// Writes via a temporary file plus rename, so a crash mid-write
    /// cannot leave a truncated account file behind.
    fn persist(&self, account: &Account) -> Result<(), AccountError> {
        let path = self.file_for(&account.id);
        let tmp = self.dir.join(format!(".{}.json.tmp", account.id));
        let raw = serde_json::to_string_pretty(account).map_err(|e| AccountError::Parse {
            path: path.clone(),
            message: e.to_string(),
        })?;
        fs::write(&tmp, raw).map_err(|source| AccountError::Io {
            path: tmp.clone(),
            source,
        })?;
        fs::rename(&tmp, &path).map_err(|source| AccountError::Io { path, source })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(id: &str, exchange: &str) -> Account {
        Account {
            id: id.to_string(),
            exchange: exchange.to_string(),
            label: format!("label-{id}"),
            enabled: true,
            testnet: true,
        }
    }

    fn manager_in(tmp: &tempfile::TempDir) -> AccountManager {
        AccountManager::load(tmp.path()).expect("load empty dir")
    }

    #[test]
    fn empty_directory_loads_no_accounts() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let manager = manager_in(&tmp);
        assert!(manager.list().is_empty());
    }

    #[test]
    fn add_persists_and_reload_sees_the_account() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let mut manager = manager_in(&tmp);
        manager.add(account("hl1", "hyperliquid")).expect("add");
        assert!(tmp.path().join("hl1.json").exists());
        let reloaded = manager_in(&tmp);
        assert_eq!(reloaded.list(), vec![account("hl1", "hyperliquid")]);
    }

    #[test]
    fn duplicate_ids_are_rejected_on_add_and_on_load() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let mut manager = manager_in(&tmp);
        manager.add(account("hl1", "hyperliquid")).expect("add");
        assert!(matches!(
            manager.add(account("hl1", "hyperliquid")),
            Err(AccountError::DuplicateId(_))
        ));
        // A second file carrying the same id must fail the load, not
        // silently override the first.
        fs::write(
            tmp.path().join("copy.json"),
            serde_json::to_string(&account("hl1", "hyperliquid")).expect("json"),
        )
        .expect("write");
        assert!(matches!(
            AccountManager::load(tmp.path()),
            Err(AccountError::DuplicateId(_))
        ));
    }

    #[test]
    fn account_file_with_credential_fields_is_rejected() {
        let tmp = tempfile::tempdir().expect("tempdir");
        fs::write(
            tmp.path().join("bad.json"),
            r#"{"id":"hl1","exchange":"hyperliquid","label":"x","enabled":true,"testnet":true,"credentials":{"agent_private_key":"0xabc"}}"#,
        )
        .expect("write");
        assert!(matches!(
            AccountManager::load(tmp.path()),
            Err(AccountError::Parse { .. })
        ));
    }

    #[test]
    fn corrupt_account_file_fails_the_load_with_its_path() {
        let tmp = tempfile::tempdir().expect("tempdir");
        fs::write(tmp.path().join("broken.json"), "{not json").expect("write");
        match AccountManager::load(tmp.path()) {
            Err(AccountError::Parse { path, .. }) => {
                assert!(path.ends_with("broken.json"));
            }
            other => panic!("expected parse error, got {other:?}"),
        }
    }

    #[test]
    fn invalid_ids_are_rejected() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let mut manager = manager_in(&tmp);
        let mut bad = account("../escape", "hyperliquid");
        assert!(matches!(
            manager.add(bad.clone()),
            Err(AccountError::Invalid(_))
        ));
        bad.id = String::new();
        assert!(matches!(manager.add(bad), Err(AccountError::Invalid(_))));
    }

    #[test]
    fn grouping_and_enabled_filter_work() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let mut manager = manager_in(&tmp);
        manager.add(account("hl1", "hyperliquid")).expect("add");
        manager.add(account("hl2", "hyperliquid")).expect("add");
        manager.add(account("lt1", "lighter")).expect("add");
        manager.set_enabled("hl2", false).expect("disable");
        let groups = manager.grouped_by_exchange();
        assert_eq!(groups["hyperliquid"].len(), 2);
        assert_eq!(groups["lighter"].len(), 1);
        let enabled = manager.enabled_accounts();
        assert_eq!(enabled.len(), 2);
        // The disabled flag must survive a reload.
        let reloaded = manager_in(&tmp);
        assert_eq!(reloaded.enabled_accounts().len(), 2);
    }

    #[test]
    fn remove_deletes_the_file_and_returns_the_account() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let mut manager = manager_in(&tmp);
        manager.add(account("hl1", "hyperliquid")).expect("add");
        let removed = manager.remove("hl1").expect("remove");
        assert_eq!(removed.id, "hl1");
        assert!(!tmp.path().join("hl1.json").exists());
        assert!(matches!(
            manager.remove("hl1"),
            Err(AccountError::NotFound(_))
        ));
    }
}
