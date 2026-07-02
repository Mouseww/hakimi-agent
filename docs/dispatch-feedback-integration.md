# 智能调度反馈系统 - 跨平台集成指南

智能模型调度系统支持用户通过多种方式提供反馈，帮助系统学习和优化调度策略。

## 📋 反馈类型

| 反馈类型 | 含义 | Emoji | 命令 | 回调数据 |
|---------|------|-------|------|---------|
| `JustRight` | 模型选择恰当 | 👍 | `/justright`, `/刚好` | `dispatch_justright` |
| `TooHeavy` | 模型过于强大（浪费） | 👎 | `/lighter`, `/轻量` | `dispatch_lighter` |
| `TooLight` | 模型能力不足 | 💪 | `/stronger`, `/增强` | `dispatch_stronger` |

## 🔌 平台集成方案

### 1. Telegram 集成

#### 方案 A: Inline Keyboard（推荐）

在每次调度决策后附加内联按钮：

```rust
use hakimi_core::model_dispatch::UserFeedback;

// 发送调度决策消息 + 反馈按钮
let keyboard_json = UserFeedback::telegram_inline_buttons();
// 返回: {"inline_keyboard":[[...三个按钮...]]}

// 使用 teloxide 发送
use teloxide::types::{InlineKeyboardMarkup, InlineKeyboardButton};
let keyboard = InlineKeyboardMarkup::new(vec![vec![
    InlineKeyboardButton::callback("👍 刚刚好", "dispatch_justright"),
    InlineKeyboardButton::callback("👎 太重了", "dispatch_lighter"),
    InlineKeyboardButton::callback("💪 太弱了", "dispatch_stronger"),
]]);

bot.send_message(chat_id, "🚀 使用主力模型 (复杂度: 5.2/10)")
    .reply_markup(keyboard)
    .await?;
```

处理回调查询：

```rust
use teloxide::prelude::*;
use hakimi_core::model_dispatch::UserFeedback;

async fn handle_callback_query(bot: Bot, q: CallbackQuery) -> ResponseResult<()> {
    if let Some(data) = &q.data {
        if let Some(feedback) = UserFeedback::from_callback(data) {
            // 应用反馈到学习引擎
            learner.apply_feedback(feedback);
            
            // 答复用户
            bot.answer_callback_query(q.id)
                .text(feedback.message())
                .await?;
        }
    }
    Ok(())
}
```

#### 方案 B: 斜杠命令（备用）

用户直接发送命令：

```text
/justright  # 模型选择正确
/lighter    # 下次用轻量模型
/stronger   # 下次用更强模型
```

在消息处理器中识别：

```rust
if let Some(feedback) = UserFeedback::from_command(&message.text) {
    learner.apply_feedback(feedback);
    bot.send_message(chat_id, feedback.message()).await?;
}
```

---

### 2. Discord 集成

使用 Discord Message Components（按钮行）：

```rust
use hakimi_core::model_dispatch::UserFeedback;
use serenity::builder::CreateActionRow;
use serenity::all::CreateButton;

// 创建按钮行
let action_row = CreateActionRow::Buttons(vec![
    CreateButton::new("dispatch_justright")
        .label("👍 刚刚好")
        .style(serenity::all::ButtonStyle::Success),
    CreateButton::new("dispatch_lighter")
        .label("👎 太重了")
        .style(serenity::all::ButtonStyle::Danger),
    CreateButton::new("dispatch_stronger")
        .label("💪 太弱了")
        .style(serenity::all::ButtonStyle::Primary),
]);

// 发送消息
ctx.send(|m| {
    m.content("🚀 使用主力模型 (复杂度: 5.2/10)")
     .components(|c| c.add_action_row(action_row))
}).await?;
```

处理按钮交互：

```rust
#[poise::command(slash_command)]
async fn handle_component_interaction(
    ctx: Context<'_>,
    interaction: ComponentInteraction,
) -> Result<(), Error> {
    if let Some(feedback) = UserFeedback::from_callback(&interaction.data.custom_id) {
        learner.apply_feedback(feedback);
        
        interaction.create_response(ctx, |r| {
            r.kind(InteractionResponseType::ChannelMessageWithSource)
             .interaction_response_data(|d| d.content(feedback.message()))
        }).await?;
    }
    Ok(())
}
```

---

### 3. WebUI 集成

在 React/Svelte 前端显示按钮：

```typescript
import { useState } from 'react';

interface DispatchDecisionProps {
  tier: 'light' | 'primary' | 'reasoning';
  complexity: number;
  onFeedback: (feedback: string) => void;
}

function DispatchDecision({ tier, complexity, onFeedback }: DispatchDecisionProps) {
  const tierEmoji = { light: '💡', primary: '🚀', reasoning: '🧠' };
  
  return (
    <div className="dispatch-feedback">
      <p>
        {tierEmoji[tier]} 使用 {tier} 模型 (复杂度: {complexity}/10)
      </p>
      <div className="feedback-buttons">
        <button onClick={() => onFeedback('dispatch_justright')}>
          👍 刚刚好
        </button>
        <button onClick={() => onFeedback('dispatch_lighter')}>
          👎 太重了
        </button>
        <button onClick={() => onFeedback('dispatch_stronger')}>
          💪 太弱了
        </button>
      </div>
    </div>
  );
}
```

后端 API 端点：

```rust
#[post("/api/dispatch/feedback")]
async fn apply_feedback(
    learner: Data<Arc<Mutex<DispatchLearner>>>,
    feedback_data: Json<FeedbackRequest>,
) -> Result<HttpResponse, Error> {
    if let Some(feedback) = UserFeedback::from_callback(&feedback_data.type_) {
        learner.lock().unwrap().apply_feedback(feedback);
        Ok(HttpResponse::Ok().json(json!({
            "success": true,
            "message": feedback.message()
        })))
    } else {
        Ok(HttpResponse::BadRequest().json(json!({
            "success": false,
            "error": "Invalid feedback type"
        })))
    }
}
```

