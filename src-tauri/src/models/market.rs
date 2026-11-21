//! Market data models: funding rates and order books. Every value carries
//! the time it was observed so the data center can enforce staleness
//! protection (Phase 5): strategies must never trade on expired data.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use super::TimestampMs;

/// Funding-rate snapshot for one symbol on one exchange.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FundingInfo {
    pub exchange: String,
    pub symbol: String,
    /// Funding rate for the current period as an exact decimal fraction
    /// (for example 0.0001 = 0.01%).
    pub rate: Decimal,
    /// When the next funding payment occurs, if the venue reports it.
    pub next_funding_at: Option<TimestampMs>,
    pub observed_at: TimestampMs,
}

/// One price level of an order book.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BookLevel {
    pub price: Decimal,
    pub quantity: Decimal,
}

/// Order book snapshot. `bids` are sorted by price descending, `asks`
/// ascending; adapters must normalise venue ordering into this form.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OrderBook {
    pub exchange: String,
    pub symbol: String,
    pub bids: Vec<BookLevel>,
    pub asks: Vec<BookLevel>,
    pub observed_at: TimestampMs,
}

impl OrderBook {
    pub fn best_bid(&self) -> Option<&BookLevel> {
        self.bids.first()
    }

    pub fn best_ask(&self) -> Option<&BookLevel> {
        self.asks.first()
    }

    /// Mid price, or `None` when either side of the book is empty.
    pub fn mid_price(&self) -> Option<Decimal> {
        match (self.best_bid(), self.best_ask()) {
            (Some(bid), Some(ask)) => Some((bid.price + ask.price) / Decimal::from(2)),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn book() -> OrderBook {
        OrderBook {
            exchange: "hyperliquid".into(),
            symbol: "BTC".into(),
            bids: vec![BookLevel {
                price: dec!(90000),
                quantity: dec!(1.5),
            }],
            asks: vec![BookLevel {
                price: dec!(90010),
                quantity: dec!(2),
            }],
            observed_at: 100,
        }
    }

    #[test]
    fn mid_price_uses_exact_decimal_arithmetic() {
        assert_eq!(book().mid_price(), Some(dec!(90005)));
        // The classic float trap: 0.1 + 0.2 must be exactly 0.3.
        let mut b = book();
        b.bids[0].price = dec!(0.1);
        b.asks[0].price = dec!(0.2);
        assert_eq!(b.mid_price(), Some(dec!(0.15)));
    }

    #[test]
    fn empty_side_has_no_mid_price() {
        let mut b = book();
        b.asks.clear();
        assert_eq!(b.mid_price(), None);
        assert_eq!(b.best_bid().map(|l| l.price), Some(dec!(90000)));
    }

    #[test]
    fn funding_info_roundtrips_through_json() {
        let funding = FundingInfo {
            exchange: "hyperliquid".into(),
            symbol: "ETH".into(),
            rate: dec!(0.0001),
            next_funding_at: Some(3_600_000),
            observed_at: 42,
        };
        let json = serde_json::to_string(&funding).expect("serialise");
        let back: FundingInfo = serde_json::from_str(&json).expect("deserialise");
        assert_eq!(funding, back);
    }
}
