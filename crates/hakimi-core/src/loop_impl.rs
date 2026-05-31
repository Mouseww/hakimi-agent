use futures::StreamExt;
use hakimi_common::{
    FinishReason, HakimiError, Message, MessageRole, NormalizedResponse, Result, ToolCall,
    ToolDefinition, Usage,
};
use hakimi_transports::{RequestParams, StreamAccumulator, StreamEvent};
use tracing::{debug, info, warn};

use crate::agent::AIAgent;
use crate::budget::IterationBudget;
use crate::conversation::ConversationResult;
use crate::error_classifier::{
    ErrorClassifier, RecoveryAction, parse_available_output_tokens_from_error,
};
use crate::guardrails::{GuardrailDecision, ToolGuardrails};
use crate::retry::{jittered_backoff, should_retry};
use std::time::Duration;

/// Maximum number of retries for transient API errors.
const MAX_RETRIES: u32 = 3;

/// Base delay for exponential backoff.
const BASE_DELAY: Duration = Duration::from_secs(1);

/// Maximum delay cap for backoff.
const MAX_DELAY: Duration = Duration::from_secs(30);

/// Maximum number of automatic continuation requests after a model response
/// stops because it hit the provider output-token limit.
const MAX_LENGTH_CONTINUATIONS: usize = 3;

/// Safety margin below provider-reported available output tokens.
const OUTPUT_TOKEN_RETRY_SAFETY_MARGIN: u32 = 64;

/// Bound automatic max-token adjustments so malformed errors cannot spin.
const MAX_OUTPUT_TOKEN_ADJUSTMENTS: u32 = 2;

const CONTINUE_AFTER_LENGTH_PROMPT: &str = "Your previous response was cut off by the output token limit. Continue exactly where you stopped. Do not repeat earlier text. Finish the answer completely.";

/// Append streamed or continuation text without flattening layout.
///
/// Providers usually split streamed deltas at arbitrary byte/token boundaries,
/// so most chunks should be concatenated exactly. Automatic continuation is
/// different: the next provider response is a new assistant turn and often
/// begins without leading whitespace even when the previous turn ended in the
/// middle of prose. In that case a tiny separator prevents ASCII `hello` +
/// `world` becoming `helloworld`, while preserving explicit newlines, Markdown,
/// and CJK text. Do not use `char::is_alphanumeric()` here: it treats Chinese
/// characters as alphanumeric and would turn streamed text into `爸 爸 ， 更 新`.
pub fn append_text_preserving_layout(buffer: &mut String, next: &str) {
    if next.is_empty() {
        return;
    }
    if buffer.is_empty() {
        buffer.push_str(next);
        return;
    }

    let prev = buffer.chars().next_back();
    let next_first = next.chars().next();

    let needs_space = matches!(prev, Some(c) if c.is_ascii_alphanumeric())
        && matches!(next_first, Some(c) if c.is_ascii_alphanumeric());

    if needs_space {
        buffer.push(' ');
    }
    buffer.push_str(next);
}

fn join_continuation_parts(parts: &[String]) -> String {
    let mut merged = String::new();
    for part in parts {
        append_text_preserving_layout(&mut merged, part);
    }
    merged
}

fn adjust_max_tokens_for_available_output(params: &mut RequestParams, error: &str) -> Option<u32> {
    let available = parse_available_output_tokens_from_error(error)?;
    let adjusted = available
        .saturating_sub(OUTPUT_TOKEN_RETRY_SAFETY_MARGIN)
        .max(1);
    if let Some(current) = params.max_tokens
        && current <= adjusted
    {
        return None;
    }

    params.max_tokens = Some(adjusted);
    Some(adjusted)
}

/// Run the core conversation loop (non-streaming).
pub async fn run_loop(agent: &mut AIAgent) -> Result<ConversationResult> {
    run_loop_inner(agent, false).await
}

/// Run the streaming variant of the core conversation loop.
pub async fn run_loop_streaming(agent: &mut AIAgent) -> Result<ConversationResult> {
    run_loop_inner(agent, true).await
}

