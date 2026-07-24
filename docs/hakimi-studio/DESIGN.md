# Hakimi Studio — 系统设计文档

> **产品名：** Hakimi Studio  
> **定位：** 以 Hakimi Agent 为统一运行时的跨端 AI 开发工作台  
> **原则：** 先完整成熟架构，再分阶段交付；桌面/服务器是执行真相源，UI 只是壳  
> **版本：** Draft v0.2 · 2026-07-23  
> **状态：** 决策已锁定 · Phase 0 完成（协议 + Runtime + `/v1/studio` WS）· 下一阶段 Phase 1 Workspace IDE  

### 已锁定产品决策（2026-07-23）

| 项 | 决策 |
|----|------|
| **品牌名** | **Hakimi Studio** |
| **默认执行** | **本机优先**，运行时可切换 Active Runner（本机 Desktop Worker ↔ 公网服务器 Worker） |
| **Hub** | 部署在**公网服务器**（self-host on public host；非 SaaS 多租户一期） |
| **双端同时输入** | **队列 + 可抢占**（后到消息可入队；用户可抢占当前 run） |
| **主界面** | **Workspace IDE 为主**，Office View 可切换 |

---

## 1. 产品定义

### 1.1 一句话

**Hakimi Studio = VSCode 式工作区 + 现代 Coding Agent 运行时 + 多端会话接力 + 服务器托管协同。**

### 1.2 目标用户

| 角色 | 场景 |
|------|------|
| 个人开发者 | 本机开项目，Agent 写代码、跑测试、部署 |
| 远程开发者 | 服务器上跑 Agent，浏览器/手机接着看、接着改 |
| 小团队 | 同一任务在不同终端接力，子 Agent 自动组队并行 |
| 运维/部署 | 内置 SSH、定时任务、MCP/Skills 扩展 |

### 1.3 核心价值主张

1. **一处执行，处处可见**：A 终端发起的任务，B 终端可实时观看并接管。
2. **真工作区**：不是纯聊天框，而是文件树 + 编辑器 + 终端 + Agent 同屏。
3. **Hakimi 内核统一**：CLI / TUI / Desktop / WebUI / Mobile 共用 `hakimi-core`，不搞第二套 Agent 循环。
4. **本地优先 + 可选托管**：本机可离线完整工作；需要多端时连 Hosted Hub。
5. **现代 Agent 能力齐备**：多 Provider、Skills、MCP、SubAgent 组队、Cron、SSH、插件。

### 1.4 非目标（YAGNI 边界）

- 不做完整 VSCode 插件市场替代品。
- 不做「云端代写密钥」——Provider Key 默认留在执行节点。
- Gateway（Telegram/微信等）与 Studio 协同，但 Studio 不重写消息网关。
- 第一阶段不追求 iOS/Android 原生 App Store 完美体验，先以 **PWA + 远程 WebUI** 覆盖。

---

## 2. 竞品/参考拆解：学什么、不学什么

### 2.1 earendil-works/pi（极简 Harness）

| 优点 | 对 Hakimi Studio 的吸收 |
|------|-------------------------|
| 核心极薄，能力靠 Extension/Skill | Studio 壳与 Core 解耦；功能可插件化 |
| 事件驱动 Agent Loop（agent/turn/message/tool） | 统一事件协议 `StudioEvent` |
| 会话树 / fork / compact | 会话分支 + 无损压缩历史 |
| Skills 标准（Agent Skills） | 与现有 `hakimi-skills` 对齐并做 GUI 管理 |
| **不内置** MCP/子代理/权限弹窗（靠扩展） | Core 可保持窄；Studio 提供「官方扩展包」默认装齐 |

**不学：** 完全放弃内置能力导致上手成本过高。Hakimi 已有完整 toolset，应保留开箱即用。

### 2.2 Mouseww/grok-build（Grok Build / 工业级 Coding Agent）

