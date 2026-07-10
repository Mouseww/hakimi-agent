//! Memory file caching for async prefetch

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::fs;
use tokio::sync::RwLock;
use tracing::{debug, warn};

/// Memory file cache entry
#[derive(Debug, Clone)]
struct CacheEntry {
    content: String,
    loaded_at: Instant,
    file_size: u64,
    mtime: std::time::SystemTime,
}

/// Memory file cache with TTL and size limits
pub struct MemoryCache {
    cache: Arc<RwLock<HashMap<PathBuf, CacheEntry>>>,
    ttl: Duration,
    max_size_bytes: usize,
}

impl MemoryCache {
    /// Create a new memory cache
    ///
    /// # Arguments
    /// * `ttl_minutes` - Time-to-live in minutes before cache entries expire
    /// * `max_size_mb` - Maximum total cache size in megabytes
    pub fn new(ttl_minutes: u64, max_size_mb: usize) -> Self {
        Self {
            cache: Arc::new(RwLock::new(HashMap::new())),
            ttl: Duration::from_secs(ttl_minutes * 60),
            max_size_bytes: max_size_mb * 1024 * 1024,
        }
    }

    /// Asynchronously prefetch a memory file into cache
    ///
    /// If the file doesn't exist, caches an empty string
    pub async fn prefetch(&self, file_path: &Path) -> Result<(), std::io::Error> {
        let metadata = match fs::metadata(file_path).await {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // File doesn't exist, cache empty content
                debug!("Caching non-existent file as empty: {:?}", file_path);
                let entry = CacheEntry {
                    content: String::new(),
                    loaded_at: Instant::now(),
                    file_size: 0,
                    mtime: std::time::SystemTime::now(),
                };
                self.cache.write().await.insert(file_path.to_path_buf(), entry);
                return Ok(());
            }
            Err(e) => return Err(e),
        };

        let content = fs::read_to_string(file_path).await?;
        let entry = CacheEntry {
            content: content.clone(),
            loaded_at: Instant::now(),
            file_size: metadata.len(),
            mtime: metadata.modified().unwrap_or_else(|_| std::time::SystemTime::now()),
        };

        // Check total cache size
        let mut cache = self.cache.write().await;
        let total_size: u64 = cache.values().map(|e| e.file_size).sum();

        if total_size + metadata.len() > self.max_size_bytes as u64 {
            // Evict oldest entry
            if let Some(oldest_key) = cache
                .iter()
                .min_by_key(|(_, e)| e.loaded_at)
                .map(|(k, _)| k.clone())
            {
                warn!(
                    "Cache size limit reached, evicting oldest entry: {:?}",
                    oldest_key
                );
                cache.remove(&oldest_key);
            }
        }

        debug!(
            "Cached memory file: {:?} ({} bytes)",
            file_path,
            metadata.len()
        );
        cache.insert(file_path.to_path_buf(), entry);
        Ok(())
    }

    /// Get cached content if valid (not expired, not modified)
    pub async fn get_cached(&self, file_path: &Path) -> Option<String> {
        let cache = self.cache.read().await;

        if let Some(entry) = cache.get(file_path) {
            // Check TTL
            if entry.loaded_at.elapsed() > self.ttl {
                debug!("Cache expired for {:?}", file_path);
                return None;
            }

            // Check if file was modified
            if let Ok(metadata) = std::fs::metadata(file_path) {
                if let Ok(mtime) = metadata.modified() {
                    if mtime != entry.mtime {
                        debug!("File modified, cache invalid: {:?}", file_path);
                        return None; // File was modified
                    }
                }
            }

            debug!("Cache hit for {:?}", file_path);
            return Some(entry.content.clone());
        }

        debug!("Cache miss for {:?}", file_path);
        None
    }

    /// Invalidate a cache entry
    pub async fn invalidate(&self, file_path: &Path) {
        self.cache.write().await.remove(file_path);
        debug!("Invalidated cache for {:?}", file_path);
    }

    /// Clear all cache entries
    pub async fn clear(&self) {
        let count = self.cache.write().await.len();
        self.cache.write().await.clear();
        debug!("Cleared all cache entries ({})", count);
    }

    /// Get cache statistics
    pub async fn stats(&self) -> CacheStats {
        let cache = self.cache.read().await;
        CacheStats {
            entry_count: cache.len(),
            total_bytes: cache.values().map(|e| e.file_size).sum(),
            oldest_entry_age: cache
                .values()
                .map(|e| e.loaded_at.elapsed())
                .max()
                .unwrap_or(Duration::ZERO),
        }
    }
}

