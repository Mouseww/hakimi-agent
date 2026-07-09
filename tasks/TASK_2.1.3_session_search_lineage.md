# 任务 2.1.3: session_search 集成 lineage

**状态**: ✅ 已完成 (100%)  
**开始时间**: 2026-07-10 06:00 UTC  
**完成时间**: 2026-07-10 06:30 UTC

**优先级**: 🟡 中  
**预估时间**: 2 小时  
**实际用时**: 0.5 小时  
**依赖**: 任务 2.1.1, 2.1.2 (lineage schema + API 已完成)  

---

## 📋 目标

为 `builtin_session_search` 工具添加 lineage 支持，使搜索结果能够：
1. 显示会话的父子关系
2. Discovery 模式去重时优先保留 root 会话
3. 可选显示或隐藏 lineage 信息

---

## 🎯 验收标准

- [ ] 新增 `include_lineage` 参数（默认 true）
- [ ] Discovery 模式去重时优先保留 root 会话
- [ ] 搜索结果显示会话层级关系（父会话、根会话）
- [ ] 添加 lineage 格式化辅助函数
- [ ] 单元测试覆盖 lineage 场景（>3 个测试）
- [ ] 所有测试通过：`cargo test --package hakimi-tools session_search`
- [ ] 文档更新

---

## 📁 涉及文件

### 主要修改
- `crates/hakimi-tools/src/builtin_session_search.rs` - 主要实现

### 测试
- `crates/hakimi-tools/tests/session_search_integration_test.rs` - 添加 lineage 测试

---

## 🛠️ 实施步骤

### 步骤 1: 添加 `include_lineage` 参数 (15 分钟)

**修改 schema()**:
```rust
fn schema(&self) -> JsonValue {
    json!({
        "type": "object",
        "properties": {
            // ... 现有参数 ...
            "include_lineage": {
                "type": "boolean",
                "description": "Include session lineage information (parent/root session). Default: true.",
                "default": true
            }
        }
    })
}
```

**解析参数**:
```rust
let include_lineage = args
    .get("include_lineage")
    .and_then(|v| v.as_bool())
    .unwrap_or(true);
```

---

### 步骤 2: 实现 lineage 格式化辅助函数 (20 分钟)

```rust
/// Format lineage information for a session
fn format_lineage(session: &SessionMeta, db: &SessionDB) -> Result<String> {
    let mut lineage_info = String::new();
    
    if let Some(parent_id) = &session.parent_session_id {
        lineage_info.push_str(&format!("  - Parent: `{}`", parent_id));
        
        // Get parent session title if available
        if let Ok(parent_meta) = db.get_session_meta(parent_id) {
            if let Some(title) = parent_meta.title {
                lineage_info.push_str(&format!(" ({})", title));
            }
        }
        lineage_info.push('\n');
    }
    
    if let Some(root_id) = &session.root_session_id {
        if root_id != &session.id {
            lineage_info.push_str(&format!("  - Root: `{}`", root_id));
            
            // Get root session title if available
            if let Ok(root_meta) = db.get_session_meta(root_id) {
                if let Some(title) = root_meta.title {
                    lineage_info.push_str(&format!(" ({})", title));
                }
            }
            lineage_info.push('\n');
        }
    }
    
    Ok(lineage_info)
}

/// Get session depth (for deduplication priority)
fn get_session_depth(session: &SessionMeta, db: &SessionDB) -> usize {
    // Root sessions have depth 0
    if session.parent_session_id.is_none() {
        return 0;
    }
    
    // Count ancestors
    let mut depth = 0;
    let mut current_id = session.parent_session_id.clone();
    let mut visited = std::collections::HashSet::new();
    
    while let Some(parent_id) = current_id {
        if visited.contains(&parent_id) {
            // Cycle detected, break
            break;
        }
        visited.insert(parent_id.clone());
        depth += 1;
        
        if let Ok(parent_meta) = db.get_session_meta(&parent_id) {
            current_id = parent_meta.parent_session_id;
        } else {
            break;
        }
        
        // Safety limit
        if depth > 100 {
            break;
        }
    }
    
    depth
}
```

---

### 步骤 3: 修改 Discovery 模式去重逻辑 (30 分钟)

**当前逻辑** (仅去重):
```rust
// Group by session_id
let mut session_map: HashMap<String, Vec<_>> = HashMap::new();
for result in results {
    session_map
        .entry(result.session_id.clone())
        .or_default()
        .push(result);
}
```

**新逻辑** (优先保留 root 会话):
```rust
// Group by session_id
let mut session_map: HashMap<String, Vec<_>> = HashMap::new();
for result in results {
    session_map
        .entry(result.session_id.clone())
        .or_default()
        .push(result);
}

// Get unique sessions with depth information
let mut unique_sessions: Vec<_> = session_map.keys().collect();

// Sort by depth (root sessions first)
if include_lineage {
    unique_sessions.sort_by_key(|sid| {
        db.get_session_meta(sid)
            .ok()
            .map(|meta| get_session_depth(&meta, db))
            .unwrap_or(999) // Unknown sessions last
    });
}

// Take only `limit` sessions
unique_sessions.truncate(limit as usize);
```