| 优点 | 吸收 |
|------|------|
| Workspace 抽象（FS/VCS/checkpoint/rewind） | `hakimi-workspace` crate：工作区、快照、回滚 |
| ACP（Agent Client Protocol）接入 IDE | 后续 IDE 插件兼容层 |
| Subagent 类型 + Persona 分层 | 强化 `delegate_task` / team / persona |
| 完整 MCP CLI（stdio/http/sse） | 对齐并做 GUI Hub |
| Background tasks / monitor / loop | 与 `process` + cron 统一任务中心 |
| Session 目录结构清晰（jsonl + plan + rewind） | 会话存储规范化，支持 resume/fork |
| Sandbox / permissions | 权限策略可配置，容器可选 |

**不学：** 超大 monorepo 与「生成式 workspace Cargo.toml」运维复杂度；Hakimi 保持手写清晰 crate 边界。

### 2.3 Stack-Cairn/LiveAgent（桌面 + Gateway + WebUI）

| 优点 | 吸收 |
|------|------|
| **Local-first**：工具/密钥在桌面真相源 | 执行节点（Desktop 或 Server Worker）是真相源 |
| **Gateway 只做中继**，不跑工具、不存真实 Key | Hosted Hub 同样原则 |
| WebSocket + 有界 seq 窗口断线恢复 | 会话事件流 `seq` / `stream_epoch` |
| 多 Agent 设备注册与凭证 | 多设备 `device_id` + token |
| Skills Hub / MCP Hub UI | Studio 设置中心复刻形态 |
| Subagent worktree 隔离 + merge | 并行子代理默认 git worktree 策略 |
| 桌面 Tauri 2 + React + 可选远程 WebUI | Desktop = Tauri；Web/Mobile = 同一前端包 |

**不学：**
- **Agent 主循环放在 TypeScript 前端**（LiveAgent 用 pi-ai 在 GUI 侧跑 loop）——Hakimi 必须坚持 **Rust `hakimi-core` 单引擎**。
- **Go Gateway**——用户偏好纯 Rust；Hub 用 `hakimi-hub`（Axum）实现。

### 2.4 对照结论（产品定位图）

```
Pi          = 极简可扩展 harness
Grok Build  = 工业级 coding agent + workspace
LiveAgent   = 本地优先桌面 + 远程中继
Hakimi      = 已有完整 Rust Agent 内核（CLI/TUI/Gateway/WebUI）
Hakimi Studio = Hakimi 内核 + LiveAgent 形态 + Grok 工作区深度 + Pi 扩展哲学
```

---

## 3. 系统架构

### 3.1 总览（分层）

```
┌──────────────────────────────────────────────────────────────────┐
│  Clients (同构 UI Shell)                                          │
│  ┌────────────┐ ┌────────────┐ ┌────────────┐ ┌────────────────┐ │
│  │ Desktop    │ │ WebUI      │ │ Mobile PWA │ │ CLI / TUI      │ │
│  │ Tauri 2    │ │ Browser    │ │ (Phase 2+) │ │ (已有)         │ │
│  └─────┬──────┘ └─────┬──────┘ └─────┬──────┘ └───────┬────────┘ │
│        │              │              │                │          │
│        └──────────────┴──────┬───────┴────────────────┘          │
│                              │ Studio Protocol (WS/HTTP/JSON)     │
└──────────────────────────────┼───────────────────────────────────┘
                               ▼
┌──────────────────────────────────────────────────────────────────┐
│  Hakimi Hub (可选托管 / 自建)  ·  crates/hakimi-hub                 │
│  会话目录 · 事件扇出 · 设备注册 · 断线 replay · 静态 WebUI          │
│  ⚠️ 不执行工具 · 不保存 Provider 明文 Key（除非用户显式选云执行）    │
└──────────────────────────────┬───────────────────────────────────┘
                               │ attach / command / event stream
                               ▼
┌──────────────────────────────────────────────────────────────────┐
│  Execution Node（执行真相源，二选一或并存）                         │
│  ┌─────────────────────────────┐  ┌────────────────────────────┐ │
│  │ Desktop Worker (Tauri侧)    │  │ Server Worker (Docker/     │ │
│  │ 本机工作区 + 本机密钥        │  │  systemd 上的 hakimi)      │ │
│  └──────────────┬──────────────┘  └─────────────┬──────────────┘ │
│                 └──────────────┬────────────────┘                │
│                                ▼                                  │
│  ┌────────────────────────────────────────────────────────────┐  │
│  │  hakimi-core  Agent Loop  (唯一大脑)                        │  │
│  │  session · context · tools · skills · mcp · cron · team    │  │
│  └────────────────────────────────────────────────────────────┘  │
│  ┌──────────────┐ ┌──────────────┐ ┌──────────────┐               │
│  │ workspace    │ │ providers    │ │ ssh / deploy │               │
│  │ FS·VCS·snap  │ │ multi-model  │ │ 内置 tool    │               │
│  └──────────────┘ └──────────────┘ └──────────────┘               │
└──────────────────────────────────────────────────────────────────┘
```

