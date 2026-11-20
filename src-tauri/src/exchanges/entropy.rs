//! Entropy is a Hyperliquid HIP-3 sub-DEX (namespace `io`, e.g. markets
//! `io:ANTH`, `io:SNDK`), not a standalone exchange. It uses the
//! Hyperliquid API, matching and settlement, and is accessed through the
//! Hyperliquid adapter's dex-namespace support in Phase 12. No separate
//! Entropy adapter is planned; this module documents that decision.