/// Shared inner loop — `streaming` controls how the response is fetched.
async fn run_loop_inner(agent: &mut AIAgent, streaming: bool) -> Result<ConversationResult> {
    let budget = IterationBudget::new(agent.max_iterations);
    let mut total_usage = Usage::default();
    let mut api_call_count: usize = 0;
    let mut continuation_parts: Vec<String> = Vec::new();
    let mut length_continuations: usize = 0;
    let mut tool_guardrails = ToolGuardrails::new();
    tool_guardrails.begin_turn();

    let tool_ctx = agent.build_tool_context();
    agent
        .tool_registry
        .configure_tool_search(
            agent.tool_search_config.clone(),
            agent.tool_search_context_length,
        )
        .await;
    let tool_assembly = agent.tool_registry.get_model_definitions().await;
    let tool_defs = tool_assembly.tool_defs;
    let params = RequestParams::default();

    debug!(
        tool_count = tool_defs.len(),
        tool_search_activated = tool_assembly.activated,
        deferred_tool_count = tool_assembly.deferred_count,
        deferred_tool_tokens = tool_assembly.deferred_tokens,
        tool_search_threshold_tokens = tool_assembly.threshold_tokens,
        max_iterations = agent.max_iterations,
        streaming = streaming,
        "Starting agent loop"
    );

    // Notify the context engine that a session has started.
    {
        let mut engine = agent.context_engine.write().await;
        engine.on_session_start();
    }

    loop {
        // Check budget and interrupt.
        if budget.is_exhausted() {
            warn!(api_calls = api_call_count, "Iteration budget exhausted");
            break;
        }
        if agent.check_interrupt() {
            info!("Agent loop interrupted by user");
            break;
        }

        // Build the messages array to send: system prompt + conversation history.
        let send_messages = build_send_messages(agent);

        // Check if context compression is needed.
        {
            let engine = agent.context_engine.read().await;
            if engine.should_compress() {
                drop(engine);
                let engine = agent.context_engine.write().await;
                engine.compress(&mut agent.messages).await?;
                info!("Context compression applied");
            }
        }

        // Fetch a response (streaming or non-streaming).
        let mut response = match fetch_response(
            agent.transport.as_ref(),
            &agent.model,
            streaming,
            &send_messages,
            &tool_defs,
            &params,
            agent.streaming_callback.clone(),
            &mut api_call_count,
        )
        .await
        {
            Ok(resp) => resp,
            Err(e) => {
                // Check if context compression might help.
                let classifier = ErrorClassifier::new();
                let classification = classifier.classify_transport_error(&e.to_string());
                if matches!(classification.action, RecoveryAction::CompressContext) {
                    warn!(error = %e, "Context overflow detected — compressing and retrying");
                    let engine = agent.context_engine.write().await;
                    engine.compress(&mut agent.messages).await?;
                    continue;
                }
                return Err(e);
            }
        };
        scrub_response_content(&mut response);

        // Track usage.
        if let Some(ref usage) = response.usage {
            total_usage.accumulate(usage);
            let mut engine = agent.context_engine.write().await;
            engine.update_from_response(usage);
        }

        // Check for content filter.
        if response.finish_reason == Some(FinishReason::ContentFilter) {
            return Err(HakimiError::Other(
                "Response was filtered by the content policy".into(),
            ));
        }

        // Check for tool calls.
        if response.has_tool_calls() {
            process_tool_calls(agent, &response, &tool_ctx, &mut tool_guardrails).await;
            budget.use_one();
            continue;
        }

        // Text response. If the provider stopped because of an output-token
        // length limit, continue automatically instead of returning a visibly
        // truncated final_response to the caller.
        let final_text = response.content.unwrap_or_default();
        let stopped_by_length = response.finish_reason == Some(FinishReason::Length);
        agent.messages.push(build_assistant_message(
            Some(final_text.clone()),
            response.reasoning.clone(),
            response.finish_reason.clone(),
        ));

        if stopped_by_length && length_continuations < MAX_LENGTH_CONTINUATIONS {
            warn!(
                continuation = length_continuations + 1,
                max_continuations = MAX_LENGTH_CONTINUATIONS,
                "Assistant response hit output length limit; requesting continuation"
            );
            continuation_parts.push(final_text);
            length_continuations += 1;
            agent
                .messages
                .push(Message::user(CONTINUE_AFTER_LENGTH_PROMPT));
            budget.use_one();
            continue;
        }

        let final_response = if continuation_parts.is_empty() {
            final_text
        } else {
            continuation_parts.push(final_text);
            join_continuation_parts(&continuation_parts)
        };

        // Notify context engine that the session ended.
        {
            let mut engine = agent.context_engine.write().await;
            engine.on_session_end();
        }

        info!(
            api_calls = api_call_count,
            response_len = final_response.len(),
            "Agent loop completed"
        );

        return Ok(ConversationResult {
            final_response,
            messages: agent.messages.clone(),
            usage: total_usage,
            api_call_count,
        });
    }

    // Budget exhausted or interrupted — return what we have.
    {
        let mut engine = agent.context_engine.write().await;
        engine.on_session_end();
    }

    Ok(ConversationResult {
        final_response: String::new(),
        messages: agent.messages.clone(),
        usage: total_usage,
        api_call_count,
    })
}

