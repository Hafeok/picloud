//! In-memory DNS cache with 30-second TTL and event-driven invalidation.

use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::RwLock;
use tracing::debug;

/// Default TTL for cached DNS records (30 seconds per ADR-052).
pub const DEFAULT_TTL_SECS: u64 = 30;

/// Thread-safe shared DNS cache.
pub type SharedDnsCache = Arc<RwLock<DnsCache>>;

/// A cached DNS record.
#[derive(Debug, Clone)]
pub enum DnsRecord {
    A { address: Ipv4Addr },
    Srv { target: String, port: u16, priority: u16, weight: u16 },
    Txt { text: String },
    Ptr { name: String },
}

/// A single cache entry with insertion time for TTL enforcement.
#[derive(Debug, Clone)]
struct CacheEntry {
    records: Vec<DnsRecord>,
    inserted_at: Instant,
    ttl: Duration,
}

impl CacheEntry {
    fn is_expired(&self) -> bool {
        self.inserted_at.elapsed() > self.ttl
    }
}

/// In-memory DNS cache keyed by (record_type, hostname).
pub struct DnsCache {
    entries: HashMap<(String, String), CacheEntry>,
    ttl: Duration,
}

impl DnsCache {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            ttl: Duration::from_secs(DEFAULT_TTL_SECS),
        }
    }

    /// Look up cached records. Returns None on miss or expired entry.
    pub fn get(&self, record_type: &str, hostname: &str) -> Option<Vec<DnsRecord>> {
        let key = (record_type.to_uppercase(), hostname.to_lowercase());
        match self.entries.get(&key) {
            Some(entry) if !entry.is_expired() => {
                debug!(record_type = %record_type, hostname = %hostname, "DNS cache hit");
                Some(entry.records.clone())
            }
            Some(_) => {
                debug!(record_type = %record_type, hostname = %hostname, "DNS cache expired");
                None
            }
            None => None,
        }
    }

    /// Insert records into the cache.
    pub fn insert(&mut self, record_type: &str, hostname: &str, records: Vec<DnsRecord>) {
        let key = (record_type.to_uppercase(), hostname.to_lowercase());
        debug!(
            record_type = %record_type,
            hostname = %hostname,
            count = records.len(),
            "DNS cache insert"
        );
        self.entries.insert(key, CacheEntry {
            records,
            inserted_at: Instant::now(),
            ttl: self.ttl,
        });
    }

    /// Invalidate all cached entries for a hostname (all record types).
    pub fn invalidate(&mut self, hostname: &str) {
        let lower = hostname.to_lowercase();
        self.entries.retain(|(_rtype, h), _| h != &lower);
        debug!(hostname = %hostname, "DNS cache invalidated");
    }

    /// Invalidate a specific record type for a hostname.
    pub fn invalidate_record(&mut self, record_type: &str, hostname: &str) {
        let key = (record_type.to_uppercase(), hostname.to_lowercase());
        self.entries.remove(&key);
    }

    /// Remove all entries.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Evict expired entries. Called periodically by background task.
    pub fn evict_expired(&mut self) -> usize {
        let before = self.entries.len();
        self.entries.retain(|_, entry| !entry.is_expired());
        let evicted = before - self.entries.len();
        if evicted > 0 {
            debug!(evicted = evicted, "DNS cache evicted expired entries");
        }
        evicted
    }
}

impl Default for DnsCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_insert_and_get() {
        let mut cache = DnsCache::new();
        cache.insert("A", "photos.picloud.local", vec![
            DnsRecord::A { address: Ipv4Addr::new(192, 168, 1, 10) },
        ]);
        let result = cache.get("A", "photos.picloud.local");
        assert!(result.is_some());
        assert_eq!(result.unwrap().len(), 1);
    }

    #[test]
    fn cache_miss_returns_none() {
        let cache = DnsCache::new();
        assert!(cache.get("A", "nonexistent.picloud.local").is_none());
    }

    #[test]
    fn cache_invalidate_removes_all_types() {
        let mut cache = DnsCache::new();
        cache.insert("A", "photos.picloud.local", vec![
            DnsRecord::A { address: Ipv4Addr::new(192, 168, 1, 10) },
        ]);
        cache.insert("TXT", "photos.picloud.local", vec![
            DnsRecord::Txt { text: "v=picloud".to_string() },
        ]);
        cache.invalidate("photos.picloud.local");
        assert!(cache.get("A", "photos.picloud.local").is_none());
        assert!(cache.get("TXT", "photos.picloud.local").is_none());
    }

    #[test]
    fn cache_case_insensitive() {
        let mut cache = DnsCache::new();
        cache.insert("A", "Photos.PiCloud.Local", vec![
            DnsRecord::A { address: Ipv4Addr::new(10, 0, 0, 1) },
        ]);
        assert!(cache.get("a", "photos.picloud.local").is_some());
    }
}