### 3.2 两种部署模式

| 模式 | 说明 | 适用 |
|------|------|------|
| **Local Solo** | 桌面 App 内嵌 Worker，不连 Hub | 日常本机开发 |
| **Hosted Relay** | 本机/服务器 Worker 连 Hub；浏览器/手机/另一台电脑 attach | 多端接力、远程办公 |
| **Cloud Worker**（可选 Phase 3） | Worker 跑在用户的 VPS/容器；密钥仍在该 Worker | 无本地强机、纯网页使用 |

**多端接力语义（核心需求）：**

```
Device A (Desktop)                Hub                     Device B (Phone/Web)
     |  create session S1          |                            |
     |  tool_call / stream  ------>|  fan-out event(seq) ------>|  实时观看
     |                             |                            |
     |  (用户离开)                  |  B: session.attach        |
     |                             |  B: chat.submit ----------->|  接管输入
     |  <--- command (若A仍是执行节点) |                            |
     |  或切换到 Server Worker 执行  |                            |
```

**关键不变量：**
1. 任一时刻一个 Session 只有一个 **Active Runner**（执行节点）。
2. 多个 **Viewer/Controller** 可订阅同一 Session 事件流。
3. 切换 Runner 必须显式 handoff（或原 Runner 离线后的自动接管策略）。

### 3.3 进程与 crate 规划

| 组件 | Crate / 包 | 技术 | 职责 |
|------|------------|------|------|
| Agent 内核 | 现有 `hakimi-core` 等 | Rust | 不变：循环、工具、委派 |
| Workspace | **新建** `hakimi-workspace` | Rust | 工作区根、文件树、checkpoint、git worktree |
| Studio API | **新建** `hakimi-studio-api` | Rust | 统一 JSON/WS 协议、会话 attach、事件总线 |
| Hub | **新建** `hakimi-hub` | Axum + WS | 托管中继、设备目录、静态 WebUI |
| Desktop Shell | **新建** `hakimi-desktop` | Tauri 2 + React | 桌面 GUI |
| Studio Web | **演进** `hakimi-webui` / 新 `studio-web` | React + Vite | 与桌面共享组件（mirror 策略） |
| SSH | **新建** tool in `hakimi-tools` | Rust `russh` 或 ssh2 | 远程 shell/SFTP/部署 |
| 现有 | gateway, cron, mcp, skills, plugin… | 保持 | 嵌入 Studio |

**禁止：** 在 React 里再实现一套 agent loop。前端只发 command、渲染 event。

---

## 4. 协议设计：Studio Protocol

### 4.1 传输

| 通道 | 用途 |
|------|------|
| `WS /v1/studio` | 主控制面 + 事件流（JSON 文本帧，Phase 1；可后期升 Protobuf） |
| `HTTP /v1/*` | 登录、设备注册、文件上传、健康检查、OpenAPI |
| `WS /v1/terminal` | 终端字节流（独立通道，防队头阻塞） |

### 4.2 核心消息类型（示意）