/// Fetch a response from the transport, with retry logic.
#[allow(clippy::too_many_arguments)]
async fn fetch_response(
    transport: &dyn hakimi_transports::ProviderTransport,
    model: &str,
    streaming: bool,
    send_messages: &[Message],
    tool_defs: &[ToolDefinition],
    params: &RequestParams,
    callback: Option<std::sync::Arc<dyn Fn(String) + Send + Sync>>,
    api_call_count: &mut usize,
) -> Result<NormalizedResponse> {
    // Maximum retry attempts per fetch.
    let max_retries = MAX_RETRIES;
    let mut attempt = 0;
    let mut output_token_adjustments = 0;
    let mut effective_params = params.clone();

    loop {
        let result = if streaming {
            fetch_streaming_response(
                transport,
                model,
                send_messages,
                tool_defs,
                &effective_params,
                callback.clone(),
            )
            .await
        } else {
            transport
                .execute(model, send_messages, tool_defs, &effective_params)
                .await
        };

        match result {
            Ok(resp) => {
                *api_call_count += 1;
                return Ok(resp);
            }
            Err(e) => {
                *api_call_count += 1;
                let error_text = e.to_string();

                if output_token_adjustments < MAX_OUTPUT_TOKEN_ADJUSTMENTS
                    && let Some(max_tokens) =
                        adjust_max_tokens_for_available_output(&mut effective_params, &error_text)
                {
                    output_token_adjustments += 1;
                    warn!(
                        max_tokens = max_tokens,
                        adjustment = output_token_adjustments,
                        "Provider reported a smaller available output budget; retrying with adjusted max_tokens"
                    );
                    continue;
                }

                let classifier = ErrorClassifier::new();
                let classification = classifier.classify_transport_error(&error_text);

                match classification.action {
                    RecoveryAction::CompressContext => {
                        // Signal the caller to compress context and retry.
                        warn!(error = %e, "Context overflow detected — returning error for caller to handle");
                        return Err(e);
                    }
                    RecoveryAction::Abort => {
                        warn!(error = %e, reason = ?classification.reason, "Non-recoverable error — aborting");
                        return Err(e);
                    }
                    _ if should_retry(&e, attempt, max_retries) => {
                        let delay = if let Some(retry_after) = classification.retry_after_ms {
                            Duration::from_millis(retry_after)
                        } else {
                            jittered_backoff(attempt, BASE_DELAY, MAX_DELAY)
                        };
                        warn!(
                            error = %e,
                            reason = ?classification.reason,
                            action = ?classification.action,
                            attempt = attempt,
                            delay_ms = delay.as_millis(),
                            "Retrying after classified error"
                        );
                        tokio::time::sleep(delay).await;
                        attempt += 1;
                        continue;
                    }
                    _ => return Err(e),
                }
            }
        }
    }
}

