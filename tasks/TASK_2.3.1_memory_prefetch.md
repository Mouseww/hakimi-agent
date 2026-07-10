# TASK 2.3.1: 异步记忆预取机制

**状态**: 🟡 进行中 (0%)  
**优先级**: P2  
**预计工作量**: 4-5 小时  
**依赖**: 无  
**解锁**: TASK 2.3.2 (缓存失效策略)

---

## 📋 任务目标

实现异步记忆预取机制，在会话创建后立即后台加载用户记忆和会话记忆，避免阻塞主循环，提升首次响应速度。

**当前问题**:
- 记忆文件（user.md、memory.md、working_memory.md）在首次请求时同步加载
- 大文件（> 20KB）加载耗时可能超过 50-100ms
- 阻塞主线程，影响首字节时间（TTFB）

**目标**:
- 会话创建后异步预取记忆到内存缓存
- 主循环延迟 < 10ms（预取在后台进行）
- 缓存命中率 > 90%（避免重复 I/O）
- 支持多会话并发预取

---

## 🎯 验收标准

- [ ] 实现 `MemoryCache` 结构体，支持异步读取和缓存
- [ ] 新增 `prefetch_memories()` 异步方法，后台加载记忆文件
- [ ] 集成到会话创建流程（`create_session` 后立即调用）
- [ ] 首次响应延迟 < 10ms（预取不阻塞）
- [ ] 缓存命中时记忆加载耗时 < 1ms
- [ ] 单元测试覆盖：缓存命中、缓存未命中、文件不存在
- [ ] 性能基准测试：对比预取前后的 TTFB
- [ ] 编译无错误：`cargo check --all-features`
- [ ] 所有测试通过：`cargo test --package hakimi-context`

---

## 📁 涉及文件

### 新增
- `crates/hakimi-context/src/memory_cache.rs` (约 200 行)
  - `MemoryCache` 结构体
  - `prefetch()` 异步方法
  - `get_cached()` 方法
  - `invalidate()` 方法

### 修改
- `crates/hakimi-context/src/lib.rs`
  - 导出 `MemoryCache`
  
- `crates/hakimi-context/src/memory.rs`
  - 集成 `MemoryCache`
  - 修改 `load()` 方法优先从缓存读取

- `crates/hakimi-session/src/session_ops.rs`
  - `create_session()` 后触发 `prefetch_memories()`

### 测试
- `crates/hakimi-context/tests/memory_cache_test.rs` (新增)

---

## 🛠️ 实施步骤

### 步骤 1: 实现 MemoryCache 结构体 (90 分钟)

**文件**: `crates/hakimi-context/src/memory_cache.rs`

```rust
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::fs;
use tokio::sync::RwLock;

/// 记忆文件缓存条目
#[derive(Debug, Clone)]
struct CacheEntry {
    content: String,
    loaded_at: Instant,
    file_size: u64,
    mtime: std::time::SystemTime,
}

/// 记忆文件缓存
pub struct MemoryCache {
    cache: Arc<RwLock<HashMap<PathBuf, CacheEntry>>>,
    ttl: Duration,
    max_size_bytes: usize,
}

impl MemoryCache {
    /// 创建新的记忆缓存
    pub fn new(ttl_minutes: u64, max_size_mb: usize) -> Self {
        Self {
            cache: Arc::new(RwLock::new(HashMap::new())),
            ttl: Duration::from_secs(ttl_minutes * 60),
            max_size_bytes: max_size_mb * 1024 * 1024,
        }
    }

    /// 异步预取记忆文件
    pub async fn prefetch(&self, file_path: &Path) -> Result<(), std::io::Error> {
        let metadata = match fs::metadata(file_path).await {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // 文件不存在，缓存空内容
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

        // 检查总缓存大小
        let mut cache = self.cache.write().await;
        let total_size: u64 = cache.values().map(|e| e.file_size).sum();
        
        if total_size + metadata.len() > self.max_size_bytes as u64 {
            // 驱逐最旧的条目
            if let Some(oldest_key) = cache
                .iter()
                .min_by_key(|(_, e)| e.loaded_at)
                .map(|(k, _)| k.clone())
            {
                cache.remove(&oldest_key);
            }
        }

        cache.insert(file_path.to_path_buf(), entry);
        Ok(())
    }

    /// 获取缓存内容（如果有效）
    pub async fn get_cached(&self, file_path: &Path) -> Option<String> {
        let cache = self.cache.read().await;
        
        if let Some(entry) = cache.get(file_path) {
            // 检查 TTL
            if entry.loaded_at.elapsed() > self.ttl {
                return None;
            }

            // 检查文件是否被修改
            if let Ok(metadata) = std::fs::metadata(file_path) {
                if let Ok(mtime) = metadata.modified() {
                    if mtime != entry.mtime {
                        return None; // 文件已修改
                    }
                }
            }

            return Some(entry.content.clone());
        }

        None
    }

    /// 使缓存条目失效
    pub async fn invalidate(&self, file_path: &Path) {
        self.cache.write().await.remove(file_path);
    }

    /// 清空所有缓存
    pub async fn clear(&self) {
        self.cache.write().await.clear();
    }

    /// 获取缓存统计信息
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
```

