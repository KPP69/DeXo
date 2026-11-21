//! Bulk adapter candidate (Phase 12). Bulk is a perpetual DEX on Solana
//! whose mainnet API is live (docs.bulk.trade). Authentication is by
//! Solana wallet signature, not an API key/secret model, and the service
//! is restricted in many jurisdictions. The authentication and signing
//! design differs from Hyperliquid and must be designed separately;
//! verify the official docs before implementing.
