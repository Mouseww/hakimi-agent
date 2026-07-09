# 任务 1.2.3: 记忆归档机制

**优先级**: 中  
**预计时间**: 2天  
**依赖**: 任务 1.2.1, 1.2.2  
**标签**: `记忆管理`, `CLI`, `存储优化`
**状态**: ✅ 已完成

---

## 目标

实现记忆归档机制，允许用户将旧的记忆数据移动到归档目录，同时保留引用索引。

## 实现内容

### 核心功能
1. **MemoryArchive 结构体** (`crates/hakimi-context/src/archive.rs`)
   - `archive_before(cutoff_date)`: 归档指定日期前的记忆
   - `list_archives()`: 列出所有归档
   - `restore_archive(year_month)`: 恢复指定月份的归档

2. **归档组织**
   - 按年-月分组：`~/.hakimi/memory/archive/2026-01/memory_archived.md`
   - 自动备份原文件（带时间戳）
   - 在 memory.md 中保留归档索引

3. **数据结构**
   - `MemoryEntry`: 时间戳 + 内容 + 元数据
   - `ArchiveStats`: 归档统计信息
   - `ArchiveInfo`: 归档月份信息

### 测试覆盖
- ✅ 时间戳解析（多种格式）
- ✅ 按月分组逻辑
- ✅ 完整归档流程
- ✅ 列出归档
- ✅ 恢复归档
- ✅ 备份机制

所有测试通过（6/6）。

### 下一步
- [ ] CLI 命令集成：`hakimi memory archive/restore/list`
- [ ] 性能基准测试（10K+ 条记忆）
- [ ] 归档搜索支持

---

**完成日期**: 2026-07-10  
**版本**: v0.5.59