/// Open a streaming connection, consume the stream, and return the accumulated response.
async fn fetch_streaming_response(
    transport: &dyn hakimi_transports::ProviderTransport,
    model: &str,
    send_messages: &[Message],
    tool_defs: &[ToolDefinition],
    params: &RequestParams,
    callback: Option<std::sync::Arc<dyn Fn(String) + Send + Sync>>,
) -> Result<NormalizedResponse> {
    let mut stream = transport
        .execute_streaming(model, send_messages, tool_defs, params)
        .await?;

    let mut accumulator = StreamAccumulator::new();
    let scrubber = hakimi_transports::scrubber::ThinkScrubber::new();
    let scrubber = std::sync::Arc::new(tokio::sync::Mutex::new(scrubber));
    let mut saw_terminal_event = false;

    while let Some(item) = stream.next().await {
        match item {
            Ok(event) => {
                if matches!(event, StreamEvent::Done | StreamEvent::Finished(_)) {
                    saw_terminal_event = true;
                }
                // Print content deltas to stdout in real-time.
                if let StreamEvent::ContentDelta(text) = event {
                    let mut s = scrubber.lock().await;
                    let (clean_text, _) = s.process(&text);
                    if !clean_text.is_empty() {
                        if let Some(ref cb) = callback {
                            cb(clean_text.clone());
                        }
                        use std::io::Write;
                        let _ = std::io::stdout().write_all(clean_text.as_bytes());
                        let _ = std::io::stdout().flush();
                        accumulator.push(StreamEvent::ContentDelta(clean_text));
                    }
                } else {
                    accumulator.push(event);
                }
            }
            Err(e) => {
                warn!(error = %e, "Error in streaming response");
                return Err(HakimiError::Transport(format!("Stream error: {e}")));
            }
        }
    }

    let (tail, scrubbed_reasoning) = {
        let mut s = scrubber.lock().await;
        let tail = s.flush();
        let reasoning = s.reasoning().to_string();
        (tail, reasoning)
    };
    if !tail.is_empty() {
        if let Some(ref cb) = callback {
            cb(tail.clone());
        }
        use std::io::Write;
        let _ = std::io::stdout().write_all(tail.as_bytes());
        let _ = std::io::stdout().flush();
        accumulator.push(StreamEvent::ContentDelta(tail));
    }
    if !scrubbed_reasoning.is_empty() {
        accumulator.push(StreamEvent::ReasoningDelta(scrubbed_reasoning));
    }

    if !saw_terminal_event {
        warn!("Streaming response ended before provider completion event");
        return Err(HakimiError::Transport(
            "stream ended before provider completion event".to_string(),
        ));
    }

    // Print a newline after the stream completes.
    {
        use std::io::Write;
        let _ = std::io::stdout().write_all(b"\n");
        let _ = std::io::stdout().flush();
    }

    Ok(accumulator_to_response(&accumulator))
}

fn scrub_response_content(response: &mut NormalizedResponse) {
    let Some(content) = response.content.take() else {
        return;
    };

    let (cleaned, scrubbed_reasoning) =
        hakimi_transports::scrubber::ThinkScrubber::strip_all_with_reasoning(&content);
    response.content = if cleaned.is_empty() {
        None
    } else {
        Some(cleaned)
    };

    if !scrubbed_reasoning.is_empty() {
        match response.reasoning.as_mut() {
            Some(existing) if !existing.is_empty() => {
                existing.push('\n');
                existing.push_str(&scrubbed_reasoning);
            }
            Some(existing) => existing.push_str(&scrubbed_reasoning),
            None => response.reasoning = Some(scrubbed_reasoning),
        }
    }
}

/// Process tool calls: append assistant message, check guardrails, dispatch tools.
fn truncate_for_tool_notice(value: &str, max_chars: usize) -> String {
    let normalized = value.replace('\n', " ");
    let mut chars = normalized.chars();
    let truncated: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
    }
}

