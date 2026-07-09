# 任务 2.1.2: Lineage 查询 API

## 目标
实现会话谱系（Lineage）的查询 API，支持获取会话的祖先链和根会话。

## 背景
任务 2.1.1 已经完成了 schema 扩展，添加了 `parent_id` 和 `root_id` 字段。现在需要提供查询接口来遍历和检索会话的父子关系链。

## 实施步骤

### 1. 实现核心查询方法

**文件**: `crates/hakimi-session/src/session_ops.rs`

需要添加以下方法：

```rust
/// 获取会话的完整谱系链（从当前会话到根会话）
fn get_session_lineage(&self, session_id: &str) -> Result<Vec<SessionMetadata>>;

/// 获取会话树的根会话
fn get_root_session(&self, session_id: &str) -> Result<SessionMetadata>;

/// 获取指定会话的所有子会话
fn get_child_sessions(&self, session_id: &str) -> Result<Vec<SessionMetadata>>;
```

**实现要点**:
- `get_session_lineage`: 递归或迭代查询 parent_id 直到 root
- `get_root_session`: 直接通过 root_id 查询，或者递归找到 parent_id = NULL 的会话
- `get_child_sessions`: 查询所有 parent_id = session_id 的会话
- 防止循环引用（检测已访问的会话 ID）
- 处理孤儿会话（parent_id 指向不存在的会话）

### 2. 编写单元测试

**文件**: `crates/hakimi-session/tests/test_lineage.rs`（扩展现有文件）

测试场景：
1. **三代会话树**: 创建 root → child → grandchild，验证：
   - `get_session_lineage(grandchild_id)` 返回 3 个会话
   - `get_root_session(grandchild_id)` 返回 root
   - `get_child_sessions(root_id)` 返回 child

2. **多分支树**: root 有 2 个 child，每个 child 有 1 个 grandchild
   - 验证 `get_child_sessions(root_id)` 返回 2 个会话
   - 验证每个分支的 lineage 独立

3. **孤立会话**: 会话没有 parent_id
   - `get_session_lineage` 返回单个会话
   - `get_root_session` 返回自身

4. **错误处理**:
   - parent_id 指向不存在的会话
   - session_id 不存在
   - 循环引用检测（如果可能构造）

### 3. 集成到现有 API

确保新方法与现有的 SessionStore trait 兼容，可能需要：
- 更新 trait 定义
- 为 SQLite 和内存实现提供具体实现
- 更新文档字符串

## 验收标准

- [ ] `get_session_lineage` 正确返回从当前到根的完整链
- [ ] `get_root_session` 能快速定位根会话
- [ ] `get_child_sessions` 返回所有直接子会话
- [ ] 单元测试覆盖 3 代会话树和多分支场景
- [ ] 所有测试通过 `cargo test --package hakimi-session`
- [ ] 代码通过 `cargo clippy`
- [ ] 文档完整，包含使用示例

## 依赖
- 任务 2.1.1（schema 扩展）必须完成

## 后续任务
- 任务 2.1.3: session_search 集成 lineage
- 任务 2.1.4: WebUI 可视化会话树

## 预计工时
1 天（8 小时）
