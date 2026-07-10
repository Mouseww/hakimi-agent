# TASK 3.2.1: Tool Call Result Caching

**状态**: 🔄 待执行 (0%)  
**优先级**: P2  
**预计工作量**: 3-4 小时  
**依赖**: 无

## 📋 任务目标

为工具调用实现智能缓存机制，避免重复执行相同参数的工具调用，提升响应速度和资源利用率。

## 🎯 成功标准

- [x] 工具调用结果自动缓存
- [x] 支持基于参数的缓存键生成
- [x] 可配置的缓存过期策略
- [x] 缓存命中率监控
- [x] 支持缓存清除和失效
- [x] 单元测试覆盖 ≥ 90%

## 🔧 实现步骤

### 1. 实现缓存层

**文件**: `crates/hakimi-tools/src/cache.rs` (新建)

```rust
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub struct ToolCallCache {
    entries: Arc<Mutex<HashMap<String, CacheEntry>>>,
    config: CacheConfig,
}

pub struct CacheEntry {
    pub result: serde_json::Value,
    pub created_at: Instant,
    pub hit_count: usize,
}

pub struct CacheConfig {
    pub ttl: Duration,
    pub max_entries: usize,
    pub enable_cache: bool,
}

impl ToolCallCache {
    pub fn new(config: CacheConfig) -> Self {
        Self {
            entries: Arc::new(Mutex::new(HashMap::new())),
            config,
        }
    }
    
    pub fn get(&self, key: &str) -> Option<serde_json::Value> {
        if !self.config.enable_cache {
            return None;
        }
        
        let mut entries = self.entries.lock().unwrap();
        
        if let Some(entry) = entries.get_mut(key) {
            if entry.created_at.elapsed() < self.config.ttl {
                entry.hit_count += 1;
                return Some(entry.result.clone());
            } else {
                // 过期，移除
                entries.remove(key);
            }
        }
        
        None
    }
    
    pub fn set(&self, key: String, result: serde_json::Value) {
        if !self.config.enable_cache {
            return;
        }
        
        let mut entries = self.entries.lock().unwrap();
        
        // LRU 淘汰
        if entries.len() >= self.config.max_entries {
            self.evict_lru(&mut entries);
        }
        
        entries.insert(key, CacheEntry {
            result,
            created_at: Instant::now(),
            hit_count: 0,
        });
    }
    
    pub fn invalidate(&self, key: &str) {
        let mut entries = self.entries.lock().unwrap();
        entries.remove(key);
    }
    
    pub fn clear(&self) {
        let mut entries = self.entries.lock().unwrap();
        entries.clear();
    }
    
    pub fn stats(&self) -> CacheStats {
        let entries = self.entries.lock().unwrap();
        
        let total_hits: usize = entries.values().map(|e| e.hit_count).sum();
        let total_entries = entries.len();
        
        CacheStats {
            total_entries,
            total_hits,
            hit_rate: if total_entries > 0 {
                total_hits as f64 / (total_entries + total_hits) as f64
            } else {
                0.0
            },
        }
    }
    
    fn evict_lru(&self, entries: &mut HashMap<String, CacheEntry>) {
        // 移除最久未使用的条目
        if let Some(oldest_key) = entries.iter()
            .min_by_key(|(_, entry)| entry.created_at)
            .map(|(k, _)| k.clone())
        {
            entries.remove(&oldest_key);
        }
    }
}

pub struct CacheStats {
    pub total_entries: usize,
    pub total_hits: usize,
    pub hit_rate: f64,
}
```

### 2. 生成缓存键

**文件**: `crates/hakimi-tools/src/cache_key.rs` (新建)

```rust
use sha2::{Sha256, Digest};

pub fn generate_cache_key(tool_name: &str, params: &serde_json::Value) -> String {
    let mut hasher = Sha256::new();
    hasher.update(tool_name.as_bytes());
    hasher.update(params.to_string().as_bytes());
    format!("{:x}", hasher.finalize())
}

pub fn generate_cache_key_with_context(
    tool_name: &str,
    params: &serde_json::Value,
    context: Option<&str>,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(tool_name.as_bytes());
    hasher.update(params.to_string().as_bytes());
    
    if let Some(ctx) = context {
        hasher.update(ctx.as_bytes());
    }
    
    format!("{:x}", hasher.finalize())
}
```

### 3. 集成缓存到工具调度

**文件**: `crates/hakimi-core/src/loop_impl.rs`