fn self_improvement_notice(message: &Message) -> Option<&'static str> {
    if message.name.as_deref() != Some("memory") {
        return None;
    }

    let content = message.content.as_deref()?;
    if content.starts_with("Added content to user memory")
        || content.starts_with("Replaced user memory content")
        || content.starts_with("Removed matching text from user memory")
    {
        Some("💾 Self-improvement review: User profile updated")
    } else if content.starts_with("Added content to memory memory")
        || content.starts_with("Replaced memory memory content")
        || content.starts_with("Removed matching text from memory memory")
    {
        Some("💾 Self-improvement review: Memory updated")
    } else {
        None
    }
}

fn tool_result_media_event(message: &Message) -> Option<String> {
    let content = message.content.as_deref()?;
    if let Some(path) = content.strip_prefix("MEDIA:") {
        let path = path.trim();
        if !path.is_empty() {
            return Some(format!("MEDIA:{path}"));
        }
    }
    if let Some(path) = content.strip_prefix("IMAGE:") {
        let path = path.trim();
        if !path.is_empty() {
            return Some(format!("IMAGE:{path}"));
        }
    }
    None
}

async fn process_tool_calls(
    agent: &mut AIAgent,
    response: &NormalizedResponse,
    tool_ctx: &hakimi_common::ToolContext,
    guardrails: &mut ToolGuardrails,
) {
    let tool_calls = response.tool_calls.as_ref().unwrap();

    debug!(count = tool_calls.len(), "Processing tool calls");

    // Append the assistant message (with tool_calls) to history.
    let assistant_msg = build_assistant_message_with_tools(
        response.content.clone(),
        tool_calls,
        response.reasoning.clone(),
    );
    agent.messages.push(assistant_msg);

    // We will collect futures for tool dispatch and run them concurrently.
    let mut futures = Vec::new();
    let mut halt_message = None;
    // First check guardrails and collect safe tools to dispatch
    for tc in tool_calls {
        let args: serde_json::Value =
            serde_json::from_str(&tc.arguments).unwrap_or_else(|_| serde_json::json!({}));
        let mut arg_summary = String::new();
        if let Some(obj) = args.as_object() {
            let mut parts = Vec::new();
            for (k, v) in obj {
                // Skip very long fields like 'content' in write_file
                if k == "content" || k == "patch" || k == "code" {
                    parts.push(format!("{}: [...]", k));
                    continue;
                }
                let v_str = if let Some(s) = v.as_str() {
                    s.to_string()
                } else {
                    v.to_string()
                };
                // Truncate long strings but keep them readable
                let v_trunc = truncate_for_tool_notice(&v_str, 40);
                parts.push(format!("{}: {}", k, v_trunc));
            }
            if !parts.is_empty() {
                arg_summary = format!(" ({})", parts.join(", "));
            }
        }

        let tool_notice = format!("⚙️ {}{}", tc.name, arg_summary);
        if let Some(ref cb) = agent.streaming_callback {
            cb(format!("\u{001e}hakimi_tool:{tool_notice}"));
        }
        // Print to stdout as well
        use std::io::Write;
        let _ = std::io::stdout().write_all(tool_notice.as_bytes());
        let _ = std::io::stdout().write_all(b"\n");
        let _ = std::io::stdout().flush();

        if agent.check_interrupt() {
            info!("Interrupted during tool dispatch");
            break;
        }

        // Check guardrails before dispatching
        let decision = guardrails.record_call(&tc.name, &tc.arguments);
        match decision {
            GuardrailDecision::Halt(reason) => {
                warn!(tool = %tc.name, reason = %reason, "Guardrails halted tool dispatch");
                halt_message = Some(Message::tool_result(
                    &tc.id,
                    &tc.name,
                    format!("HALT: Tool dispatch halted by guardrails: {reason}"),
                ));
                break;
            }
            GuardrailDecision::SyntheticResult(msg) => {
                warn!(tool = %tc.name, "Injecting synthetic result to break loop");
                halt_message = Some(Message::tool_result(&tc.id, &tc.name, msg));
                break;
            }
            GuardrailDecision::Warn(msg) => {
                warn!(tool = %tc.name, reason = %msg, "Guardrail warning");
            }
            GuardrailDecision::Allow => {}
        }

        let registry = agent.tool_registry.clone();
        futures.push(async move { dispatch_tool(&registry, tool_ctx, tc).await });
    }

    let results = if !futures.is_empty() {
        futures::future::join_all(futures).await
    } else {
        Vec::new()
    };

    for mut res in results {
        let guardrail_decision =
            if let (Some(tool_name), Some(content)) = (res.name.as_deref(), res.content.as_deref())
            {
                Some((
                    tool_name.to_string(),
                    guardrails.record_result(tool_name, content),
                ))
            } else {
                None
            };

        if let Some((tool_name, decision)) = guardrail_decision {
            match decision {
                GuardrailDecision::Warn(msg) => {
                    warn!(tool = %tool_name, reason = %msg, "Guardrail result warning");
                    append_tool_guardrail_notice(&mut res, "Tool loop warning", &msg);
                }
                GuardrailDecision::SyntheticResult(msg) => {
                    warn!(tool = %tool_name, "Guardrail result synthetic guidance");
                    append_tool_guardrail_notice(&mut res, "Tool loop guardrail", &msg);
                }
                GuardrailDecision::Halt(msg) => {
                    warn!(tool = %tool_name, reason = %msg, "Guardrail result halt guidance");
                    append_tool_guardrail_notice(&mut res, "Tool loop hard stop", &msg);
                }
                GuardrailDecision::Allow => {}
            }
        }
        if let Some(store) = &mut agent.skill_store
            && let Some(content) = &res.content
        {
            store.observe_tool_result(content);
        }
        if let Some(media_event) = tool_result_media_event(&res)
            && let Some(ref cb) = agent.streaming_callback
        {
            cb(format!("\u{001e}hakimi_media:{media_event}"));
        }
        if let Some(review_notice) = self_improvement_notice(&res)
            && let Some(ref cb) = agent.streaming_callback
        {
            cb(format!("\u{001e}hakimi_review:{review_notice}"));
        }
        agent.messages.push(res);
    }

    if let Some(msg) = halt_message {
        agent.messages.push(msg);
    }
}