---

### 4. 命令行（CLI）集成

在 CLI 模式下，调度决策显示后提示用户反馈：

```rust
println!("🚀 使用主力模型 (复杂度: 5.2/10)");
println!();
println!("如何评价此次调度决策？");
println!("  [1] 👍 刚刚好");
println!("  [2] 👎 太重了（下次用轻量模型）");
println!("  [3] 💪 太弱了（下次用更强模型）");
println!("  [Enter] 跳过");

let mut input = String::new();
std::io::stdin().read_line(&mut input)?;

let feedback = match input.trim() {
    "1" => Some(UserFeedback::JustRight),
    "2" => Some(UserFeedback::TooHeavy),
    "3" => Some(UserFeedback::TooLight),
    _ => None,
};

if let Some(fb) = feedback {
    learner.apply_feedback(fb);
    println!("{}", fb.message());
}
```

---

## 🎯 最佳实践

### 1. 反馈时机

**显示反馈选项的时机：**
- ✅ 任务成功完成后
- ✅ 用户对结果表示满意/不满意时
- ❌ 任务失败时（自动升级已触发）
- ❌ 用户主动要求特定模型时（`@light` / `@pro` 前缀）

### 2. 反馈超时

Inline 按钮应设置超时时间，避免历史消息按钮干扰新调度：

```rust
// Telegram: 在发送按钮时记录消息ID和时间戳
struct FeedbackContext {
    message_id: i64,
    timestamp: DateTime<Utc>,
    dispatch_record_idx: usize,
}

// 处理回调时验证：
if Utc::now() - context.timestamp > Duration::minutes(10) {
    bot.answer_callback_query(q.id)
        .text("⚠️ 此反馈已过期，请在新消息中反馈")
        .await?;
    return Ok(());
}
```

### 3. 防止重复反馈

每个调度记录只接受一次反馈：

```rust
impl DispatchLearner {
    pub fn apply_feedback(&mut self, feedback: UserFeedback) -> bool {
        if let Some(record) = self.history.back_mut() {
            // 检查是否已有反馈
            if record.user_feedback.is_some() {
                eprintln!("⚠️  此调度已有反馈，无法重复提交");
                return false;
            }
            
            record.apply_feedback(feedback);
            // ... 持久化
            true
        } else {
            false
        }
    }
}
```

### 4. 反馈统计展示

在统计报告中包含反馈覆盖率：

```rust
pub struct DispatchStats {
    // ... 现有字段
    
    /// Feedback coverage rate (0.0 - 1.0).
    pub feedback_rate: f32,
    
    /// Feedback distribution.
    pub feedback_justright: usize,
    pub feedback_too_heavy: usize,
    pub feedback_too_light: usize,
}
```

---

## 🔧 配置选项

在 `config.yaml` 中添加反馈相关配置：

```yaml
model:
  auto_dispatch:
    enabled: true
    
    # 反馈收集设置
    feedback:
      # 是否自动显示反馈按钮（Telegram/Discord）
      show_buttons: true
      
      # 按钮超时时间（分钟）
      button_timeout: 10
      
      # CLI 模式下是否提示反馈
      prompt_in_cli: false
      
      # 反馈覆盖率警告阈值（低于此值发出提醒）
      low_feedback_warning: 0.1  # 10%
```

---

## 📊 学习效果验证

通过反馈数据优化调度策略：

```rust
let trends = learner.analyze_trends();
let suggestions = trends.suggest_optimizations();

// 示例输出：
// ⚠️ 近期准确率偏低 (62%)，建议调整复杂度评分阈值
// 💡 轻量模型使用率较低 (8%)，可尝试提高轻量模型阈值
// ✅ 用户反馈覆盖率: 23% (良好)
```

---

## 🚀 实施检查清单

### Telegram
- [ ] 在 `handle_message()` 中添加 `UserFeedback::from_command()` 解析
- [ ] 在调度决策消息后附加 `InlineKeyboardMarkup`
- [ ] 实现 `handle_callback_query()` 处理按钮点击
- [ ] 添加反馈超时验证
- [ ] 测试完整流程

### Discord
- [ ] 创建 `CreateActionRow` 按钮组件
- [ ] 在调度决策后发送带按钮的消息
- [ ] 实现 `ComponentInteraction` 处理器
- [ ] 测试交互响应

### WebUI
- [ ] 在前端添加反馈按钮组件
- [ ] 实现 `/api/dispatch/feedback` 后端端点
- [ ] 在调度统计页面显示反馈覆盖率
- [ ] 添加反馈历史可视化

### CLI
- [ ] 在 `hakimi chat` 模式中添加反馈提示
- [ ] 实现 `/lighter` / `/stronger` / `/justright` 命令
- [ ] 在 `--help` 中说明反馈命令

---

## 📚 API 参考

完整 API 文档见 `hakimi_core::model_dispatch::UserFeedback`：

```rust
pub enum UserFeedback {
    TooHeavy,    // 模型过重
    TooLight,    // 模型过轻
    JustRight,   // 选择恰当
}

impl UserFeedback {
    pub fn from_command(cmd: &str) -> Option<Self>;
    pub fn from_callback(data: &str) -> Option<Self>;
    pub fn message(&self) -> &'static str;
    pub fn emoji(&self) -> &'static str;
    pub fn button_text(&self) -> &'static str;
    pub fn callback_data(&self) -> &'static str;
    pub fn telegram_inline_buttons() -> String;
    pub fn discord_action_row() -> String;
}
```