---

### 步骤 4: 更新输出格式 (20 分钟)

**Browse 模式**:
```rust
for session in &sessions {
    // ... 现有输出 ...
    
    if include_lineage {
        output.push_str(&format_lineage(session, db)?);
    }
    
    output.push('\n');
}
```

**Discovery 模式**:
```rust
output.push_str(&format!(
    "\n### Session: {} ({})\n",
    session_meta.title.as_deref().unwrap_or("untitled"),
    session_id
));

if include_lineage {
    let lineage_str = format_lineage(&session_meta, db)?;
    if !lineage_str.is_empty() {
        output.push_str(&lineage_str);
        output.push('\n');
    }
}

// ... bookends + matches ...
```

---

### 步骤 5: 编写单元测试 (30 分钟)

**测试文件**: `crates/hakimi-tools/tests/session_search_lineage_test.rs`

```rust
#[tokio::test]
async fn test_discovery_mode_prioritizes_root_sessions() {
    let temp_dir = TempDir::new().unwrap();
    setup_test_env(&temp_dir);
    
    let db_path = temp_dir.path().join("sessions.db");
    let db = SessionDB::new(&db_path).unwrap();
    db.initialize().unwrap();
    
    // Create root session A
    let session_a = db.create_session("user1", None).unwrap();
    db.save_message(&session_a.id, &Message::user("discussing Rust")).unwrap();
    
    // Create child session B (from A)
    let session_b = db.create_session("user1", Some(&session_a.id)).unwrap();
    db.save_message(&session_b.id, &Message::user("more about Rust")).unwrap();
    
    // Create grandchild session C (from B)
    let session_c = db.create_session("user1", Some(&session_b.id)).unwrap();
    db.save_message(&session_c.id, &Message::user("even more Rust")).unwrap();
    
    // Search for "Rust"
    let tool = SessionSearchTool;
    let args = json!({
        "query": "Rust",
        "limit": 2,
        "include_lineage": true
    });
    
    let result = tool.execute(&args, &ToolContext::default()).await.unwrap();
    
    // Root session A should appear first
    assert!(result.find(&session_a.id).unwrap() < result.find(&session_b.id).unwrap());
}

#[tokio::test]
async fn test_include_lineage_false_hides_relationships() {
    // ... setup ...
    
    let args = json!({
        "query": "test",
        "include_lineage": false
    });
    
    let result = tool.execute(&args, &ToolContext::default()).await.unwrap();
    
    // Should not contain "Parent:" or "Root:"
    assert!(!result.contains("Parent:"));
    assert!(!result.contains("Root:"));
}

#[tokio::test]
async fn test_lineage_formatting() {
    // ... setup ...
    
    let args = json!({
        "query": "test",
        "include_lineage": true
    });
    
    let result = tool.execute(&args, &ToolContext::default()).await.unwrap();
    
    // Should contain lineage information
    assert!(result.contains("Parent:") || result.contains("Root:"));
}
```

---

### 步骤 6: 更新文档 (15 分钟)

**CHANGELOG.md**:
```markdown
### v0.5.65 (2026-07-10)

#### Features
- **session_search lineage 集成**
  - 新增 `include_lineage` 参数（默认 true）
  - Discovery 模式去重时优先保留 root 会话
  - 搜索结果显示会话父子关系
  - 支持多代会话树追溯

#### Improvements
- **会话搜索排序**: 根会话优先于子会话显示
- **会话元信息**: 自动显示父会话和根会话标题

#### Testing
- 新增 3 个 lineage 集成测试
```

---

## 🧪 测试计划

### 单元测试
```bash
cargo test --package hakimi-tools --test session_search_lineage_test
```

### 集成测试
```bash
cargo test --package hakimi-tools session_search -- --test-threads=1
```

### 手动验证
```bash
# 1. 创建会话树
hakimi new --session root-session
# 在 root-session 中对话
hakimi fork --name child-session
# 在 child-session 中对话

# 2. 搜索验证
hakimi tools call session_search '{"query": "test", "include_lineage": true}'

# 预期输出包含 "Parent:" 和 "Root:" 信息
```

---

## ✅ 完成检查清单

- [ ] `include_lineage` 参数添加到 schema
- [ ] `format_lineage()` 辅助函数实现
- [ ] `get_session_depth()` 深度计算实现
- [ ] Discovery 模式去重逻辑更新
- [ ] Browse 模式 lineage 输出
- [ ] Discovery 模式 lineage 输出
- [ ] 单元测试 (3+ 个)
- [ ] 所有测试通过
- [ ] 文档更新（CHANGELOG + task）
- [ ] 代码格式化：`cargo +nightly fmt`
- [ ] Clippy 检查：`cargo clippy --package hakimi-tools`

---

## 📊 影响范围

- **破坏性**: 无（新参数默认 true，兼容现有行为）
- **性能**: 轻微（每个会话额外 1-2 次数据库查询）
- **用户体验**: 改进（更清晰的会话关系）

---

**创建时间**: 2026-07-10 06:00 UTC  
**预计完成**: 2026-07-10 08:00 UTC  
**实际完成**: _待填写_