---

### 步骤 2: 集成到 Memory 模块 (60 分钟)

**文件**: `crates/hakimi-context/src/memory.rs`

```rust
use crate::memory_cache::MemoryCache;
use std::sync::Arc;

pub struct MemoryManager {
    home_dir: PathBuf,
    cache: Arc<MemoryCache>,
}

impl MemoryManager {
    pub fn new(home_dir: impl AsRef<Path>) -> Self {
        Self {
            home_dir: home_dir.as_ref().to_path_buf(),
            cache: Arc::new(MemoryCache::new(30, 10)), // 30 分钟 TTL, 10MB 上限
        }
    }

    /// 异步预取用户记忆
    pub async fn prefetch_user_memory(&self) -> Result<(), std::io::Error> {
        let user_md = self.home_dir.join("memory/user.md");
        self.cache.prefetch(&user_md).await
    }

    /// 异步预取会话记忆
    pub async fn prefetch_session_memories(&self) -> Result<(), std::io::Error> {
        let memory_md = self.home_dir.join("memory/memory.md");
        let working_md = self.home_dir.join("memory/working_memory.md");

        // 并行预取
        let cache = self.cache.clone();
        let (r1, r2) = tokio::join!(
            cache.prefetch(&memory_md),
            cache.prefetch(&working_md)
        );

        r1.and(r2)
    }

    /// 加载用户记忆（优先从缓存）
    pub async fn load_user_memory(&self) -> Result<String, std::io::Error> {
        let user_md = self.home_dir.join("memory/user.md");

        // 尝试从缓存获取
        if let Some(content) = self.cache.get_cached(&user_md).await {
            return Ok(content);
        }

        // 缓存未命中，同步加载并缓存
        let content = tokio::fs::read_to_string(&user_md).await?;
        self.cache.prefetch(&user_md).await.ok(); // 异步缓存，不等待
        Ok(content)
    }

    // 类似实现 load_session_memory() ...
}
```

---

### 步骤 3: 触发预取 (30 分钟)

**文件**: `crates/hakimi-session/src/session_ops.rs`

```rust
impl SessionDB {
    pub fn create_session(
        &self,
        platform: &str,
        user_id: Option<&str>,
    ) -> Result<String> {
        let session_id = self.create_session_impl(platform, user_id)?;

        // 异步预取记忆（不阻塞）
        let memory_mgr = MemoryManager::new(&get_hakimi_home());
        tokio::spawn(async move {
            if let Err(e) = memory_mgr.prefetch_user_memory().await {
                warn!("Failed to prefetch user memory: {}", e);
            }
            if let Err(e) = memory_mgr.prefetch_session_memories().await {
                warn!("Failed to prefetch session memories: {}", e);
            }
        });

        Ok(session_id)
    }
}
```

---

### 步骤 4: 单元测试 (90 分钟)

**文件**: `crates/hakimi-context/tests/memory_cache_test.rs`

