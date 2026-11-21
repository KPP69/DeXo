//! Unified error types. Adapters and centers must convert their native
//! errors into these types; raw venue error payloads must not leak into
//! strategy or UI code (they may be kept, sanitised, inside `Remote`).

use std::fmt;

/// Errors from an exchange adapter, after conversion to the unified form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExchangeError {
    /// The venue does not support the requested operation.
    Unsupported(String),
    /// Transport failure where the request may or may not have arrived.
    Network(String),
    /// The request timed out; the outcome is ambiguous and the order
    /// state MUST be queried before any retry or resubmission.
    Timeout,
    /// The venue rejected the request for rate limiting.
    RateLimited,
    /// Authentication or permission failure.
    Authentication(String),
    /// The request itself is invalid (bad symbol, quantity, price...).
    InvalidRequest(String),
    /// The venue answered with an error.
    Remote {
        code: Option<String>,
        message: String,
    },
    /// Any other failure whose outcome cannot be determined locally.
    Unknown(String),
}

impl ExchangeError {
    /// True when the real outcome is not known locally (timeout, lost
    /// connection, unclassifiable failure). Ambiguous outcomes must be
    /// reconciled by querying the exchange, never retried blindly.
    pub fn outcome_is_ambiguous(&self) -> bool {
        matches!(self, Self::Timeout | Self::Network(_) | Self::Unknown(_))
    }
}

impl fmt::Display for ExchangeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported(m) => write!(f, "unsupported operation: {m}"),
            Self::Network(m) => write!(f, "network error: {m}"),
            Self::Timeout => write!(f, "request timed out; outcome unknown"),
            Self::RateLimited => write!(f, "rate limited by exchange"),
            Self::Authentication(m) => write!(f, "authentication failed: {m}"),
            Self::InvalidRequest(m) => write!(f, "invalid request: {m}"),
            Self::Remote { code, message } => match code {
                Some(code) => write!(f, "exchange error {code}: {message}"),
                None => write!(f, "exchange error: {message}"),
            },
            Self::Unknown(m) => write!(f, "unknown exchange error: {m}"),
        }
    }
}

impl std::error::Error for ExchangeError {}

/// Errors from the data center.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DataError {
    /// No data has been loaded for the requested key yet.
    NotFound(String),
    /// Data exists but is older than the caller's freshness limit. Stale
    /// data must never silently drive new orders.
    StaleData(String),
    /// The underlying source (an adapter, in practice) failed.
    Backend(String),
}

impl fmt::Display for DataError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound(m) => write!(f, "data not available: {m}"),
            Self::StaleData(m) => write!(f, "stale data: {m}"),
            Self::Backend(m) => write!(f, "data source error: {m}"),
        }
    }
}

impl std::error::Error for DataError {}

impl From<ExchangeError> for DataError {
    fn from(error: ExchangeError) -> Self {
        Self::Backend(error.to_string())
    }
}

/// Errors from a strategy evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StrategyError {
    Data(DataError),
    InvalidConfiguration(String),
    Internal(String),
}

impl fmt::Display for StrategyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Data(e) => write!(f, "strategy data error: {e}"),
            Self::InvalidConfiguration(m) => write!(f, "invalid strategy configuration: {m}"),
            Self::Internal(m) => write!(f, "strategy internal error: {m}"),
        }
    }
}

impl std::error::Error for StrategyError {}

impl From<DataError> for StrategyError {
    fn from(error: DataError) -> Self {
        Self::Data(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_ambiguous_variants_require_reconciliation() {
        assert!(ExchangeError::Timeout.outcome_is_ambiguous());
        assert!(ExchangeError::Network("reset".into()).outcome_is_ambiguous());
        assert!(ExchangeError::Unknown("?".into()).outcome_is_ambiguous());
        assert!(!ExchangeError::RateLimited.outcome_is_ambiguous());
        assert!(!ExchangeError::InvalidRequest("qty".into()).outcome_is_ambiguous());
        assert!(!ExchangeError::Authentication("bad key".into()).outcome_is_ambiguous());
    }

    #[test]
    fn error_messages_never_claim_success() {
        assert!(ExchangeError::Timeout.to_string().contains("unknown"));
        let data: DataError = ExchangeError::RateLimited.into();
        assert!(matches!(data, DataError::Backend(_)));
    }
}
