# 任务 2.1.1: Lineage 数据库 Schema 扩展

**目标**: 为会话表添加父子关系字段，支持会话树结构

**优先级**: P1（里程碑 2.1 第一个任务）

**依赖**: 任务 1.x（已完成）

---

## 背景

当前 Hakimi 会话系统缺少会话之间的关联关系追踪。Hermes Agent 通过 lineage 机制支持：
- 从某个会话创建子会话（/fork, /branch）
- 追溯会话树结构（parent → root）
- 在搜索时理解会话上下文关系

实现会话 lineage 是功能对齐的关键步骤。

---

## 实现步骤

### 1. 扩展 `sessions` 表 Schema

**文件**: `crates/hakimi-session/src/schema.rs`

**新增字段**:
```sql
ALTER TABLE sessions ADD COLUMN parent_id TEXT;
ALTER TABLE sessions ADD COLUMN root_id TEXT;
CREATE INDEX idx_lineage ON sessions(parent_id, root_id);
```

**字段语义**:
- `parent_id`: 直接父会话 ID（如果从某会话 fork 而来）
- `root_id`: 会话树的根节点 ID（便于快速查找整棵树）
- 新建会话时两个字段均为 `NULL`

### 2. 添加数据库迁移逻辑

**文件**: `crates/hakimi-session/src/migrations.rs`（新建）

**迁移版本**: `v2`

**逻辑**:
```rust
pub fn migrate_to_v2(conn: &Connection) -> Result<()> {
    // 1. 检查当前版本（从 metadata 表读取）
    // 2. 如果是 v1，执行 ALTER TABLE 语句
    // 3. 更新版本号到 v2
    // 4. 提交事务
}
```

**向后兼容**:
- 旧数据库自动升级，不破坏现有会话
- 新字段默认 `NULL`，对现有功能无影响

### 3. 更新 `Session` 结构体

**文件**: `crates/hakimi-session/src/types.rs`

**修改**:
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub user_id: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub title: Option<String>,
    pub parent_id: Option<String>,  // 新增
    pub root_id: Option<String>,    // 新增
}
```

### 4. 更新会话创建逻辑

**文件**: `crates/hakimi-session/src/session_ops.rs`

**方法**: `create_session()`

**修改**:
```rust
pub fn create_session(
    &self,
    user_id: &str,
    parent_id: Option<&str>,  // 新增参数
) -> Result<Session> {
    let session_id = generate_session_id();
    let root_id = if let Some(pid) = parent_id {
        // 查询父会话的 root_id，如果父会话本身是 root，则继承父 ID
        self.get_session_root(pid)?
    } else {
        None  // 新会话默认没有 root
    };

    conn.execute(
        "INSERT INTO sessions (id, user_id, parent_id, root_id, created_at, updated_at) 
         VALUES (?, ?, ?, ?, ?, ?)",
        params![session_id, user_id, parent_id, root_id, now, now],
    )?;

    // ...
}
```

### 5. 添加 Lineage 查询辅助方法

**文件**: `crates/hakimi-session/src/session_ops.rs`

**新增方法**:
```rust
impl SessionManager {
    /// 获取会话的 root ID（如果是子会话）
    pub fn get_session_root(&self, session_id: &str) -> Result<Option<String>> {
        // 查询 sessions 表的 root_id 字段
        // 如果为 NULL，返回 None
    }

    /// 检查会话是否有父会话
    pub fn has_parent(&self, session_id: &str) -> Result<bool> {
        // 查询 parent_id 是否为 NULL
    }

    /// 获取会话的深度（从 root 开始计数）
    pub fn get_session_depth(&self, session_id: &str) -> Result<usize> {
        // 递归或循环向上查找 parent_id，直到 NULL
        // 返回层级深度（root = 0）
    }
}
```

### 6. 单元测试

**文件**: `crates/hakimi-session/tests/test_lineage.rs`（新建）

**测试用例**:
```rust
#[test]
fn test_create_root_session() {
    // 创建根会话
    // 验证 parent_id 和 root_id 都是 NULL
}

#[test]
fn test_create_child_session() {
    // 创建根会话 A
    // 从 A 创建子会话 B
    // 验证 B.parent_id == A.id
    // 验证 B.root_id == A.id
}

#[test]
fn test_create_grandchild_session() {
    // 创建 A → B → C 三层会话
    // 验证 C.parent_id == B.id
    // 验证 C.root_id == A.id
}

#[test]
fn test_get_session_depth() {
    // 创建 3 层会话树
    // 验证深度计算正确：A=0, B=1, C=2
}

#[test]
fn test_migration_from_v1() {
    // 创建 v1 数据库（无 parent_id/root_id）
    // 运行迁移
    // 验证字段已添加，旧会话字段为 NULL
}
```

---

## 验收标准

1. ✅ 数据库迁移自动运行，旧数据库升级成功
2. ✅ 新会话可指定 `parent_id`，自动计算 `root_id`
3. ✅ 索引创建成功，查询性能无回归
4. ✅ 所有单元测试通过
5. ✅ 无破坏性变更（现有代码无需修改）

---

## 测试方法

```bash
# 1. 运行单元测试
cargo test --package hakimi-session test_lineage

# 2. 测试迁移逻辑
rm -rf /tmp/test_sessions.db
cargo run --example test_migration

# 3. 验证索引生效
sqlite3 ~/.hakimi/sessions.db ".schema sessions"
# 应该看到 parent_id, root_id 字段和 idx_lineage 索引
```

---

## 后续任务

完成本任务后，下一步是：
- **任务 2.1.2**: 实现 Lineage 查询 API（获取子会话列表、会话树等）
- **任务 2.1.3**: session_search 工具集成 lineage（搜索结果显示会话关系）
- **任务 2.1.4**: WebUI 可视化会话树

---

**状态**: ✅ 已完成  
**开始时间**: 2026-07-10  
**完成时间**: 2026-07-10