fn append_tool_guardrail_notice(message: &mut Message, label: &str, notice: &str) {
    let Some(content) = message.content.as_mut() else {
        return;
    };

    content.push_str("\n\n[");
    content.push_str(label);
    content.push_str(": ");
    content.push_str(notice);
    content.push(']');
}

/// Build the messages array to send to the API:
/// dynamic system prompt + full conversation history.
fn build_send_messages(agent: &AIAgent) -> Vec<Message> {
    let mut send = Vec::with_capacity(agent.messages.len() + 1);

    let base = agent
        .system_prompt
        .clone()
        .unwrap_or_else(|| crate::DEFAULT_SYSTEM_PROMPT.to_string());
    let skill_context = agent
        .skill_store
        .as_ref()
        .map(|store| store.render_active_skill_context())
        .unwrap_or_default();

    let system_prompt = if skill_context.is_empty() {
        base
    } else {
        format!("{base}\n\n{skill_context}")
    };

    if !system_prompt.is_empty() {
        send.push(Message::system(system_prompt));
    }

    send.extend(agent.messages.iter().cloned());
    send
}

/// Build a plain assistant message while preserving metadata such as reasoning
/// and finish reason. This is important for continuation handling: a response
/// with `finish_reason=length` is stored as a partial assistant turn before the
/// agent asks the model to continue.
fn build_assistant_message(
    content: Option<String>,
    reasoning: Option<String>,
    finish_reason: Option<FinishReason>,
) -> Message {
    Message {
        role: MessageRole::Assistant,
        content,
        images: None,
        tool_calls: None,
        tool_call_id: None,
        name: None,
        reasoning: reasoning.clone(),
        reasoning_content: reasoning,
        timestamp: None,
        token_count: None,
        finish_reason: finish_reason.map(finish_reason_as_str).map(str::to_string),
    }
}