```jsonc
// Client → Runner/Hub
{ "type": "session.create", "workspace_id": "...", "title": "..." }
{ "type": "session.attach", "session_id": "...", "after_seq": 120 }
{ "type": "chat.submit", "session_id": "...", "text": "...", "client_request_id": "..." }
{ "type": "chat.cancel", "session_id": "...", "run_id": "..." }
{ "type": "workspace.list", "path": "." }
{ "type": "workspace.read", "path": "src/main.rs" }
{ "type": "workspace.write", "path": "...", "content": "..." }  // 人工编辑也可经此同步
{ "type": "runner.handoff", "session_id": "...", "to_device_id": "..." }

// Runner → Clients (带单调 seq)
{ "seq": 121, "type": "message.delta", "session_id": "...", "delta": "..." }
{ "seq": 122, "type": "tool.started", "name": "terminal", "call_id": "..." }
{ "seq": 123, "type": "tool.completed", "call_id": "...", "ok": true }
{ "seq": 124, "type": "agent.progress", "todos": [...] }
{ "seq": 125, "type": "subagent.spawned", "child_id": "...", "goal": "..." }
{ "seq": 126, "type": "session.ended", "reason": "done" }
```

### 4.3 断线恢复（学 LiveAgent）

- 每个 session 维护有界事件窗口（默认 10 分钟 / 4096 条 / 8MB）。
- 客户端重连带 `after_seq`；缺口过大则 `reset` + 从持久化 history snapshot 重建。
- `client_request_id` 幂等，防重复提交。

### 4.4 与现有 Gateway 关系

| 系统 | 角色 |
|------|------|
| `hakimi-gateway`（Telegram 等） | 消息平台入站 → 仍走现有路径写 session |
| `hakimi-hub` | Studio 多端 UI 协同 |
| 两者共享 | `hakimi-session` 存储；可互相发现同一 session（可选「在 Telegram 开的任务出现在 Studio」） |

---

## 5. 功能架构

### 5.1 工作区浏览器（VSCode-like）

**UI 布局：**

```
┌────────┬─────────────────────────────┬──────────────────┐
│ 文件树  │  编辑器 Tabs (Monaco)        │  Agent Chat      │
│ + Git  │  Diff / Preview             │  工具轨迹         │
│        ├─────────────────────────────┤  SubAgent 面板    │
│        │  集成终端 / 进程面板          │  Todos           │
└────────┴─────────────────────────────┴──────────────────┘
        底栏：状态 / Runner 设备 / Provider / Token 用量
```

**后端能力（`hakimi-workspace`）：**

| API | 说明 |
|-----|------|
| open/list/read/write/edit/delete | 工作区安全边界内 |
| glob/grep | 对齐现有 search tools |
| git status/diff/commit/worktree | 子代理隔离基础 |
| checkpoint / rewind | 会话级文件快照（学 Grok） |
| watch | FS 事件 → UI 刷新 |

**安全：** 默认 chroot 在 workspace root；越界路径拒绝。

### 5.2 多 Provider

扩展现有 `hakimi-transports` + config：

| 能力 | 说明 |
|------|------|
| Provider 配置中心 | OpenAI / Anthropic / Google / 兼容 Base URL / 国产模型 |
| 每会话 / 每 Agent 模型覆盖 | 子代理可用更便宜模型 |
| 模型路由 | 沿用 `model_dispatcher` 复杂度路由 |
| Key 存储 | 本机 keyring / 加密文件；Hub 侧仅脱敏展示 |
| 健康探测 | `/models` 探测 + 延迟统计 |

### 5.3 子 Agent / 组队 / 并行

基于现有：`delegate_task`、`dispatched_agent`、`team`、`persona`。

| 能力 | 设计 |
|------|------|
| 手动委派 | 用户或主 Agent 调 `delegate_task` |
| 自动构建 SubAgent | Planner 根据任务图 spawn（角色：explore / implement / review / test） |
| 自动组队 | `team` 工具：定义角色、共享 bus、聚合结果 |
| 并行 | 现有 max_concurrent；默认 worktree 隔离 |
| 通信 | Message bus + 父级 summary；UI 显示 Agent 树与状态 |
| 可视化 | Office View 已有基础 → Studio 侧栏升级为 **Agent Fleet** |

**Agent 类型模板（学 Grok）：**

| type | 工具策略 | 用途 |
|------|----------|------|
| general | 全量 | 默认 |
| explore | 只读 + bash | 调研 |
| implement | 读写 + bash | 实现 |
| review | 只读 | 审查 |
| deploy | ssh + 受限写 | 部署 |

