# Codex Experience for hakimi-agent

更新时间: 2026-05-27
适用仓库: `E:/projects/hakimi-agent`
维护原则: 只记录有仓库证据支撑、可被后续 Codex 直接执行的经验。

## 路线

### 1. Hermes parity 切片闭环
- 日期: 2026-05-27
- 适用场景: `hakimi-agent` 继续补齐 Hermes parity，且用户希望单轮交付一个边界清晰的能力切片。
- 推荐步骤:
  1. 先从 `GAP_ANALYSIS.md` 找仍未完成、边界清晰的缺口。
  2. 只做一个垂直切片，不把相邻缺口打包一起推进。
  3. 代码落地后同步更新 `README.md`、`README_CN.md`、`GAP_ANALYSIS.md`，明确“已完成边界”和“剩余缺口”。
  4. 以 GitHub CI 和 tag 触发的 Release 作为验收闭环，不把本地编译结果当完成信号。
- 反例/不适用场景: 需求本身是大范围架构重整、跨多个缺口的 refactor，或用户明确要求本地验证。
- 证据来源:
  - 提交 `51a6db7`、`14b5bec`、`08ab892` 都同时修改了实现代码与 `README.md`、`README_CN.md`、`GAP_ANALYSIS.md`。
  - `GAP_ANALYSIS.md` 当前 cron system 状态明确区分“已落地能力”和“仍缺能力”。
  - `README.md`/`README_CN.md` 近期版本记录连续按 `v0.3.71`、`v0.3.72`、`v0.3.73` 追踪单个切片结果。
- 置信度: 高
- 后续验证方式: 后续 parity 任务若仍能按“单切片 + 文档同步 + CI/Release 验收”稳定闭环，则继续保留。

### 2. CI-only 发布验收顺序
- 日期: 2026-05-27
- 适用场景: `hakimi-agent` 自动化任务涉及功能交付、发布或版本验证，且已有用户约束“不在本地跑 Hakimi 编译/测试”。
- 推荐步骤:
  1. 本地只做静态证据检查与最小改动，不运行 Hakimi 编译/测试。
  2. 推进到远端后先观察 `main` 分支 CI。
  3. 仅在 CI 通过后再打 `v*` tag，让 Release workflow 产出资产。
  4. 回填版本、tag、CI/Release 结果到任务汇报或版本文档。
- 反例/不适用场景: 用户显式要求本地编译，或仓库本身没有远端 CI/Release 工作流。
- 证据来源:
  - `.github/workflows/ci.yml` 将 `push` 到 `main/master` 作为 `Check & Lint` 与 `Test` 触发条件。
  - `.github/workflows/release.yml` 只在 `push` `v*` tag 时执行 Release。
  - `GAP_ANALYSIS.md` 明确写有 `Total tests: 1061 (latest CI target; local compilation intentionally not run in automation)`。
- 置信度: 高
- 后续验证方式: 后续发布任务继续检查是否仍遵循“CI 先于 tag”的顺序，以及是否继续禁止本地编译。

### 3. 流式传输问题优先按 transport 层闭环
- 日期: 2026-05-27
- 适用场景: 症状是“流式回答被截断”“SSE 提前结束”“长连接挂起不返回”，且问题跨 CLI/server/TUI 复现。
- 推荐步骤:
  1. 先检查 provider 是否发出了终止事件或中途关闭。
  2. 把“无终止事件关闭”判定为 transport failure，而不是业务层正常结束。
  3. 超时、重试与共享 HTTP client 统一放在 transport 层，而不是在单个入口重复兜底。
  4. 用集成测试覆盖 continuation/retry 行为，避免只修单一 provider。
- 反例/不适用场景: 问题只发生在单个工具调用或单个业务状态机，不涉及跨入口共享 transport。
- 证据来源:
  - 提交 `08ab892` 新增 `crates/hakimi-transports/src/client.rs`，并修改 `responses.rs`、`loop_impl.rs`、`integration.rs`。
  - `README.md`/`README_CN.md` 的 `v0.3.73` 记录明确将问题归因为 `response.incomplete` 和缺失终止事件。
