//! Account-side models: exchanges, accounts, balances and positions.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use super::TimestampMs;

/// How an exchange entry is integrated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExchangeKind {
    /// A standalone exchange with its own adapter and API.
    Standalone,
    /// A sub-DEX / market namespace served through a parent exchange's
    /// adapter (for example a Hyperliquid HIP-3 builder DEX). The entry
    /// still appears as its own exchange in configuration and the UI;
    /// only the transport is shared.
    Namespace,
}

/// A tradable venue as the product sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Exchange {
    /// Stable identifier used in accounts and orders, e.g. "hyperliquid".
    pub id: String,
    pub name: String,
    pub kind: ExchangeKind,
    /// For `ExchangeKind::Namespace`, the exchange whose adapter serves
    /// this venue, e.g. "hyperliquid" for Entropy.
    pub parent: Option<String>,
}

impl Exchange {
    pub fn standalone(id: &str, name: &str) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            kind: ExchangeKind::Standalone,
            parent: None,
        }
    }

    pub fn namespace(id: &str, name: &str, parent: &str) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            kind: ExchangeKind::Namespace,
            parent: Some(parent.to_string()),
        }
    }
}

/// A trading account's non-sensitive configuration.
///
/// Credentials are deliberately NOT part of this structure: they live in
/// the credential store (Phase 3) and are never serialised with the
/// account, written to logs, or sent to the frontend.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    /// `Exchange::id` this account trades on.
    pub exchange: String,
    pub label: String,
    pub enabled: bool,
    pub testnet: bool,
}

/// Account balance on one exchange, in one currency.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Balance {
    pub account_id: String,
    pub exchange: String,
    pub currency: String,
    pub total: Decimal,
    pub available: Decimal,
    pub updated_at: TimestampMs,
}

/// An open position. `quantity` is signed: positive = long, negative =
/// short, zero = flat (a flat position should normally not be reported).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Position {
    pub account_id: String,
    pub exchange: String,
    pub symbol: String,
    pub quantity: Decimal,
    pub entry_price: Option<Decimal>,
    pub leverage: Option<Decimal>,
    pub unrealized_pnl: Option<Decimal>,
    pub updated_at: TimestampMs,
}

impl Position {
    pub fn is_long(&self) -> bool {
        self.quantity > Decimal::ZERO
    }

    pub fn is_short(&self) -> bool {
        self.quantity < Decimal::ZERO
    }

    /// Absolute position size, used for exposure and rollback quantities.
    pub fn size(&self) -> Decimal {
        self.quantity.abs()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn entropy_is_a_namespace_of_hyperliquid() {
        let entropy = Exchange::namespace("entropy", "Entropy", "hyperliquid");
        assert_eq!(entropy.kind, ExchangeKind::Namespace);
        assert_eq!(entropy.parent.as_deref(), Some("hyperliquid"));
        let hl = Exchange::standalone("hyperliquid", "Hyperliquid");
        assert_eq!(hl.kind, ExchangeKind::Standalone);
        assert_eq!(hl.parent, None);
    }

    #[test]
    fn position_direction_comes_from_signed_quantity() {
        let mut pos = Position {
            account_id: "hl1".into(),
            exchange: "hyperliquid".into(),
            symbol: "BTC".into(),
            quantity: dec!(-0.5),
            entry_price: Some(dec!(90000)),
            leverage: None,
            unrealized_pnl: None,
            updated_at: 1,
        };
        assert!(pos.is_short());
        assert_eq!(pos.size(), dec!(0.5));
        pos.quantity = dec!(0.25);
        assert!(pos.is_long());
    }

    #[test]
    fn account_serialises_without_any_credential_field() {
        let account = Account {
            id: "hl1".into(),
            exchange: "hyperliquid".into(),
            label: "main".into(),
            enabled: true,
            testnet: true,
        };
        let json = serde_json::to_string(&account).expect("serialise");
        assert!(!json.contains("key"));
        assert!(!json.contains("secret"));
        assert!(!json.contains("credential"));
        let back: Account = serde_json::from_str(&json).expect("deserialise");
        assert_eq!(account, back);
    }
}