### 5.4 Skills & MCP

| 模块 | 现状 | Studio 增强 |
|------|------|-------------|
| Skills | `hakimi-skills` + skill_manage | Skills Hub UI：安装/创建/启用/预览 |
| MCP | `hakimi-mcp` | MCP Hub：stdio/http/sse、测试连接、动态 tool 列表 |
| 插件 | WASM + 动态库 | 高级扩展；默认不暴露给小白 |

### 5.5 内置开发 Tools（Coding Agent 全套）

保留并对齐「现代 coding agent」最小闭包：

| 类别 | Tools |
|------|-------|
| 文件 | read_file, write_file, patch/edit, search_files, delete |
| Shell | terminal, process（bg/poll/kill） |
| 代码智能 | codebase graph（可选后期）, git, checkpoint |
| Web | web_search, web_extract, browser |
| 协作 | todo, delegate_task, team, send_message, clarify |
| 记忆 | memory, session_search |
| 自动化 | cronjob |
| 部署 | **ssh_exec, ssh_sftp, deploy_***（新建） |
| 媒体 | vision, tts 等（可选） |

### 5.6 SSH 连接工具

```
ssh_connect(host, user, port, auth)
ssh_exec(session_id, command, timeout)
ssh_sftp_{list,get,put}
ssh_tunnel (可选)
```

- 凭据：本机安全存储，不进 Hub 明文。
- UI：连接管理器 + 远程文件树挂载（只读优先）。
- 部署 skill：标准「构建 → 上传 → 重启 → 健康检查」流程。

### 5.7 定时任务

复用 `hakimi-cron`：

- UI 管理 crontab / interval / one-shot
- 类型：prompt / shell / http
- 执行日志与失败告警
- 与 Studio 会话联动（结果可推送到绑定 session）

### 5.8 现代 Agent 应具备的其他能力（清单）

- [x] 流式输出（chunk-by-chunk，Hakimi 已有方向）
- [x] 上下文压缩 / session_search
- [x] 记忆系统
- [ ] Plan mode（可文件化 + 可选强制只读）
- [ ] 权限分级（read-only / standard / full）
- [ ] 沙箱（Docker/microVM 可选）
- [ ] 使用量与成本面板
- [ ] Hooks（pre/post tool）
- [ ] 会话分享只读链接
- [ ] ACP/IDE 桥（后期）

---

## 6. 数据模型（核心表）

### 6.1 执行节点本地（SQLite，扩展现有）

```sql
-- 设备
CREATE TABLE devices (
  device_id TEXT PRIMARY KEY,
  name TEXT,
  kind TEXT, -- desktop|server|cli
  created_at TEXT,
  last_seen_at TEXT
);

-- 工作区
CREATE TABLE workspaces (
  workspace_id TEXT PRIMARY KEY,
  root_path TEXT NOT NULL,
  name TEXT,
  created_at TEXT
);

-- 会话（可与现有 sessions 对齐/迁移）
CREATE TABLE studio_sessions (
  session_id TEXT PRIMARY KEY,
  workspace_id TEXT,
  title TEXT,
  active_runner_device_id TEXT,
  status TEXT, -- idle|running|error|archived
  created_at TEXT,
  updated_at TEXT
);

-- 事件 log（本地完整；Hub 仅有界窗口）
CREATE TABLE session_events (
  session_id TEXT,
  seq INTEGER,
  event_type TEXT,
  payload_json TEXT,
  created_at TEXT,
  PRIMARY KEY (session_id, seq)
);

-- Provider 配置（密钥加密）
CREATE TABLE providers (
  id TEXT PRIMARY KEY,
  kind TEXT,
  base_url TEXT,
  model_defaults_json TEXT,
  secret_ref TEXT, -- keyring 引用，非明文
  enabled INTEGER
);

-- SSH profiles
CREATE TABLE ssh_profiles (
  id TEXT PRIMARY KEY,
  host TEXT, port INTEGER, user TEXT,
  auth_kind TEXT, -- key|password|agent
  secret_ref TEXT
);
```

### 6.2 Hub 侧

