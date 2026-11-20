//! Exchange center: one Adapter per exchange behind a unified trait.
//!
//! Planned adapters: Hyperliquid (Phase 4), then Entropy, Arcus, Bulk,
//! Lighter and AsterDex (Phase 12). No adapter may leak exchange-specific
//! data structures into the strategy layer.

pub mod traits;

pub mod arcus;
pub mod aster;
pub mod bulk;
pub mod entropy;
pub mod hyperliquid;
pub mod lighter;