```rust
use hakimi_context::MemoryCache;
use std::path::PathBuf;
use tempfile::TempDir;
use tokio::fs;

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
async fn test_cache_invalidation_on_file_change() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("test.md");
    fs::write(&file, "v1").await.unwrap();

    let cache = MemoryCache::new(30, 10);
    cache.prefetch(&file).await.unwrap();

    // 修改文件
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    fs::write(&file, "v2").await.unwrap();

    // 缓存应失效
    let cached = cache.get_cached(&file).await;
    assert!(cached.is_none());
}

#[tokio::test]
async fn test_ttl_expiration() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("test.md");
    fs::write(&file, "content").await.unwrap();

    // 1 秒 TTL
    let cache = MemoryCache::new(0, 10);
    cache.prefetch(&file).await.unwrap();

    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    // 缓存应过期
    let cached = cache.get_cached(&file).await;
    assert!(cached.is_none());
}

#[tokio::test]
async fn test_max_size_eviction() {
    let tmp = TempDir::new().unwrap();
    let cache = MemoryCache::new(30, 1); // 1MB 上限

    // 创建 2 个 600KB 文件
    let file1 = tmp.path().join("large1.md");
    let file2 = tmp.path().join("large2.md");
    let large_content = "x".repeat(600_000);

    fs::write(&file1, &large_content).await.unwrap();
    fs::write(&file2, &large_content).await.unwrap();

    cache.prefetch(&file1).await.unwrap();
    cache.prefetch(&file2).await.unwrap();

    // 第一个文件应被驱逐
    let cached1 = cache.get_cached(&file1).await;
    let cached2 = cache.get_cached(&file2).await;
    
    assert!(cached1.is_none()); // 被驱逐
    assert!(cached2.is_some()); // 保留
}
```

---

### 步骤 5: 性能基准测试 (60 分钟)

**文件**: `crates/hakimi-context/benches/memory_prefetch_bench.rs`

```rust
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use hakimi_context::MemoryManager;
use tempfile::TempDir;
use tokio::runtime::Runtime;

fn bench_load_without_cache(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let tmp = TempDir::new().unwrap();
    let mgr = MemoryManager::new(tmp.path());

    // 创建 20KB 记忆文件
    let memory_md = tmp.path().join("memory/memory.md");
    std::fs::create_dir_all(memory_md.parent().unwrap()).unwrap();
    std::fs::write(&memory_md, "x".repeat(20_000)).unwrap();

    c.bench_function("load_memory_no_cache", |b| {
        b.to_async(&rt).iter(|| async {
            black_box(mgr.load_session_memory().await.unwrap())
        })
    });
}

fn bench_load_with_cache(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let tmp = TempDir::new().unwrap();
    let mgr = MemoryManager::new(tmp.path());

    let memory_md = tmp.path().join("memory/memory.md");
    std::fs::create_dir_all(memory_md.parent().unwrap()).unwrap();
    std::fs::write(&memory_md, "x".repeat(20_000)).unwrap();

    // 预热缓存
    rt.block_on(mgr.prefetch_session_memories()).unwrap();

    c.bench_function("load_memory_cached", |b| {
        b.to_async(&rt).iter(|| async {
            black_box(mgr.load_session_memory().await.unwrap())
        })
    });
}

criterion_group!(benches, bench_load_without_cache, bench_load_with_cache);
criterion_main!(benches);
```

预期结果:
- 无缓存: 50-100 μs (20KB 文件)
- 有缓存: < 1 μs (内存读取)

---

## 📊 完成检查清单

- [ ] `MemoryCache` 结构体实现完成
- [ ] `prefetch()` 异步方法实现完成
- [ ] `get_cached()` 缓存读取实现完成
- [ ] `invalidate()` 失效机制实现完成
- [ ] 集成到 `MemoryManager` 完成
- [ ] 会话创建触发预取完成
- [ ] 单元测试全部通过（4+ 测试用例）
- [ ] 性能基准测试完成，缓存命中 < 1μs
- [ ] 编译无错误：`cargo check`
- [ ] 集成测试通过：`cargo test --all`
- [ ] CHANGELOG 更新
- [ ] README 更新（可选）
- [ ] 版本号递增至 0.5.86
- [ ] PR 创建并合并

---

## 🔗 参考资料

- [Tokio async/await](https://tokio.rs/tokio/tutorial/async)
- [Rust RwLock](https://doc.rust-lang.org/std/sync/struct.RwLock.html)
- [LRU 缓存实现](https://github.com/jeromefroe/lru-rs)

---

## 📝 设计决策

### 为什么使用 RwLock 而非 Mutex？
- 缓存读多写少，RwLock 允许多个读者并发访问
- 写操作（预取、失效）较少，锁竞争低

### 为什么不使用 LRU 缓存库？
- 需要自定义 TTL 和 mtime 检查逻辑
- 简单 HashMap + 手动驱逐更可控

### 为什么后台预取而非懒加载？
- 首次响应延迟敏感（TTFB）
- 预取可在会话创建后立即开始，无需等待首次请求

---

**创建时间**: 2026-07-10 18:15 UTC  
**预计完成**: 2026-07-11 02:00 UTC（8 小时内）