```sql
CREATE TABLE hub_devices (
  device_id TEXT PRIMARY KEY,
  token_hash TEXT,
  name TEXT,
  online INTEGER,
  last_seen_at TEXT
);

CREATE TABLE hub_session_index (
  session_id TEXT PRIMARY KEY,
  owner_device_id TEXT,
  active_runner_device_id TEXT,
  title TEXT,
  updated_at TEXT
);

-- 事件仅内存窗口；可选短持久化用于运维
```

---

## 7. UI/UX 设计要点

对齐用户已有偏好（Office View 审美、反感冗余空行、真流式、工具进度可见）：

| 原则 | 落地 |
|------|------|
| 高级感 | 深色默认、细边框、16:9 信息密度、层级阴影 |
| 真流式 | 事件级 delta，禁止缓冲假流式 |
| 工具可见 | ⚙️ + 时间戳进度；可折叠详情 |
| 模态配置 | Provider/MCP/Skills 用模态，不整页跳转丢上下文 |
| 多端状态 | 顶栏常驻：Runner 在线设备、当前 session、延迟 |
| 移动端 | Chat 优先 + 文件只读浏览；编辑/终端降级 |

### 7.1 关键页面

1. **Home** — 最近会话、工作区、在线设备  
2. **Workspace** — 文件树 + 编辑器 + Chat  
3. **Agent Fleet** — 子代理/组队可视化  
4. **Settings** — Providers / MCP / Skills / SSH / Cron / Remote Hub  
5. **Activity** — 全局任务与 cron 日志  

---

## 8. 技术栈选型

| 层 | 选型 | 理由 |
|----|------|------|
| Agent 内核 | 现有 Rust crates | 用户强制 Rust；已有 40k+ 行资产 |
| Hub | Rust Axum + tokio-tungstenite | 与 server 统一，无 Go 第二语言 |
| Desktop | Tauri 2 | 比 Electron 更轻；LiveAgent 验证过；Rust 侧可直接链 hakimi |
| 前端 | React 19 + Vite + Tailwind + Monaco | 工作区编辑器必备 Monaco |
| 状态 | Zustand / 轻量 store | 与现有 webui 可渐进迁移 |
| 终端前端 | xterm.js | 标准 |
| 移动 | Phase1: 响应式 WebUI/PWA；Phase2: Tauri Mobile 或 Capacitor 壳 | 降低首版风险 |
| 打包 | GitHub Actions → DMG/MSI/AppImage + Docker Hub 镜像 | 对齐 LiveAgent 发布体验 |
| DB | SQLite (rusqlite) | 已有会话体系 |

---

## 9. 安全模型

| 威胁 | 对策 |
|------|------|
| Hub 被攻破 | Hub 无工具执行权；无明文 API Key；token 仅哈希 |
| 远程指令滥用 | 设备配对 + 可选二次确认敏感工具（写文件/ssh/删除） |
| 路径穿越 | workspace root jail |
| 供应链 | 与现有 pin 依赖策略一致；扩展包签名校验（后期） |
| 多租户 | Phase1 单用户 Hub；Phase2 加 account/tenant |

权限档位：

| Profile | 能力 |
|---------|------|
| read_only | 读文件/搜索 |
| standard | 读写工作区 + 受限 shell |
| deploy | + SSH |
| full | 无限制（仅本机确认） |

---

## 10. 与现有 Hakimi 的映射（复用优先）

| 已有 | Studio 用法 |
|------|-------------|
| `hakimi-core` loop / stream | Runner 唯一引擎 |
| `hakimi-session` | 会话持久化与 search |
| `hakimi-context` | SmartContext |
| `hakimi-tools` | 工具全集 |
| `builtin_delegate_task` / team / persona | 多 Agent |
| `hakimi-mcp` / skills / cron / plugin | 设置中心绑定 |
| `hakimi-server` + webui | 演进为 Studio API + UI |
| Office View | 可保留为「趣味视图」；主路径为 Workspace IDE 布局 |
| `hakimi-gateway` | 继续消息平台；可把活动推到 Studio session |

**新建工作量集中在：** Desktop 壳、Hub 中继、Workspace crate、SSH tool、统一协议、前端 IDE 布局。

