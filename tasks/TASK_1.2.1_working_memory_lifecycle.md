# 任务 1.2.1: 工作记忆生命周期管理

**状态**: ✅ 已完成 (100%)  
**完成时间**: 2026-07-10 00:15 UTC

**目标**: 实现会话结束时自动清理工作记忆（working_memory）

**优先级**: P1（里程碑 1.2 第一个任务）

**依赖**: 任务 1.1.x（已完成）

---

## 背景

当前系统支持三层记忆：
- `user.md`: 用户档案（稳定信息）
- `memory.md`: 长期个人笔记（agent 持久化知识）
- `working_memory.md`: 当前会话临时记忆（会话结束后应清空）

问题：working_memory.md 在会话结束后不会自动清理，导致上次会话的临时记忆泄漏到新会话。

---

## 实现步骤

### 1. 为 `FileMemoryProvider` 添加 `finalize_session()` 方法

**文件**: `crates/hakimi-context/src/memory.rs`

**逻辑**:
```rust
/// Finalize the current session by:
/// 1. Reading working_memory.md
/// 2. If non-empty, appending to memory.md with timestamp
/// 3. Clearing working_memory.md
/// 4. Logging the operation
pub fn finalize_session(&self) -> Result<(), Box<dyn std::error::Error>> {
    let working_path = self.memory_dir.join("working_memory.md");
    let memory_path = self.memory_dir.join("memory.md");

    // 1. Read working memory
    let working_content = match std::fs::read_to_string(&working_path) {
        Ok(c) => c.trim().to_string(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e.into()),
    };

    // 2. If non-empty, archive to memory.md
    if !working_content.is_empty() {
        let timestamp = chrono::Utc::now().format("%Y-%m-%d %H:%M UTC");
        let archive_section = format!(
            "\n\n---\n[Session ended: {}]\n{}",
            timestamp, working_content
        );

        let mut memory_content = std::fs::read_to_string(&memory_path).unwrap_or_default();
        memory_content.push_str(&archive_section);
        std::fs::write(&memory_path, memory_content)?;

        tracing::info!(
            chars = working_content.chars().count(),
            "Archived working memory to memory.md"
        );
    }

    // 3. Clear working_memory.md
    std::fs::write(&working_path, "")?;

    Ok(())
}
```

### 2. 在 Gateway 处理 `/new` 命令时调用

**选项 A**: 在 `hakimi-gateway` 中注册钩子（如果存在命令处理中心）

**选项 B**: 在 CLI 的主循环中处理（测试环境）

**验收**: 
- 调用 finalize_session() 后，working_memory.md 被清空
- 如果原内容非空，memory.md 末尾追加带时间戳的归档块
- 日志记录操作

### 3. 测试

**文件**: `crates/hakimi-context/src/memory.rs` (同文件添加测试)

**测试用例**:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_finalize_session_empty() {
        let temp_dir = TempDir::new().unwrap();
        let provider = FileMemoryProvider::new(temp_dir.path());
        
        // Working memory doesn't exist
        let result = provider.finalize_session();
        assert!(result.is_ok());
        
        let working_path = temp_dir.path().join("working_memory.md");
        assert_eq!(std::fs::read_to_string(&working_path).unwrap(), "");
    }

    #[test]
    fn test_finalize_session_with_content() {
        let temp_dir = TempDir::new().unwrap();
        let provider = FileMemoryProvider::new(temp_dir.path());
        
        // Create working memory with content
        let working_path = temp_dir.path().join("working_memory.md");
        std::fs::write(&working_path, "Temporary note 123").unwrap();
        
        // Finalize
        provider.finalize_session().unwrap();
        
        // Working memory should be empty
        assert_eq!(std::fs::read_to_string(&working_path).unwrap(), "");
        
        // Memory should contain archived content
        let memory_path = temp_dir.path().join("memory.md");
        let memory_content = std::fs::read_to_string(&memory_path).unwrap();
        assert!(memory_content.contains("Temporary note 123"));
        assert!(memory_content.contains("[Session ended:"));
    }
}
```

---

## 验收标准

- [ ] `FileMemoryProvider::finalize_session()` 方法实现完成
- [ ] 测试用例通过（至少 2 个测试）
- [ ] 编译无错误：`cargo build`
- [ ] 日志输出正确（使用 tracing::info）
- [ ] 为 Gateway `/new` 命令集成留出扩展点（后续任务）

---

## 技术细节

**依赖新增**: 可能需要添加 `chrono` crate 用于时间戳（检查 Cargo.toml 是否已有）

**日志级别**: 使用 `tracing::info!` 而非 `debug!`，因为这是重要的会话生命周期事件

**错误处理**: 
- 文件不存在：视为空内容（正常场景）
- 读写失败：向上传播错误
- 空内容：跳过归档，直接清空文件

---

## 后续任务

- 任务 1.2.2: 添加记忆容量监控（60KB 警告，64KB 拒绝）
- 任务 1.2.3: 记忆归档机制（CLI 命令）
- 集成到 Gateway 命令处理流程（需跨 crate 协调）
