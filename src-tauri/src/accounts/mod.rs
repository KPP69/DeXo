//! Account center: account loading, enable/disable, exchange association
//! and credential management. Strategies never read account files or
//! credentials directly. Implemented in Phase 3: `account_manager` owns
//! the non-sensitive configuration files, `credential_store` owns the
//! secrets behind a separate interface.

pub mod account_manager;
pub mod credential_store;
