# Hakimi Agent 智能模型调度系统

## 概述

Hakimi Agent v0.4.x 引入了智能模型调度系统，能够根据任务复杂度自动选择最合适的模型，类似 Claude Code 的分层策略。

## 核心特性

### 三层模型架构

1. **轻量模型 (Light Model)**: 简单查询、文件读取等低复杂度任务
2. **主力模型 (Primary Model)**: 常规开发任务、代码重构等中等复杂度任务  
3. **高级思考模型 (Reasoning Model)**: 系统设计、架构规划等高复杂度任务

### 复杂度评估

系统通过多维度评估任务复杂度（0-10分）：

- **任务类型** (30% 权重): 简单查询(1-2) → 文件操作(3-5) → 代码重构(6-7) → 系统设计(8-10)
- **上下文需求** (20% 权重): 对话历史长度和消息大小
- **推理深度** (25% 权重): 是否需要多步骤规划或深度分析
- **工具调用** (15% 权重): 预测需要的工具调用数量和复杂度
- **嵌套深度修正** (-10% 惩罚): 子 Agent 倾向使用更轻量的模型

### 两阶段执行 (Two-Stage Execution)

对于高复杂度任务（评分 ≥ 8），系统可以启用两阶段执行：

1. **规划阶段**: 高级思考模型（如 DeepSeek-R1）生成详细的执行计划
2. **实施阶段**: 主力模型（如 DeepSeek-V3）根据计划执行具体任务

这种模式结合了推理模型的规划能力和主力模型的执行效率。

## 配置示例

### 基础配置

```yaml
model:
  # 向下兼容：顶层 default/provider 仍然有效
  default: "deepseek-v3"
  provider: "custom:router"
  base_url: "https://router.goldras.edu.kg/v1"
  api_key: "YOUR_API_KEY"
  
  # 新增：三层模型配置
  tiers:
    primary:
      provider: "custom:router"
      model: "deepseek-v3"
      # api_key 和 base_url 可选，默认继承顶层配置
    
    light:
      provider: "custom:router"
      model: "qwen2.5-32b-instruct"
    
    reasoning:
      provider: "custom:router"
      model: "deepseek-r1"
  
  # 自动调度配置
  auto_dispatch:
    enabled: true
    
    # 复杂度阈值 (0-10)
    thresholds:
      light: 3      # ≤ 3 分使用轻量模型
      primary: 7    # ≤ 7 分使用主力模型
      reasoning: 8  # ≥ 8 分使用高级思考模型（两阶段执行）
    
    # 子 Agent 继承调度配置
    inherit_dispatch: true
    
    # 向用户显示调度决策
    show_dispatch_decision: true
    
    # 两阶段执行配置
    two_stage:
      enabled: true
      show_reasoning_to_primary: true  # 将 reasoning 输出传递给 primary
      allow_tools_in_reasoning: false  # reasoning 模型不调用工具，只做规划
```

### 禁用自动调度（单模型模式）

```yaml
model:
  default: "claude-sonnet-4"
  provider: "anthropic"
  
  # 不配置 tiers 或禁用 auto_dispatch
  auto_dispatch:
    enabled: false
```

## 实施状态 (v0.4.1)

✅ **Phase 1.1**: 配置结构实现
- `ModelTiers`, `AutoDispatchConfig`, `DispatchThresholds`
- 集成到 `hakimi-config` crate

✅ **Phase 1.2**: 复杂度分析器
- `ComplexityAnalyzer`: 多维度任务复杂度评估
- `TaskComplexity`, `ComplexityFactor`: 评分和推理结果

✅ **Phase 1.3**: 模型调度器
- `ModelDispatcher`: 模型选择和两阶段执行逻辑
- `build_dispatcher_from_config`: 从配置构建调度器

⏳ **Phase 1.4**: Agent 层集成 (进行中)
- 在 `AIAgent` 中集成调度逻辑
- SSE 流式推送调度决策

⏳ **Phase 2**: 两阶段执行实现
- Reasoning Agent 生成规划
- Primary Agent 执行实施

⏳ **Phase 3**: 递归调度
- 子 Agent 继承调度配置
- Team 协作模式支持

⏳ **Phase 4**: WebUI 可视化
- Settings 界面配置调度策略
- 实时调度决策展示

## 技术细节

### 复杂度评分示例

**简单查询**（评分 1-3，使用轻量模型）：
```
用户: 什么是 Rust?
任务类型: 1/10
上下文需求: 1/10
推理深度: 3/10
工具调用: 2/10
→ 总分: 1.6 → 轻量模型 (qwen2.5-32b-instruct)
```

**代码重构**（评分 5-7，使用主力模型）：
```
用户: 重构 src/agent.rs 的 run_conversation 方法，提取复杂度分析逻辑
任务类型: 6/10
上下文需求: 5/10
推理深度: 6/10
工具调用: 5/10
→ 总分: 5.6 → 主力模型 (deepseek-v3)
```

**系统设计**（评分 ≥ 8，使用高级思考模型 + 主力模型）：
```
用户: 设计一个智能模型调度系统，支持三层模型和递归委派
任务类型: 9/10
上下文需求: 6/10
推理深度: 9/10
工具调用: 7/10
→ 总分: 8.3 → 两阶段执行 (deepseek-r1 规划 + deepseek-v3 实施)
```

### 嵌套深度修正

子 Agent 的复杂度评分会额外减少 1-2 分，避免过度使用高级思考模型：

```
主 Agent: "设计系统" → 评分 9 → Reasoning Model
子 Agent (depth=1): "读取配置文件" → 评分 4 - 1 (深度惩罚) = 3 → Light Model
子 Agent (depth=2): "搜索函数" → 评分 5 - 2 (深度惩罚) = 3 → Light Model
```

## 向下兼容性

- **无 `tiers` 配置**: 系统回退到单模型模式，使用 `model.default`
- **`auto_dispatch.enabled: false`**: 强制单模型模式
- **现有 Agent 代码**: 无需修改，自动检测并应用调度逻辑

## 性能影响

- **复杂度分析**: < 1ms，基于规则和关键词匹配
- **调度决策**: < 1ms，简单阈值比较
- **SSE 推送**: 异步非阻塞，对主流程无影响

## 未来计划

- [ ] 基于历史成功率的动态阈值调整
- [ ] 用户手动覆盖调度决策（`/force-model light`）
- [ ] Persona 级别的模型覆盖配置
- [ ] 调度决策审计日志和统计分析

## 相关文件

- `crates/hakimi-config/src/config.rs`: 配置结构定义
- `crates/hakimi-core/src/model_dispatch.rs`: 核心类型
- `crates/hakimi-core/src/complexity_analyzer.rs`: 复杂度分析器
- `crates/hakimi-core/src/model_dispatcher.rs`: 模型调度器
- `crates/hakimi-core/src/agent.rs`: Agent 集成（待实现）
