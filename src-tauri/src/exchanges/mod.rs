//! Exchange center: one Adapter per exchange behind a unified trait.
//!
//! Integration status (must be re-verified against official docs before
//! any implementation; nothing here is marked verified):
//! - Hyperliquid: first adapter, Phase 4. Its HIP-3 sub-DEX namespaces
//!   (for example Entropy, namespace `io`) are served through this
//!   adapter's namespace support, not through separate adapters.
//! - Arcus: candidate, Phase 12. Perpetual API availability is rolling
//!   beta; verify API access and Ed25519 key auth before designing.
//! - Bulk: candidate, Phase 12, standalone adapter with its own design:
//!   Solana wallet authentication, not an API-key model.
//! - Lighter, AsterDex: candidates, Phase 12.
//!
//! No adapter may leak exchange-specific data structures into the
//! strategy layer.

pub mod traits;

pub mod arcus;
pub mod aster;
pub mod bulk;
pub mod entropy;
pub mod hyperliquid;
pub mod lighter;