fn finish_reason_as_str(reason: FinishReason) -> &'static str {
    match reason {
        FinishReason::Stop => "stop",
        FinishReason::ToolCalls => "tool_calls",
        FinishReason::Length => "length",
        FinishReason::ContentFilter => "content_filter",
        FinishReason::Error => "error",
    }
}

/// Build an assistant message that carries tool calls.
fn build_assistant_message_with_tools(
    content: Option<String>,
    tool_calls: &[ToolCall],
    reasoning: Option<String>,
) -> Message {
    Message {
        role: MessageRole::Assistant,
        content,
        images: None,
        tool_calls: Some(tool_calls.to_vec()),
        tool_call_id: None,
        name: None,
        reasoning: reasoning.clone(),
        reasoning_content: reasoning,
        timestamp: None,
        token_count: None,
        finish_reason: None,
    }
}

/// Dispatch a single tool call via the tool registry and return the result message.
async fn dispatch_tool(
    tool_registry: &hakimi_tools::ToolRegistry,
    tool_ctx: &hakimi_common::ToolContext,
    tc: &ToolCall,
) -> Message {
    // Parse the JSON arguments.
    let args: serde_json::Value = serde_json::from_str(&tc.arguments).unwrap_or_else(|e| {
        warn!(
            tool = %tc.name,
            error = %e,
            raw_args = %tc.arguments,
            "Failed to parse tool arguments, using empty object"
        );
        serde_json::Value::Object(Default::default())
    });

    debug!(tool = %tc.name, call_id = %tc.id, "Dispatching tool");

    match tool_registry.dispatch(&tc.name, &args, tool_ctx).await {
        Ok(content) => {
            debug!(
                tool = %tc.name,
                result_len = content.len(),
                "Tool executed successfully"
            );
            Message::tool_result(&tc.id, &tc.name, content)
        }
        Err(e) => {
            warn!(tool = %tc.name, error = %e, "Tool execution failed");
            Message::tool_result(&tc.id, &tc.name, format!("Error: {e}"))
        }
    }
}