---

## 11. 路线图

### Phase 0 — 协议与骨架（2 周）

- 定义 Studio Protocol 与事件 schema
- `hakimi-studio-api` 本地进程内跑通：create/attach/submit/stream
- 最小 React「Chat + 事件日志」连本地 API

### Phase 1 — Workspace IDE MVP（4–6 周）

- 文件树 + Monaco + Chat 三栏
- 多 Provider 设置页
- 本机 Runner 完整 tool 循环
- 会话列表 resume
- SSH tool v1（exec + key auth）

### Phase 2 — 多端接力（4 周）

- `hakimi-hub` Docker 部署
- Desktop/Web 设备注册
- 事件 fan-out + after_seq 恢复
- **A 终端任务 B 终端可见并可接着做**

### Phase 3 — Agent Fleet 与生态（4 周）

- SubAgent 自动组队 UI
- Skills Hub / MCP Hub
- Cron UI
- worktree 隔离默认开启
- 使用量面板

### Phase 4 — 桌面安装包 + 移动 PWA（3–4 周）

- Tauri 打包 Windows/macOS（Linux 可选）
- PWA 安装体验
- 权限档位与可选沙箱
- 文档与演示视频

### Phase 5 — 深化（持续）

- ACP / IDE 插件
- iOS/Android 原生壳
- 云 Worker 一键
- Checkpoint/Rewind 完整
- 多用户团队租户

---

## 12. 成功标准（验收）

| ID | 标准 |
|----|------|
| S1 | 本机打开工作区，Agent 能改代码、跑测试、展示真流式与工具进度 |
| S2 | 配置 ≥3 个 Provider 并可切换 |
| S3 | 主 Agent 自动 spawn ≥2 子 Agent 并行，结果汇总 |
| S4 | Skills 与 MCP 可在 GUI 增删启停 |
| S5 | SSH 到服务器完成一次部署健康检查 |
| S6 | Cron 任务到点执行并在 UI 可见日志 |
| S7 | **服务器 Hub：设备 A 开任务，设备 B 实时看到流并继续对话** |
| S8 | Windows + macOS 安装包可启动；WebUI 可远程使用 |

---

## 13. 风险与缓解

| 风险 | 缓解 |
|------|------|
| Tauri 移动不成熟 | 移动先走 PWA |
| 双端 UI 分叉 | mirror 清单 + CI 字节一致（学 LiveAgent） |
| 事件协议过早 Protobuf | Phase1 JSON；稳定后再切 |
| 会话切换 Runner 状态撕裂 | 显式 handoff + 单 Active Runner |
| 磁盘/依赖膨胀 | 前端分包；Rust workspace feature 门控 |
| 与现有 WebUI 冲突 | 新路由 `/studio` 渐进替换，不硬砍 Office View |

---

## 14. 产品决策（已锁定 · 2026-07-23）

| 项 | 最终决策 | 实现含义 |
|----|----------|----------|
| 品牌名 | **Hakimi Studio** | 文档/安装包/UI 统一命名 |
| 默认执行 | **本机优先**，可切换执行位置 | `runner.prefer=local`；支持 `runner.handoff` 到 public Server Worker |
| Hub | **公网服务器 self-host** | 一期非多租户 SaaS；用户自己的公网机部署 `hakimi-hub` |
| 双端输入 | **队列 + 可抢占** | 后到 `chat.submit` 入队；`chat.preempt` / 高优先级可打断当前 run |
| 主界面 | **Workspace 为主，Office 可切换** | 默认路由 Workspace IDE；Office View 为可选模式 |

---

## 15. 参考链接

- https://github.com/earendil-works/pi  
- https://github.com/Mouseww/grok-build  
- https://github.com/Stack-Cairn/LiveAgent  
- 本仓库 `docs/ARCHITECTURE.md`  
- pi 哲学：https://mariozechner.at/posts/2025-11-30-pi-coding-agent/

---

**下一步：** 按 `docs/hakimi-studio/IMPLEMENTATION_PLAN.md` 实施 Phase 0（Studio Protocol + `hakimi-studio-api` + 本地 WS）。
