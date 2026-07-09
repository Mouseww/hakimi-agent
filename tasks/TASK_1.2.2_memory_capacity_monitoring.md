# 任务 1.2.2: 记忆容量监控

**目标**: 添加记忆文件大小限制和警告机制，防止记忆文件过大影响性能

**优先级**: P1（里程碑 1.2 第二个任务）

**依赖**: 任务 1.2.1（工作记忆生命周期管理，已完成）

---

## 背景

当前系统的记忆文件（`user.md`, `memory.md`, `working_memory.md`）没有大小限制，可能导致：
1. 加载到 system prompt 时占用大量 token
2. 文件读写性能下降
3. 意外的巨大上下文窗口消耗

需要实现：
- **软限制（60KB）**：警告日志，提示用户清理
- **硬限制（64KB）**：拒绝加载，返回错误

---

## 实现步骤

### 1. 在 `FileMemoryProvider` 中添加容量检查

**文件**: `crates/hakimi-context/src/memory.rs`

**实现位置**: `system_prompt_block()` 方法中，读取文件后检查大小

**逻辑**:
```rust
const MEMORY_WARN_SIZE_BYTES: u64 = 60 * 1024;  // 60 KB
const MEMORY_MAX_SIZE_BYTES: u64 = 64 * 1024;   // 64 KB

// In system_prompt_block():
for entry in entries.flatten() {
    let path = entry.path();
    if !path.is_file() {
        continue;
    }

    // Check file size
    match std::fs::metadata(&path) {
        Ok(metadata) => {
            let size = metadata.len();
            let name = path.file_stem().and_then(|n| n.to_str()).unwrap_or("unknown");

            if size > MEMORY_MAX_SIZE_BYTES {
                error!(
                    path = %path.display(),
                    size_kb = size / 1024,
                    limit_kb = MEMORY_MAX_SIZE_BYTES / 1024,
                    "Memory file exceeds maximum size, skipping load"
                );
                return format!(
                    "[ERROR] Memory file '{}' is too large ({} KB > {} KB limit). \
                     Please clean up or archive old content.",
                    name, size / 1024, MEMORY_MAX_SIZE_BYTES / 1024
                );
            } else if size > MEMORY_WARN_SIZE_BYTES {
                warn!(
                    path = %path.display(),
                    size_kb = size / 1024,
                    "Memory file approaching size limit, consider cleaning up"
                );
            }
        }
        Err(e) => {
            warn!(path = %path.display(), error = %e, "Failed to get file metadata");
            continue;
        }
    }

    // ... existing file reading logic ...
}
```

### 2. 添加专用方法检查单个文件大小

**方法签名**:
```rust
impl FileMemoryProvider {
    /// Check if a memory file exceeds size limits.
    ///
    /// Returns:
    /// - `Ok(())` if within limits
    /// - `Err(...)` with descriptive message if exceeds hard limit
    ///
    /// Logs warning if exceeds soft limit (60KB) but still returns Ok.
    pub fn check_file_size(&self, filename: &str) -> std::result::Result<(), String> {
        let path = self.memory_dir.join(filename);
        
        if !path.exists() {
            return Ok(()); // File doesn't exist yet, no problem
        }

        match std::fs::metadata(&path) {
            Ok(metadata) => {
                let size = metadata.len();
                
                if size > MEMORY_MAX_SIZE_BYTES {
                    Err(format!(
                        "Memory file '{}' exceeds maximum size ({} KB > {} KB). \
                         Please clean up or use 'hakimi memory archive' command.",
                        filename, size / 1024, MEMORY_MAX_SIZE_BYTES / 1024
                    ))
                } else if size > MEMORY_WARN_SIZE_BYTES {
                    warn!(
                        file = filename,
                        size_kb = size / 1024,
                        "Memory file approaching size limit"
                    );
                    Ok(())
                } else {
                    Ok(())
                }
            }
            Err(e) => Err(format!("Failed to check file size: {}", e))
        }
    }
}
```

### 3. 在 MemoryTool 中集成容量检查

**文件**: `crates/hakimi-tools/src/builtin_memory.rs`

**位置**: `execute()` 方法中，在写入文件前调用 `check_file_size()`