#[derive(Debug)]
pub struct CacheStats {
    pub entry_count: usize,
    pub total_bytes: u64,
    pub oldest_entry_age: Duration,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_prefetch_and_cache_hit() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("test.md");
        fs::write(&file, "test content").await.unwrap();

        let cache = MemoryCache::new(30, 10);
        cache.prefetch(&file).await.unwrap();

        let cached = cache.get_cached(&file).await;
        assert_eq!(cached, Some("test content".to_string()));
    }

    #[tokio::test]
    async fn test_prefetch_nonexistent_file() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("nonexistent.md");

        let cache = MemoryCache::new(30, 10);
        cache.prefetch(&file).await.unwrap();

        let cached = cache.get_cached(&file).await;
        assert_eq!(cached, Some(String::new()));
    }

    #[tokio::test]
    async fn test_cache_invalidation_on_file_change() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("test.md");
        fs::write(&file, "v1").await.unwrap();

        let cache = MemoryCache::new(30, 10);
        cache.prefetch(&file).await.unwrap();

        // Modify file
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        fs::write(&file, "v2").await.unwrap();

        // Cache should be invalid
        let cached = cache.get_cached(&file).await;
        assert!(cached.is_none());
    }

    #[tokio::test]
    async fn test_ttl_expiration() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("test.md");
        fs::write(&file, "content").await.unwrap();

        // 0 minutes TTL (expires immediately in real seconds)
        let cache = MemoryCache::new(0, 10);
        cache.prefetch(&file).await.unwrap();

        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

        // Cache should be expired
        let cached = cache.get_cached(&file).await;
        assert!(cached.is_none());
    }

    #[tokio::test]
    async fn test_manual_invalidation() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("test.md");
        fs::write(&file, "content").await.unwrap();

        let cache = MemoryCache::new(30, 10);
        cache.prefetch(&file).await.unwrap();

        // Manually invalidate
        cache.invalidate(&file).await;

        let cached = cache.get_cached(&file).await;
        assert!(cached.is_none());
    }

    #[tokio::test]
    async fn test_cache_stats() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("test.md");
        fs::write(&file, "x".repeat(1000)).await.unwrap();

        let cache = MemoryCache::new(30, 10);
        cache.prefetch(&file).await.unwrap();

        let stats = cache.stats().await;
        assert_eq!(stats.entry_count, 1);
        assert_eq!(stats.total_bytes, 1000);
        assert!(stats.oldest_entry_age < Duration::from_secs(1));
    }

    #[tokio::test]
    async fn test_max_size_eviction() {
        let tmp = TempDir::new().unwrap();
        let cache = MemoryCache::new(30, 1); // 1MB limit

        // Create 2 files of 600KB each
        let file1 = tmp.path().join("large1.md");
        let file2 = tmp.path().join("large2.md");
        let large_content = "x".repeat(600_000);

        fs::write(&file1, &large_content).await.unwrap();
        fs::write(&file2, &large_content).await.unwrap();

        cache.prefetch(&file1).await.unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        cache.prefetch(&file2).await.unwrap();

        // First file should be evicted
        let cached1 = cache.get_cached(&file1).await;
        let cached2 = cache.get_cached(&file2).await;

        assert!(cached1.is_none()); // Evicted
        assert!(cached2.is_some()); // Retained
    }
}
