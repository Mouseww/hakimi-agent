# 三项目优点对照速查

## 1. Pi (earendil-works/pi)

**本质：** 极简 Agent Harness + Coding CLI  
**栈：** TypeScript monorepo（pi-ai / agent-core / coding-agent / tui）

**核心优点：**
- 事件驱动 loop，边界清晰
- 扩展系统（Skills / Extensions / Packages）而不是无限堆核心
- 会话树、fork、compact
- 默认工具极少（read/write/edit/bash），可替换

**Hakimi 该抄：** StudioEvent 阶段划分；扩展点；会话分支思维  
**Hakimi 不该抄：** 放弃开箱即用工具

## 2. Grok Build (Mouseww/grok-build)

**本质：** 工业级终端 Coding Agent  
**栈：** 大型 Rust monorepo

**核心优点：**
- Workspace 抽象 + checkpoint/rewind
- Subagent 类型 + Persona
- MCP 完整管理体验
- Background / monitor / loop 任务模型
- ACP 对接 IDE
- Sandbox 与权限文档化

**Hakimi 该抄：** workspace crate；agent 类型模板；任务中心；rewind  
**Hakimi 不该抄：** 过重的 monorepo 生成式工程

## 3. LiveAgent (Stack-Cairn/LiveAgent)

**本质：** Local-first 桌面 Agent + 远程 Gateway + WebUI  
**栈：** Tauri2 + React + Go Gateway + Protobuf WS

**核心优点：**
- 桌面是工具/密钥真相源
- Gateway 纯中继 + 有界 seq 恢复
- 多设备 agent_id 凭证
- Skills/MCP Hub UI
- Subagent worktree
- 桌面与 WebUI mirror 策略

**Hakimi 该抄：** 中继模型、多端接力、Hub 边界、UI 形态  
**Hakimi 不该抄：** 前端跑 Agent Loop；Go 第二语言（改用 Rust Hub）

## 4. Hakimi 现有资产

已具备：core loop、session、context、tools（含 delegate/team/cron/memory…）、mcp、skills、plugin、gateway、webui/office、server。

**缺口相对 Studio 目标：**
- 真·工作区 IDE 壳（文件树+Monaco）
- 多端会话接力 Hub
- Desktop 安装包
- SSH 一等公民 tool
- 统一 Studio 协议与 Active Runner 模型
