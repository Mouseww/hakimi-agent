use futures::StreamExt;
use hakimi_common::{
    FinishReason, HakimiError, Message, MessageRole, NormalizedResponse, Result, ToolCall, Usage,
};
use hakimi_transports::{RequestParams, StreamAccumulator, StreamEvent};
use tracing::{debug, info, warn};

use crate::agent::AIAgent;
use crate::budget::IterationBudget;
use crate::conversation::ConversationResult;
use crate::error_classifier::{ErrorClassifier, RecoveryAction};
use crate::guardrails::{GuardrailDecision, ToolGuardrails};
use crate::retry::{jittered_backoff, should_retry};
use std::time::Duration;

/// Maximum number of retries for transient API errors.
const MAX_RETRIES: u32 = 3;

/// Base delay for exponential backoff.
const BASE_DELAY: Duration = Duration::from_secs(1);

/// Maximum delay cap for backoff.
const MAX_DELAY: Duration = Duration::from_secs(30);

/// Run the core conversation loop.
///
/// This function:
/// 1. Builds the messages array (system prompt + conversation history).
/// 2. Calls the transport to get an LLM response.
/// 3. If the response contains tool calls, dispatches each one, appends
///    the results, and loops back.
/// 4. If the response is plain text, returns it as the final answer.
/// 5. On transient errors, retries with jittered exponential backoff.
///
/// Returns a [`ConversationResult`] with the final response, all messages,
/// accumulated usage, and the API call count.
pub async fn run_loop(agent: &mut AIAgent) -> Result<ConversationResult> {
    let budget = IterationBudget::new(agent.max_iterations);
    let mut total_usage = Usage::default();
    let mut api_call_count: usize = 0;

    let tool_ctx = agent.build_tool_context();
    let tool_defs = agent.tool_registry.get_definitions().await;
    let params = RequestParams::default();

    debug!(
        tool_count = tool_defs.len(),
        max_iterations = agent.max_iterations,
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

        // Call the transport.
        let response = match agent
            .transport
            .execute(&agent.model, &send_messages, &tool_defs, &params)
            .await
        {
            Ok(resp) => resp,
            Err(e) => {
                api_call_count += 1;
                let attempt = (api_call_count % (MAX_RETRIES as usize + 1)) as u32;

                // Use error classifier for smarter recovery decisions
                let classifier = ErrorClassifier::new();
                let classification = classifier.classify_transport_error(&e.to_string());

                match classification.action {
                    RecoveryAction::CompressContext => {
                        warn!(error = %e, "Context overflow detected — triggering compression");
                        let engine = agent.context_engine.write().await;
                        engine.compress(&mut agent.messages).await?;
                        continue;
                    }
                    RecoveryAction::Abort => {
                        warn!(error = %e, reason = ?classification.reason, "Non-recoverable error — aborting");
                        return Err(e);
                    }
                    _ if should_retry(&e, attempt, MAX_RETRIES) => {
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
                        continue;
                    }
                    _ => return Err(e),
                }
            }
        };

        api_call_count += 1;

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
            let tool_calls = response.tool_calls.unwrap();

            debug!(count = tool_calls.len(), "Processing tool calls");

            // Append the assistant message (with tool_calls) to history.
            let assistant_msg = build_assistant_message_with_tools(
                response.content.clone(),
                &tool_calls,
                response.reasoning.clone(),
            );
            agent.messages.push(assistant_msg);

            // Initialize guardrails for this turn.
            let mut guardrails = ToolGuardrails::new();

            // Dispatch each tool call and append results.
            for tc in &tool_calls {
                if agent.check_interrupt() {
                    info!("Interrupted during tool dispatch");
                    break;
                }

                // Check guardrails before dispatching
                let decision = guardrails.record_call(&tc.name, &tc.arguments);
                match decision {
                    GuardrailDecision::Halt(reason) => {
                        warn!(tool = %tc.name, reason = %reason, "Guardrails halted tool dispatch");
                        agent.messages.push(Message::tool_result(
                            &tc.id,
                            &tc.name,
                            format!("HALT: Tool dispatch halted by guardrails: {reason}"),
                        ));
                        break;
                    }
                    GuardrailDecision::SyntheticResult(msg) => {
                        warn!(tool = %tc.name, "Injecting synthetic result to break loop");
                        agent
                            .messages
                            .push(Message::tool_result(&tc.id, &tc.name, msg));
                        continue;
                    }
                    GuardrailDecision::Warn(msg) => {
                        warn!(tool = %tc.name, reason = %msg, "Guardrail warning");
                    }
                    GuardrailDecision::Allow => {}
                }

                let tool_result = dispatch_tool(agent, &tool_ctx, tc).await;
                agent.messages.push(tool_result);
            }

            budget.use_one();
            continue;
        }

        // Text response — we're done.
        let final_text = response.content.unwrap_or_default();

        // Append the final assistant message to history.
        agent.messages.push(Message::assistant(&final_text));

        // Notify context engine that the session ended.
        {
            let mut engine = agent.context_engine.write().await;
            engine.on_session_end();
        }

        info!(
            api_calls = api_call_count,
            response_len = final_text.len(),
            "Agent loop completed"
        );

        return Ok(ConversationResult {
            final_response: final_text,
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

/// Build the messages array to send to the API:
/// system prompt (if set) + full conversation history.
fn build_send_messages(agent: &AIAgent) -> Vec<Message> {
    let mut send = Vec::with_capacity(agent.messages.len() + 1);

    // Prepend system prompt if configured.
    if let Some(ref sp) = agent.system_prompt
        && !sp.is_empty()
    {
        send.push(Message::system(sp.clone()));
    }

    send.extend(agent.messages.iter().cloned());
    send
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
    agent: &AIAgent,
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

    match agent
        .tool_registry
        .dispatch(&tc.name, &args, tool_ctx)
        .await
    {
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

/// Run the streaming variant of the core conversation loop.
///
/// This function is identical to [`run_loop`] in its overall structure, but
/// uses `transport.execute_streaming()` to receive tokens as they are generated.
/// Content deltas are printed to stdout in real-time for a responsive REPL UX.
///
/// After the stream is fully consumed, the accumulated response is processed
/// exactly like a non-streaming response: tool calls are dispatched and the
/// loop iterates until a text response is produced or the budget is exhausted.
pub async fn run_loop_streaming(agent: &mut AIAgent) -> Result<ConversationResult> {
    let budget = IterationBudget::new(agent.max_iterations);
    let mut total_usage = Usage::default();
    let mut api_call_count: usize = 0;

    let tool_ctx = agent.build_tool_context();
    let tool_defs = agent.tool_registry.get_definitions().await;
    let params = RequestParams::default();

    debug!(
        tool_count = tool_defs.len(),
        max_iterations = agent.max_iterations,
        "Starting streaming agent loop"
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

        // Open a streaming connection to the transport.
        let mut stream = match agent
            .transport
            .execute_streaming(&agent.model, &send_messages, &tool_defs, &params)
            .await
        {
            Ok(s) => s,
            Err(e) => {
                api_call_count += 1;
                let attempt = (api_call_count % (MAX_RETRIES as usize + 1)) as u32;

                // Use error classifier for smarter recovery decisions
                let classifier = ErrorClassifier::new();
                let classification = classifier.classify_transport_error(&e.to_string());

                match classification.action {
                    RecoveryAction::CompressContext => {
                        warn!(error = %e, "Context overflow detected — triggering compression (streaming)");
                        let engine = agent.context_engine.write().await;
                        engine.compress(&mut agent.messages).await?;
                        continue;
                    }
                    RecoveryAction::Abort => {
                        warn!(error = %e, reason = ?classification.reason, "Non-recoverable error — aborting (streaming)");
                        return Err(e);
                    }
                    _ if should_retry(&e, attempt, MAX_RETRIES) => {
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
                            "Retrying after classified error (streaming)"
                        );
                        tokio::time::sleep(delay).await;
                        continue;
                    }
                    _ => return Err(e),
                }
            }
        };

        api_call_count += 1;

        // Consume the stream and accumulate the response.
        let mut accumulator = StreamAccumulator::new();
        while let Some(item) = stream.next().await {
            match item {
                Ok(event) => {
                    // Print content deltas to stdout in real-time.
                    if let StreamEvent::ContentDelta(ref text) = event {
                        use std::io::Write;
                        let _ = std::io::stdout().write_all(text.as_bytes());
                        let _ = std::io::stdout().flush();
                    }
                    accumulator.push(event);
                }
                Err(e) => {
                    warn!(error = %e, "Error in streaming response");
                    return Err(HakimiError::Transport(format!("Stream error: {e}")));
                }
            }
        }
        // Print a newline after the stream completes.
        {
            use std::io::Write;
            let _ = std::io::stdout().write_all(b"\n");
            let _ = std::io::stdout().flush();
        }

        // Build a NormalizedResponse from the accumulated stream data.
        let response = accumulator_to_response(&accumulator);

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
            let tool_calls = response.tool_calls.unwrap();

            debug!(
                count = tool_calls.len(),
                "Processing tool calls (streaming)"
            );

            // Append the assistant message (with tool_calls) to history.
            let assistant_msg = build_assistant_message_with_tools(
                response.content.clone(),
                &tool_calls,
                response.reasoning.clone(),
            );
            agent.messages.push(assistant_msg);

            // Initialize guardrails for this turn.
            let mut guardrails = ToolGuardrails::new();

            // Dispatch each tool call and append results.
            for tc in &tool_calls {
                if agent.check_interrupt() {
                    info!("Interrupted during tool dispatch");
                    break;
                }

                // Check guardrails before dispatching
                let decision = guardrails.record_call(&tc.name, &tc.arguments);
                match decision {
                    GuardrailDecision::Halt(reason) => {
                        warn!(tool = %tc.name, reason = %reason, "Guardrails halted tool dispatch (streaming)");
                        agent.messages.push(Message::tool_result(
                            &tc.id,
                            &tc.name,
                            format!("HALT: Tool dispatch halted by guardrails: {reason}"),
                        ));
                        break;
                    }
                    GuardrailDecision::SyntheticResult(msg) => {
                        warn!(tool = %tc.name, "Injecting synthetic result to break loop (streaming)");
                        agent
                            .messages
                            .push(Message::tool_result(&tc.id, &tc.name, msg));
                        continue;
                    }
                    GuardrailDecision::Warn(msg) => {
                        warn!(tool = %tc.name, reason = %msg, "Guardrail warning (streaming)");
                    }
                    GuardrailDecision::Allow => {}
                }

                let tool_result = dispatch_tool(agent, &tool_ctx, tc).await;
                agent.messages.push(tool_result);
            }

            budget.use_one();
            continue;
        }

        // Text response — we're done.
        let final_text = response.content.unwrap_or_default();

        // Append the final assistant message to history.
        agent.messages.push(Message::assistant(&final_text));

        // Notify context engine that the session ended.
        {
            let mut engine = agent.context_engine.write().await;
            engine.on_session_end();
        }

        info!(
            api_calls = api_call_count,
            response_len = final_text.len(),
            "Streaming agent loop completed"
        );

        return Ok(ConversationResult {
            final_response: final_text,
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