/// Convert a [`StreamAccumulator`] into a [`NormalizedResponse`].
fn accumulator_to_response(acc: &StreamAccumulator) -> NormalizedResponse {
    let content = if acc.content.is_empty() {
        None
    } else {
        Some(acc.content.clone())
    };

    let tool_calls: Vec<ToolCall> = acc
        .tool_calls
        .iter()
        .map(|tc| ToolCall {
            id: tc.id.clone(),
            name: tc.name.clone(),
            arguments: tc.arguments.clone(),
            index: None,
        })
        .collect();

    let tool_calls = if tool_calls.is_empty() {
        None
    } else {
        Some(tool_calls)
    };

    let finish_reason = if tool_calls.is_some() {
        Some(FinishReason::ToolCalls)
    } else if let Some(reason) = acc.finish_reason.as_deref() {
        match reason {
            "stop" | "end_turn" | "STOP" => Some(FinishReason::Stop),
            "length" | "max_tokens" | "MAX_TOKENS" => Some(FinishReason::Length),
            "content_filter" | "SAFETY" => Some(FinishReason::ContentFilter),
            _ => Some(FinishReason::Stop),
        }
    } else if content.is_some() {
        Some(FinishReason::Stop)
    } else {
        None
    };

    let usage = if acc.prompt_tokens > 0 || acc.completion_tokens > 0 {
        Some(Usage {
            prompt_tokens: acc.prompt_tokens,
            completion_tokens: acc.completion_tokens,
            total_tokens: acc.prompt_tokens + acc.completion_tokens,
            cached_tokens: 0,
            reasoning_tokens: 0,
        })
    } else {
        None
    };

    let reasoning = if acc.reasoning.is_empty() {
        None
    } else {
        Some(acc.reasoning.clone())
    };

    NormalizedResponse {
        content,
        tool_calls,
        finish_reason,
        usage,
        reasoning,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        adjust_max_tokens_for_available_output, append_text_preserving_layout,
        self_improvement_notice, tool_result_media_event, truncate_for_tool_notice,
    };
    use hakimi_common::Message;
    use hakimi_transports::RequestParams;

    #[test]
    fn append_streamed_cjk_chunks_without_spaces() {
        let mut out = String::new();
        for chunk in ["爸", "爸", "，", "/", "update", " ", "现在", "只是"] {
            append_text_preserving_layout(&mut out, chunk);
        }
        assert_eq!(out, "爸爸，/update 现在只是");
    }

    #[test]
    fn append_ascii_word_continuations_with_separator() {
        let mut out = String::from("hello");
        append_text_preserving_layout(&mut out, "world");
        assert_eq!(out, "hello world");
    }

    #[test]
    fn truncate_tool_notice_is_utf8_safe_for_chinese() {
        let input = "[\"我想配置 OpenAI 反代 API\",\"我想配置 Anthropic/Claude 反代 API\",\"我需要查看当前的环境变量配置\",\"我不确定，请先帮我看看当前设置\"]";
        let truncated = truncate_for_tool_notice(input, 40);

        assert!(truncated.ends_with("..."));
        assert!(truncated.is_char_boundary(truncated.len()));
        assert!(truncated.contains("我想配置 OpenAI 反代 API"));
    }

    #[test]
    fn truncate_tool_notice_replaces_newlines_without_truncating_short_text() {
        assert_eq!(truncate_for_tool_notice("hello\nworld", 40), "hello world");
    }

    #[test]
    fn adjusts_unset_max_tokens_from_provider_available_budget() {
        let mut params = RequestParams::default();
        let error = "max_tokens: 128000 > context_window: 200000 - input_tokens: 180000 = available_tokens: 20000";

        assert_eq!(
            adjust_max_tokens_for_available_output(&mut params, error),
            Some(19936)
        );
        assert_eq!(params.max_tokens, Some(19936));
    }

    #[test]
    fn keeps_smaller_explicit_max_tokens() {
        let mut params = RequestParams {
            max_tokens: Some(4096),
            ..Default::default()
        };
        let error = "max_tokens: 8192 > context_window: 200000 - input_tokens: 195000 = available_tokens: 5000";

        assert_eq!(
            adjust_max_tokens_for_available_output(&mut params, error),
            None
        );
        assert_eq!(params.max_tokens, Some(4096));
    }

    #[test]
    fn ignores_prompt_overflow_without_available_output_budget() {
        let mut params = RequestParams::default();
        let error = "prompt is too long: 205000 tokens > 200000 maximum";

        assert_eq!(
            adjust_max_tokens_for_available_output(&mut params, error),
            None
        );
        assert_eq!(params.max_tokens, None);
    }

    #[test]
    fn self_improvement_notice_reports_user_profile_updates() {
        let msg = Message::tool_result(
            "call-1",
            "memory",
            "Added content to user memory (/tmp/user.md).",
        );

        assert_eq!(
            self_improvement_notice(&msg),
            Some("💾 Self-improvement review: User profile updated")
        );
    }

    #[test]
    fn self_improvement_notice_ignores_non_memory_tools() {
        let msg = Message::tool_result("call-1", "patch", "Added content to user memory");

        assert_eq!(self_improvement_notice(&msg), None);
    }

    #[test]
    fn tool_result_media_event_extracts_media_and_image_prefixes() {
        let media = Message::tool_result("call-1", "text_to_speech", "MEDIA:/tmp/audio.mp3");
        let image = Message::tool_result("call-2", "image_generate", "IMAGE:/tmp/image.png");

        assert_eq!(
            tool_result_media_event(&media).as_deref(),
            Some("MEDIA:/tmp/audio.mp3")
        );
        assert_eq!(
            tool_result_media_event(&image).as_deref(),
            Some("IMAGE:/tmp/image.png")
        );
    }
}