- 置信度: 中
- 后续验证方式: 后续若出现新的 provider 流问题，优先从 transport 统一策略解决；若只能在业务层修复，则降低此规则权重。

## 行为

### 1. 文档沉淀与业务改动隔离
- 日期: 2026-05-27
- 适用场景: 仓库存在用户未提交改动，但当前任务只要求经验整理、规则归纳或知识更新。
- 稳定规范:
  - 先看 `git status --short --branch`，确认哪些文件已被修改。
  - 已有业务/产品文档处于脏状态时，不把经验直接混入这些文件。
  - 优先新增独立知识载体，例如 `docs/agent-memory/codex-experience.md`。
- 反例/不适用场景: 用户明确要求整理到某个已修改文件，并接受改动混入。
- 证据来源:
  - 2026-05-27 当前工作区 `README.md`、`README_CN.md`、`GAP_ANALYSIS.md`、`.spec-workflow/templates/*` 均已有未提交改动。
  - 本次整理任务的执行边界明确要求“必须保留并避开用户已有未提交改动”。
- 置信度: 高
- 后续验证方式: 之后运行同类自动化时，继续先检查 dirty worktree；若能稳定通过新增独立文档规避冲突，则保留。

### 2. 经验只沉淀“被仓库证据重复证明”的规则
- 日期: 2026-05-27
- 适用场景: 需要把近期任务抽象成路线、行为或技能候选。
- 稳定规范:
  - 至少有两类证据中的一类可复核：提交/文件改动、工作流配置、测试记录、用户明确反馈。
  - 时间敏感事实只作为证据，不把具体 run id、最新版本号、分支落后数写成长期规则。
  - 如果只是一次性的操作细节，不上升为仓库通用经验。
- 反例/不适用场景: 需要记录一次性事故复盘原文时，可单独建事件记录，不应写成默认规则。
- 证据来源:
  - 最近连续提交 `51a6db7`、`14b5bec`、`08ab892` 呈现出重复的“单切片 + 文档同步”模式。
  - `.github/workflows/ci.yml` 与 `release.yml` 提供了比口头约定更稳定的验收依据。
- 置信度: 高
- 后续验证方式: 后续自动化继续要求每条经验至少能回溯到 commit、文件或 workflow。

## 技能候选

### 1. `repo-release-loop`
- 日期: 2026-05-27
- 适用场景: 类似 `hakimi-agent` 这种把远端 CI/Release 作为唯一验收权威的仓库。
- 候选内容:
  - 先查 dirty worktree、当前分支、与远端关系。
  - 从 workflow 文件确认“分支 CI”与“tag Release”的真实触发条件。
  - 任务汇报必须带版本、tag、CI/Release 结果，而不是只说“本地已完成”。
- 反例/不适用场景: 没有 Release workflow 的仓库，或本地测试才是主验收手段的仓库。
- 证据来源:
  - `.github/workflows/ci.yml`
  - `.github/workflows/release.yml`
  - `README.md` 最近版本记录
- 置信度: 中
- 后续验证方式: 需要至少再出现一到两次其他仓库复用成功，才值得抽成跨项目 skill。

### 2. `dirty-worktree-knowledge-update`
- 日期: 2026-05-27
- 适用场景: 自动化只做知识沉淀，但仓库已有用户未提交改动。
- 候选内容:
  - 强制先看 dirty 文件范围。
  - 已改文件只读不写，知识更新落到新文档或专用 memory 文件。
  - 最终汇报中必须明确说明为何没有把规则回写到主文档。
- 反例/不适用场景: 用户授权清理或统一整理现有文档时。
- 证据来源:
  - 本次工作区状态与任务边界约束。
- 置信度: 中
- 后续验证方式: 若该模式在多个仓库都能稳定降低冲突，再考虑升级为通用 skill。