```rust
pub async fn dispatch_tool(
    tool_call: &ToolCall,
    tools: &[Box<dyn Tool>],
    cache: Option<&ToolCallCache>,
) -> Result<String, ToolError> {
    // 生成缓存键
    if let Some(cache) = cache {
        let cache_key = generate_cache_key(&tool_call.name, &tool_call.arguments);
        
        // 检查缓存
        if let Some(cached_result) = cache.get(&cache_key) {
            tracing::info!("Tool call cache hit: {}", tool_call.name);
            return Ok(cached_result.as_str().unwrap_or("").to_string());
        }
    }
    
    // 执行工具
    let result = execute_tool(tool_call, tools).await?;
    
    // 缓存结果
    if let Some(cache) = cache {
        let cache_key = generate_cache_key(&tool_call.name, &tool_call.arguments);
        cache.set(cache_key, serde_json::Value::String(result.clone()));
    }
    
    Ok(result)
}
```

### 4. 添加缓存配置

**文件**: `crates/hakimi-config/src/tool_config.rs`

```rust
#[derive(Serialize, Deserialize, Clone)]
pub struct ToolCacheConfig {
    pub enable_cache: bool,
    pub ttl_seconds: u64,
    pub max_entries: usize,
    pub cacheable_tools: Vec<String>,  // 可缓存工具白名单
}

impl Default for ToolCacheConfig {
    fn default() -> Self {
        Self {
            enable_cache: true,
            ttl_seconds: 300,  // 5 分钟
            max_entries: 1000,
            cacheable_tools: vec![
                "read_file".to_string(),
                "search_files".to_string(),
                "terminal".to_string(),  // 只缓存幂等命令
            ],
        }
    }
}
```

### 5. 添加缓存 API

**文件**: `crates/hakimi-server/src/routes/tools.rs`

```rust
// GET /api/tools/cache/stats
pub async fn get_cache_stats(
    State(state): State<Arc<AppState>>,
) -> Result<Json<CacheStats>, StatusCode> {
    let cache = state.tool_cache.lock().await;
    Ok(Json(cache.stats()))
}

// DELETE /api/tools/cache
pub async fn clear_cache(
    State(state): State<Arc<AppState>>,
) -> Result<StatusCode, StatusCode> {
    let cache = state.tool_cache.lock().await;
    cache.clear();
    Ok(StatusCode::NO_CONTENT)
}

// DELETE /api/tools/cache/:key
pub async fn invalidate_cache_entry(
    Path(key): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Result<StatusCode, StatusCode> {
    let cache = state.tool_cache.lock().await;
    cache.invalidate(&key);
    Ok(StatusCode::NO_CONTENT)
}
```

### 6. 单元测试

**文件**: `crates/hakimi-tools/src/cache_test.rs`

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_cache_hit() {
        // 测试缓存命中
    }
    
    #[test]
    fn test_cache_miss() {
        // 测试缓存未命中
    }
    
    #[test]
    fn test_cache_expiration() {
        // 测试缓存过期
    }
    
    #[test]
    fn test_lru_eviction() {
        // 测试 LRU 淘汰
    }
    
    #[test]
    fn test_cache_key_generation() {
        // 测试缓存键生成
    }
    
    #[test]
    fn test_cache_stats() {
        // 测试缓存统计
    }
}
```

## 🔍 验证清单

- [ ] 所有单元测试通过
- [ ] 相同参数的工具调用正确命中缓存
- [ ] 缓存过期后自动失效
- [ ] LRU 淘汰策略正常工作
- [ ] 缓存统计准确
- [ ] 缓存键冲突率 < 0.01%
- [ ] 缓存命中延迟 < 1ms

## 📊 性能指标

- 缓存查询延迟: < 1ms
- 缓存命中率: > 30% (典型场景)
- 内存占用: < 100MB (1000 条目)
- LRU 淘汰延迟: < 10ms

## 🔗 相关文件

- `crates/hakimi-tools/src/cache.rs` (新建)
- `crates/hakimi-tools/src/cache_key.rs` (新建)
- `crates/hakimi-core/src/loop_impl.rs`
- `crates/hakimi-config/src/tool_config.rs`
- `crates/hakimi-server/src/routes/tools.rs`

## 📝 注意事项

1. 只缓存幂等工具调用（读操作）
2. 不缓存副作用工具（写操作、网络请求）
3. 考虑参数顺序对缓存键的影响
4. 大型结果应考虑压缩存储
5. 支持手动失效特定工具的缓存
