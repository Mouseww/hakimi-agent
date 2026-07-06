# 工具调用记录显示设计

## 目标
在 Telegram 和 WebUI 中，协作消息（子 agent）的折叠块内部显示逐行的工具调用记录。

## 当前实现
1. 工具调用发送通知：`loop_impl.rs:667-670`
   ```rust
   cb(format!("\u{001e}hakimi_tool:{tool_notice}"));
   ```

2. 子 agent 转发：`delegate.rs:283-292`
   ```rust
   if let Some(tool_notice) = token.strip_prefix("\u{001e}hakimi_tool:") {
       emit_delegate_progress(..., tool_notice.trim());
   }
   ```

3. 进度格式：`delegate.rs:59-66`
   ```rust
   cb(format!("\u{001e}hakimi_delegate:{}|{}|{}|{}",
       task_id, title, line, timestamp));
   ```

## 问题分析
- 当前每个工具调用都作为独立的进度行发送
- Gateway 接收后立即显示为单独的消息
- 折叠块只有开始/结束标记，内部没有积累记录

## 解决方案

### 方案 A：在完成时收集并显示（推荐）
**核心思路**：
- 子 agent 执行过程中，**不**实时发送工具调用通知
- 在完成时，把所有工具调用记录一次性放到完成消息中
- 完成消息格式：
  ```
  完成，返回结果（使用了 N 个工具）
  [工具调用详情]
  ⚙️ read_file (path: agent.rs)
  ⚙️ search_files (pattern: hakimi_tool)
  ⚙️ patch (path: lib.rs)
  ```

**实现步骤**：
1. 在 `delegate.rs` 中收集工具调用记录
2. 修改完成消息，附加工具调用列表
3. Gateway 识别并格式化为折叠块

**代码位置**：
- `crates/hakimi-core/src/delegate.rs:275-323`
- `crates/hakimi-gateway/src/telegram.rs`
- `crates/hakimi-gateway/src/teams_webhook.rs`（WebUI 相关）

### 方案 B：实时流式积累（更复杂）
**核心思路**：
- Gateway 层维护每个子 agent 的工具调用历史
- 实时接收并积累
- 最终格式化为折叠块

**实现步骤**：
1. Gateway 维护 `HashMap<task_id, Vec<tool_notice>>`
2. 接收 `hakimi_delegate` 消息时积累
3. 完成时一次性渲染

**缺点**：
- Gateway 层需要维护状态
- 复杂度高
- 不同渠道需要各自实现

## 推荐实现：方案 A

### 1. 修改 delegate.rs
```rust
// 在 child_agent.set_streaming_callback 中收集工具调用
let tool_calls = Arc::new(tokio::sync::Mutex::new(Vec::new()));
let tool_calls_clone = tool_calls.clone();

child_agent.set_streaming_callback(Some(Arc::new(move |token: String| {
    if let Some(tool_notice) = token.strip_prefix("\u{001e}hakimi_tool:") {
        counter.fetch_add(1, Ordering::Relaxed);
        let mut calls = tool_calls_clone.blocking_lock();
        calls.push(tool_notice.trim().to_string());
        // 不再立即发送进度通知
    }
})));

// 完成时发送带工具列表的完成消息
let tool_usage = tool_count.load(Ordering::Relaxed);
let calls = tool_calls.lock().await;
let mut summary = if tool_usage > 0 {
    format!("完成，返回结果（使用了 {} 个工具）", tool_usage)
} else {
    "完成，返回结果".to_string()
};

if !calls.is_empty() {
    summary.push_str("\n[工具调用详情]\n");
    for call in calls.iter() {
        summary.push_str(call);
        summary.push('\n');
    }
}

emit_delegate_progress(&progress_callback, &progress_task_id, &progress_title, summary);
```

### 2. 修改 Telegram Gateway
在 `telegram.rs` 中识别并格式化：
```rust
fn format_collaboration_message(text: &str) -> String {
    if let Some((summary, details)) = text.split_once("[工具调用详情]") {
        format!("{}||{}||", summary.trim(), details.trim())
    } else {
        text.to_string()
    }
}
```

Telegram 的 `||spoiler||` 语法会自动创建折叠块。

### 3. 修改 WebUI
在 SSE 消息中嵌入特殊格式，前端识别并渲染为折叠块。

## 优先级
1. **Telegram**：最常用，优先实现
2. **WebUI**：次要，后续实现
3. **其他渠道**：按需实现

## 测试
```bash
# 启动 Gateway
hakimi gateway

# 触发包含 delegate_task 的任务
# 观察子 agent 完成消息是否包含工具调用列表
```

## 兼容性
- 不影响非子 agent 的工具调用显示
- 不影响其他进度通知
- 向后兼容现有 gateway
