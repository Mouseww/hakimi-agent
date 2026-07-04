# Changelog

All notable changes to Hakimi Agent will be documented in this file.

## [0.5.19] - 2026-07-04

### Added
- **Team 工具任务分工增强**：新增 `tasks` 参数，支持为每个 teammate 分配不同的子任务
  - 旧模式（`teammates` 数组）：所有 agent 接收相同任务（已标记为 DEPRECATED）
  - 新模式（`tasks` 数组）：每个 agent 接收专属的 `task` 和 `context`，实现真正的任务分工
  - 示例：`{"tasks": [{"teammate": "researcher", "task": "搜索解决方案"}, {"teammate": "coder", "task": "实现修复"}]}`

### Fixed
- 修复多 agent 并行调度时所有 agent 接收相同提示词的问题

## [0.5.6] - 2025-07-01

### Fixed
- Fixed OpCode Bug in qq-bot-sdk WebSocket implementation
- Corrected OpCode enum representation to match QQ Bot API specification

## [0.5.5] - Previous Release

### Previous Changes
- See git history for details
