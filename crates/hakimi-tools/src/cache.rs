use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Tool call result cache entry
#[derive(Debug, Clone)]
pub struct CacheEntry {
    pub result: serde_json::Value,
    pub created_at: Instant,
    pub hit_count: usize,
}

/// Cache configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    pub ttl_seconds: u64,
    pub max_entries: usize,
    pub enable_cache: bool,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            ttl_seconds: 300, // 5 minutes
            max_entries: 1000,
            enable_cache: true,
        }
    }
}

impl CacheConfig {
    pub fn ttl(&self) -> Duration {
        Duration::from_secs(self.ttl_seconds)
    }
}

/// Tool call result cache with TTL and LRU eviction
pub struct ToolCallCache {
    entries: Arc<Mutex<HashMap<String, CacheEntry>>>,
    config: CacheConfig,
}

impl ToolCallCache {
    pub fn new(config: CacheConfig) -> Self {
        Self {
            entries: Arc::new(Mutex::new(HashMap::new())),
            config,
        }
    }

    /// Get cached result if available and not expired
    pub fn get(&self, key: &str) -> Option<serde_json::Value> {
        if !self.config.enable_cache {
            return None;
        }

        let mut entries = self.entries.lock().unwrap();

        if let Some(entry) = entries.get_mut(key) {
            // Check if entry is expired
            if entry.created_at.elapsed() < self.config.ttl() {
                entry.hit_count += 1;
                return Some(entry.result.clone());
            } else {
                // Expired, remove it
                entries.remove(key);
            }
        }

        None
    }

    /// Set cache entry
    pub fn set(&self, key: String, result: serde_json::Value) {
        if !self.config.enable_cache {
            return;
        }

        let mut entries = self.entries.lock().unwrap();

        // LRU eviction if cache is full
        if entries.len() >= self.config.max_entries && !entries.contains_key(&key) {
            self.evict_lru(&mut entries);
        }

        entries.insert(
            key,
            CacheEntry {
                result,
                created_at: Instant::now(),
                hit_count: 0,
            },
        );
    }

    /// Invalidate a specific cache entry
    pub fn invalidate(&self, key: &str) -> bool {
        let mut entries = self.entries.lock().unwrap();
        entries.remove(key).is_some()
    }

    /// Clear all cache entries
    pub fn clear(&self) {
        let mut entries = self.entries.lock().unwrap();
        entries.clear();
    }

    /// Get cache statistics
    pub fn stats(&self) -> CacheStats {
        let entries = self.entries.lock().unwrap();

        let total_hits: usize = entries.values().map(|e| e.hit_count).sum();
        let total_entries = entries.len();
        let total_accesses = total_entries + total_hits;

        CacheStats {
            total_entries,
            total_hits,
            hit_rate: if total_accesses > 0 {
                total_hits as f64 / total_accesses as f64
            } else {
                0.0
            },
        }
    }

    /// Evict least recently used (oldest) entry
    fn evict_lru(&self, entries: &mut HashMap<String, CacheEntry>) {
        if let Some(oldest_key) = entries
            .iter()
            .min_by_key(|(_, entry)| entry.created_at)
            .map(|(k, _)| k.clone())
        {
            entries.remove(&oldest_key);
        }
    }

    /// Remove expired entries
    pub fn cleanup_expired(&self) -> usize {
        let mut entries = self.entries.lock().unwrap();
        let ttl = self.config.ttl();
        let before_count = entries.len();

        entries.retain(|_, entry| entry.created_at.elapsed() < ttl);

        before_count - entries.len()
    }

    /// Get cache size
    pub fn size(&self) -> usize {
        let entries = self.entries.lock().unwrap();
        entries.len()
    }
}

impl Default for ToolCallCache {
    fn default() -> Self {
        Self::new(CacheConfig::default())
    }
}

