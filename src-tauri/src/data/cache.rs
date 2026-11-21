//! A tiny timestamped cache: every entry records when it was fetched,
//! and freshness is always judged against an explicit TTL by the caller.
//! The cache itself never decides that old data is "good enough" — that
//! decision lives in the data center, which refuses to serve stale data
//! for trading reads.

use std::collections::HashMap;
use std::hash::Hash;
use std::sync::Mutex;

use crate::models::TimestampMs;

#[derive(Debug, Clone)]
pub struct CacheEntry<T> {
    pub value: T,
    pub fetched_at: TimestampMs,
}

impl<T> CacheEntry<T> {
    /// Age of the entry at `now`; negative ages (clock skew) count as 0.
    pub fn age_at(&self, now: TimestampMs) -> TimestampMs {
        (now - self.fetched_at).max(0)
    }

    pub fn is_fresh_at(&self, now: TimestampMs, ttl_ms: TimestampMs) -> bool {
        self.age_at(now) <= ttl_ms
    }
}

#[derive(Debug, Default)]
pub struct TtlCache<K, V> {
    entries: Mutex<HashMap<K, CacheEntry<V>>>,
}

impl<K: Eq + Hash, V: Clone> TtlCache<K, V> {
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
        }
    }

    /// Returns the value only when present AND fresh at `now`.
    pub fn get_fresh(&self, key: &K, now: TimestampMs, ttl_ms: TimestampMs) -> Option<V> {
        let entries = self.entries.lock().ok()?;
        entries
            .get(key)
            .filter(|entry| entry.is_fresh_at(now, ttl_ms))
            .map(|entry| entry.value.clone())
    }

    /// Returns the entry regardless of freshness (for error reporting:
    /// "we have data, but it is too old").
    pub fn get_any(&self, key: &K) -> Option<CacheEntry<V>> {
        self.entries.lock().ok()?.get(key).cloned()
    }

    pub fn put(&self, key: K, value: V, fetched_at: TimestampMs) {
        if let Ok(mut entries) = self.entries.lock() {
            entries.insert(key, CacheEntry { value, fetched_at });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn freshness_is_judged_against_the_ttl() {
        let cache = TtlCache::<String, i64>::new();
        cache.put("k".to_string(), 7, 1_000);
        assert_eq!(cache.get_fresh(&"k".to_string(), 1_500, 1_000), Some(7));
        assert_eq!(cache.get_fresh(&"k".to_string(), 2_500, 1_000), None);
        // Stale entries are still visible for diagnostics.
        assert_eq!(cache.get_any(&"k".to_string()).map(|e| e.value), Some(7));
    }
}
