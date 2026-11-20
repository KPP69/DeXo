//! Unified exchange Adapter trait.
//!
//! The full trait (market data, account data, order operations, fills) is
//! defined in Phase 2 and first implemented for Hyperliquid in Phase 4.
//! Every method is async; unsupported capabilities must return an explicit
//! `Unsupported` error, never a fabricated success.