/// Cache statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheStats {
    pub total_entries: usize,
    pub total_hits: usize,
    pub hit_rate: f64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::thread;

    #[test]
    fn test_cache_hit() {
        let cache = ToolCallCache::new(CacheConfig::default());
        let key = "test_key".to_string();
        let value = json!({"result": "success"});

        cache.set(key.clone(), value.clone());
        let result = cache.get(&key);

        assert_eq!(result, Some(value));
    }

    #[test]
    fn test_cache_miss() {
        let cache = ToolCallCache::new(CacheConfig::default());
        let result = cache.get("nonexistent");

        assert_eq!(result, None);
    }

    #[test]
    fn test_cache_expiration() {
        let config = CacheConfig {
            ttl_seconds: 1,
            max_entries: 100,
            enable_cache: true,
        };
        let cache = ToolCallCache::new(config);
        let key = "expire_key".to_string();
        let value = json!({"result": "will_expire"});

        cache.set(key.clone(), value.clone());

        // Should hit before expiration
        assert_eq!(cache.get(&key), Some(value.clone()));

        // Wait for expiration
        thread::sleep(Duration::from_secs(2));

        // Should miss after expiration
        assert_eq!(cache.get(&key), None);
    }

    #[test]
    fn test_lru_eviction() {
        let config = CacheConfig {
            ttl_seconds: 300,
            max_entries: 2,
            enable_cache: true,
        };
        let cache = ToolCallCache::new(config);

        cache.set("key1".to_string(), json!("value1"));
        thread::sleep(Duration::from_millis(10));
        cache.set("key2".to_string(), json!("value2"));
        thread::sleep(Duration::from_millis(10));

        // Adding third entry should evict key1 (oldest)
        cache.set("key3".to_string(), json!("value3"));

        assert_eq!(cache.get("key1"), None);
        assert_eq!(cache.get("key2"), Some(json!("value2")));
        assert_eq!(cache.get("key3"), Some(json!("value3")));
    }

    #[test]
    fn test_cache_invalidate() {
        let cache = ToolCallCache::new(CacheConfig::default());
        let key = "inv_key".to_string();
        let value = json!({"result": "will_be_invalidated"});

        cache.set(key.clone(), value.clone());
        assert_eq!(cache.get(&key), Some(value));

        // Invalidate
        let removed = cache.invalidate(&key);
        assert!(removed);

        // Should miss after invalidation
        assert_eq!(cache.get(&key), None);
    }

    #[test]
    fn test_cache_clear() {
        let cache = ToolCallCache::new(CacheConfig::default());

        cache.set("key1".to_string(), json!("value1"));
        cache.set("key2".to_string(), json!("value2"));

        assert_eq!(cache.size(), 2);

        cache.clear();

        assert_eq!(cache.size(), 0);
        assert_eq!(cache.get("key1"), None);
        assert_eq!(cache.get("key2"), None);
    }

    #[test]
    fn test_cache_stats() {
        let cache = ToolCallCache::new(CacheConfig::default());

        cache.set("key1".to_string(), json!("value1"));
        cache.set("key2".to_string(), json!("value2"));

        // First access - miss converted to hit
        cache.get("key1");
        cache.get("key1"); // Second hit
        cache.get("key2");

        let stats = cache.stats();
        assert_eq!(stats.total_entries, 2);
        assert_eq!(stats.total_hits, 3);
        // hit_rate = hits / (entries + hits) = 3 / 5 = 0.6
        assert!((stats.hit_rate - 0.6).abs() < 0.01);
    }

    #[test]
    fn test_disabled_cache() {
        let config = CacheConfig {
            ttl_seconds: 300,
            max_entries: 100,
            enable_cache: false,
        };
        let cache = ToolCallCache::new(config);

        cache.set("key1".to_string(), json!("value1"));

        // Should always miss when cache is disabled
        assert_eq!(cache.get("key1"), None);
        assert_eq!(cache.size(), 0);
    }

    #[test]
    fn test_cleanup_expired() {
        let config = CacheConfig {
            ttl_seconds: 1,
            max_entries: 100,
            enable_cache: true,
        };
        let cache = ToolCallCache::new(config);

        cache.set("key1".to_string(), json!("value1"));
        cache.set("key2".to_string(), json!("value2"));

        assert_eq!(cache.size(), 2);

        // Wait for expiration
        thread::sleep(Duration::from_secs(2));

        // Cleanup expired
        let removed = cache.cleanup_expired();
        assert_eq!(removed, 2);
        assert_eq!(cache.size(), 0);
    }

    #[test]
    fn test_update_existing_entry() {
        let cache = ToolCallCache::new(CacheConfig::default());
        let key = "update_key".to_string();

        cache.set(key.clone(), json!("value1"));
        assert_eq!(cache.get(&key), Some(json!("value1")));

        // Update with new value
        cache.set(key.clone(), json!("value2"));
        assert_eq!(cache.get(&key), Some(json!("value2")));

        // Should still have only one entry
        assert_eq!(cache.size(), 1);
    }
}