**修改**:
```rust
// Before writing (in "add" and "replace" actions):
// Note: This assumes MemoryTool can access FileMemoryProvider somehow,
// or we add a standalone check_file_size() function in the module.

fn check_memory_size(file_path: &Path) -> Result<()> {
    const MEMORY_WARN_SIZE_BYTES: u64 = 60 * 1024;
    const MEMORY_MAX_SIZE_BYTES: u64 = 64 * 1024;

    if !file_path.exists() {
        return Ok(());
    }

    let metadata = std::fs::metadata(file_path)
        .map_err(|e| HakimiError::MemorySimple(format!("Failed to check file size: {e}")))?;
    let size = metadata.len();

    if size > MEMORY_MAX_SIZE_BYTES {
        return Err(HakimiError::Memory(MemoryError::FileTooLarge {
            target: file_path.display().to_string(),
            size: size as usize,
            limit: MEMORY_MAX_SIZE_BYTES as usize,
        }));
    } else if size > MEMORY_WARN_SIZE_BYTES {
        warn!(
            path = %file_path.display(),
            size_kb = size / 1024,
            "Memory file approaching size limit"
        );
    }

    Ok(())
}

// In execute(), before fs::write():
check_memory_size(&file_path)?;
```

---

## 测试

### 测试用例 1: 正常大小文件（< 60KB）
```rust
#[test]
fn test_memory_size_within_limits() {
    let temp_dir = TempDir::new().unwrap();
    let provider = FileMemoryProvider::new(temp_dir.path());
    
    // Create a 30KB file
    let path = temp_dir.path().join("memory.md");
    let content = "x".repeat(30 * 1024);
    std::fs::write(&path, content).unwrap();
    
    let result = provider.check_file_size("memory.md");
    assert!(result.is_ok(), "30KB file should be accepted");
}
```

### 测试用例 2: 警告区间文件（60KB ~ 64KB）
```rust
#[test]
fn test_memory_size_warning_zone() {
    let temp_dir = TempDir::new().unwrap();
    let provider = FileMemoryProvider::new(temp_dir.path());
    
    // Create a 62KB file
    let path = temp_dir.path().join("memory.md");
    let content = "x".repeat(62 * 1024);
    std::fs::write(&path, content).unwrap();
    
    let result = provider.check_file_size("memory.md");
    assert!(result.is_ok(), "62KB file should still be accepted with warning");
    // Note: Check logs manually for warning output
}
```

### 测试用例 3: 超限文件（> 64KB）
```rust
#[test]
fn test_memory_size_exceeds_limit() {
    let temp_dir = TempDir::new().unwrap();
    let provider = FileMemoryProvider::new(temp_dir.path());
    
    // Create a 70KB file
    let path = temp_dir.path().join("memory.md");
    let content = "x".repeat(70 * 1024);
    std::fs::write(&path, content).unwrap();
    
    let result = provider.check_file_size("memory.md");
    assert!(result.is_err(), "70KB file should be rejected");
    assert!(result.unwrap_err().contains("exceeds maximum size"));
}
```

---

## 验收标准

- [ ] 常量定义：`MEMORY_WARN_SIZE_BYTES` (60KB), `MEMORY_MAX_SIZE_BYTES` (64KB)
- [ ] `system_prompt_block()` 中集成大小检查
- [ ] 新增 `check_file_size()` 方法
- [ ] 超限时返回友好错误信息（包含大小和限制）
- [ ] 警告区间使用 `warn!` 日志
- [ ] 硬限制使用 `error!` 日志
- [ ] 至少 3 个测试用例通过
- [ ] 编译无错误：`cargo build`

---

## 用户提示信息

当文件超限时，错误信息应引导用户行动：
```
Memory file 'memory.md' exceeds maximum size (70 KB > 64 KB).
Please clean up old content or use 'hakimi memory archive' command to archive old entries.
```

---

## 后续任务

- 任务 1.2.3: 实现 `hakimi memory archive` CLI 命令（引用自此错误提示）
- 任务 1.3.x: 补充测试覆盖率

---

## 技术注意事项

1. **性能**：`std::fs::metadata()` 调用开销小，在加载记忆时执行可接受
2. **兼容性**：使用 `tracing::warn!` 和 `error!` 而非 `info!`（警告级别更高）
3. **错误类型**：使用 `HakimiError::Memory(MemoryError::FileTooLarge { ... })`（任务 1.1.3 已定义）
4. **文件检查时机**：
   - `system_prompt_block()`: 加载时检查（阻止过大文件进入 prompt）
   - `MemoryTool::execute()`: 写入前检查（阻止文件增长超限）

---

**预计工作量**: 2-3 小时（含测试）
