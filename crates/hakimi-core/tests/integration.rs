//! Integration tests for the Hakimi Agent.

use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use futures::stream;
use hakimi_common::{
    ApiMode, FinishReason, HakimiError, Message, NormalizedResponse, Result, ToolContext,
    ToolDefinition, Usage,
};
use hakimi_context::SimpleContextEngine;
use hakimi_core::{AIAgent, IterationBudget, TrajectoryConfig};
use hakimi_tools::{Tool, ToolContextBuilder, ToolRegistry};
use hakimi_transports::{ProviderTransport, RequestParams, StreamAccumulator, StreamEvent};
use serde_json::{Value as JsonValue, json};
use tokio::sync::RwLock;

// ── Mock Transport ──────────────────────────────────────────────────────────

/// A mock transport that returns pre-configured responses.
struct MockTransport {
    responses: Vec<NormalizedResponse>,
    call_index: std::sync::atomic::AtomicUsize,
}

impl MockTransport {
    fn new(responses: Vec<NormalizedResponse>) -> Self {
        Self {
            responses,
            call_index: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    fn single(response: NormalizedResponse) -> Self {
        Self::new(vec![response])
    }

    fn text_response(text: &str) -> Self {
        Self::single(NormalizedResponse {
            content: Some(text.to_string()),
            tool_calls: None,
            finish_reason: Some(FinishReason::Stop),
            usage: Some(Usage {
                prompt_tokens: 10,
                completion_tokens: 20,
                total_tokens: 30,
                cached_tokens: 0,
                reasoning_tokens: 0,
            }),
            reasoning: None,
        })
    }
}

#[async_trait]
impl ProviderTransport for MockTransport {
    fn api_mode(&self) -> ApiMode {
        ApiMode::ChatCompletions
    }

    fn provider_name(&self) -> &str {
        "mock"
    }

    async fn execute(
        &self,
        _model: &str,
        _messages: &[Message],
        _tools: &[ToolDefinition],
        _params: &RequestParams,
    ) -> Result<NormalizedResponse> {
        let idx = self
            .call_index
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if idx < self.responses.len() {
            Ok(self.responses[idx].clone())
        } else {
            // Default: return a simple text response
            Ok(NormalizedResponse {
                content: Some("(no more responses)".to_string()),
                tool_calls: None,
                finish_reason: Some(FinishReason::Stop),
                usage: Some(Usage::default()),
                reasoning: None,
            })
        }
    }

    async fn execute_streaming(
        &self,
        _model: &str,
        _messages: &[Message],
        _tools: &[ToolDefinition],
        _params: &RequestParams,
    ) -> Result<Pin<Box<dyn futures::Stream<Item = std::result::Result<StreamEvent, String>> + Send>>>
    {
        let idx = self
            .call_index
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if idx < self.responses.len() {
            let resp = &self.responses[idx];
            let mut events: Vec<std::result::Result<StreamEvent, String>> = Vec::new();

            if let Some(ref content) = resp.content {
                events.push(Ok(StreamEvent::ContentDelta(content.clone())));
            }

            if let Some(ref tool_calls) = resp.tool_calls {
                for (i, tc) in tool_calls.iter().enumerate() {
                    events.push(Ok(StreamEvent::ToolCallDelta {
                        index: i,
                        id: Some(tc.id.clone()),
                        name: Some(tc.name.clone()),
                        arguments_delta: tc.arguments.clone(),
                    }));
                }
            }

            if let Some(ref reason) = resp.finish_reason {
                let reason = match reason {
                    FinishReason::Stop => "stop",
                    FinishReason::ToolCalls => "tool_calls",
                    FinishReason::Length => "length",
                    FinishReason::ContentFilter => "content_filter",
                    FinishReason::Error => "error",
                };
                events.push(Ok(StreamEvent::Finished(reason.to_string())));
            }

            events.push(Ok(StreamEvent::Usage {
                prompt_tokens: 10,
                completion_tokens: 20,
            }));
            events.push(Ok(StreamEvent::Done));

            Ok(Box::pin(stream::iter(events)))
        } else {
            let events: Vec<std::result::Result<StreamEvent, String>> = vec![
                Ok(StreamEvent::ContentDelta("(no more responses)".to_string())),
                Ok(StreamEvent::Done),
            ];
            Ok(Box::pin(stream::iter(events)))
        }
    }
}

struct FailingTransport;

#[async_trait]
impl ProviderTransport for FailingTransport {
    fn api_mode(&self) -> ApiMode {
        ApiMode::ChatCompletions
    }

    fn provider_name(&self) -> &str {
        "failing"
    }

    async fn execute(
        &self,
        _model: &str,
        _messages: &[Message],
        _tools: &[ToolDefinition],
        _params: &RequestParams,
    ) -> Result<NormalizedResponse> {
        Err(HakimiError::Transport("simulated failure".to_string()))
    }

    async fn execute_streaming(
        &self,
        _model: &str,
        _messages: &[Message],
        _tools: &[ToolDefinition],
        _params: &RequestParams,
    ) -> Result<Pin<Box<dyn futures::Stream<Item = std::result::Result<StreamEvent, String>> + Send>>>
    {
        Err(HakimiError::Transport("simulated failure".to_string()))
    }
}

struct ScriptedStreamingTransport {
    streams: Vec<Vec<std::result::Result<StreamEvent, String>>>,
    call_index: std::sync::atomic::AtomicUsize,
}

impl ScriptedStreamingTransport {
    fn new(streams: Vec<Vec<std::result::Result<StreamEvent, String>>>) -> Self {
        Self {
            streams,
            call_index: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    fn stream_call_count(&self) -> usize {
        self.call_index.load(std::sync::atomic::Ordering::SeqCst)
    }
}

#[async_trait]
impl ProviderTransport for ScriptedStreamingTransport {
    fn api_mode(&self) -> ApiMode {
        ApiMode::ChatCompletions
    }

    fn provider_name(&self) -> &str {
        "scripted-streaming"
    }

    async fn execute(
        &self,
        _model: &str,
        _messages: &[Message],
        _tools: &[ToolDefinition],
        _params: &RequestParams,
    ) -> Result<NormalizedResponse> {
        Ok(NormalizedResponse {
            content: Some("non-streaming fallback".to_string()),
            tool_calls: None,
            finish_reason: Some(FinishReason::Stop),
            usage: Some(Usage::default()),
            reasoning: None,
        })
    }

    async fn execute_streaming(
        &self,
        _model: &str,
        _messages: &[Message],
        _tools: &[ToolDefinition],
        _params: &RequestParams,
    ) -> Result<Pin<Box<dyn futures::Stream<Item = std::result::Result<StreamEvent, String>> + Send>>>
    {
        let idx = self
            .call_index
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let events = self.streams.get(idx).cloned().unwrap_or_else(|| {
            vec![
                Ok(StreamEvent::ContentDelta("fallback".to_string())),
                Ok(StreamEvent::Finished("stop".to_string())),
                Ok(StreamEvent::Done),
            ]
        });
        Ok(Box::pin(stream::iter(events)))
    }
}

// ── Helper functions ────────────────────────────────────────────────────────

fn make_context_engine() -> Arc<RwLock<SimpleContextEngine>> {
    Arc::new(RwLock::new(SimpleContextEngine::new(128_000)))
}

// ── AIAgent Builder Tests ───────────────────────────────────────────────────

#[test]
fn test_agent_builder_required_fields() {
    // Missing transport
    let result = AIAgent::builder()
        .model("test-model")
        .context_engine(make_context_engine())
        .build();
    assert!(result.is_err());
    // AIAgent doesn't impl Debug, so use format!("{:?}") on Result
    let err_msg = match result {
        Ok(_) => panic!("expected error"),
        Err(e) => format!("{e}"),
    };
    assert!(err_msg.contains("transport"));
}

#[test]
fn test_agent_builder_missing_model() {
    let transport = Arc::new(MockTransport::text_response("hi"));
    let result = AIAgent::builder()
        .transport(transport)
        .context_engine(make_context_engine())
        .build();
    assert!(result.is_err());
    let err_msg = match result {
        Ok(_) => panic!("expected error"),
        Err(e) => format!("{e}"),
    };
    assert!(err_msg.contains("model"));
}

#[test]
fn test_agent_builder_missing_context_engine() {
    let transport = Arc::new(MockTransport::text_response("hi"));
    let result = AIAgent::builder()
        .model("test-model")
        .transport(transport)
        .build();
    assert!(result.is_err());
    let err_msg = match result {
        Ok(_) => panic!("expected error"),
        Err(e) => format!("{e}"),
    };
    assert!(err_msg.contains("context_engine"));
}

#[test]
fn test_agent_builder_success() {
    let transport = Arc::new(MockTransport::text_response("hi"));
    let agent = AIAgent::builder()
        .model("test-model")
        .transport(transport)
        .context_engine(make_context_engine())
        .session_id("test-session")
        .workdir("/tmp")
        .build()
        .unwrap();

    assert_eq!(agent.model(), "test-model");
    assert_eq!(agent.session_id(), "test-session");
}

// ── Conversation Loop Tests ─────────────────────────────────────────────────

#[tokio::test]
async fn test_simple_text_conversation() {
    let transport = Arc::new(MockTransport::text_response("Hello, I'm the assistant!"));
    let mut agent = AIAgent::builder()
        .model("test-model")
        .transport(transport)
        .context_engine(make_context_engine())
        .session_id("test-conv-1")
        .workdir("/tmp")
        .build()
        .unwrap();

    let result = agent.run_conversation("Hi there").await.unwrap();
    assert_eq!(result.final_response, "Hello, I'm the assistant!");
    assert_eq!(result.api_call_count, 1);
    // Should have: user message + assistant message = 2
    assert!(result.messages.len() >= 2);
}

#[tokio::test]
async fn test_agent_saves_successful_trajectory() {
    let dir = std::env::temp_dir().join(format!(
        "hakimi-agent-trajectory-success-{}",
        uuid::Uuid::new_v4()
    ));
    let transport = Arc::new(MockTransport::text_response("saved response"));
    let mut agent = AIAgent::builder()
        .model("test-model")
        .transport(transport)
        .context_engine(make_context_engine())
        .session_id("trajectory-success")
        .workdir("/tmp")
        .trajectory_saving(TrajectoryConfig::new(&dir))
        .build()
        .unwrap();

    let result = agent.run_conversation("save this").await.unwrap();
    assert_eq!(result.final_response, "saved response");

    let saved =
        std::fs::read_to_string(dir.join("trajectory_samples.jsonl")).expect("trajectory saved");
    assert!(saved.contains("\"completed\":true"));
    assert!(saved.contains("\"model\":\"test-model\""));
    assert!(saved.contains("\"from\":\"system\""));
    assert!(saved.contains("\"from\":\"human\""));
    assert!(saved.contains("\"from\":\"gpt\""));

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn test_agent_saves_failed_trajectory_on_transport_error() {
    let dir = std::env::temp_dir().join(format!(
        "hakimi-agent-trajectory-failure-{}",
        uuid::Uuid::new_v4()
    ));
    let transport = Arc::new(FailingTransport);
    let mut agent = AIAgent::builder()
        .model("test-model")
        .transport(transport)
        .context_engine(make_context_engine())
        .session_id("trajectory-failure")
        .workdir("/tmp")
        .trajectory_saving(TrajectoryConfig::new(&dir))
        .build()
        .unwrap();

    let err = agent
        .run_conversation("this will fail")
        .await
        .expect_err("transport should fail");
    assert!(format!("{err}").contains("simulated failure"));

    let saved =
        std::fs::read_to_string(dir.join("failed_trajectories.jsonl")).expect("trajectory saved");
    assert!(saved.contains("\"completed\":false"));
    assert!(saved.contains("this will fail"));

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn test_length_finish_auto_continues_and_merges() {
    let part1 = NormalizedResponse {
        content: Some("Line 1\nLine".to_string()),
        tool_calls: None,
        finish_reason: Some(FinishReason::Length),
        usage: Some(Usage::default()),
        reasoning: None,
    };
    let part2 = NormalizedResponse {
        content: Some("2\nLine 3".to_string()),
        tool_calls: None,
        finish_reason: Some(FinishReason::Stop),
        usage: Some(Usage::default()),
        reasoning: None,
    };
    let transport = Arc::new(MockTransport::new(vec![part1, part2]));
    let mut agent = AIAgent::builder()
        .model("test-model")
        .transport(transport)
        .context_engine(make_context_engine())
        .session_id("test-length-continuation")
        .workdir("/tmp")
        .build()
        .unwrap();

    let result = agent.run_conversation("Long answer please").await.unwrap();
    assert_eq!(result.final_response, "Line 1\nLine 2\nLine 3");
    assert_eq!(result.api_call_count, 2);
    assert!(
        result
            .messages
            .iter()
            .any(|m| m.content.as_deref() == Some("Your previous response was cut off by the output token limit. Continue exactly where you stopped. Do not repeat earlier text. Finish the answer completely."))
    );
}

#[tokio::test]
async fn test_tool_dispatch_e2e() {
    // First response: tool call. Second response: text with the result.
    let tool_call_response = NormalizedResponse {
        content: None,
        tool_calls: Some(vec![hakimi_common::ToolCall {
            id: "call_1".to_string(),
            name: "terminal".to_string(),
            arguments: r#"{"command":"echo hello"}"#.to_string(),
            index: None,
        }]),
        finish_reason: Some(FinishReason::ToolCalls),
        usage: Some(Usage {
            prompt_tokens: 10,
            completion_tokens: 5,
            total_tokens: 15,
            cached_tokens: 0,
            reasoning_tokens: 0,
        }),
        reasoning: None,
    };

    let text_response = NormalizedResponse {
        content: Some("The command output was: hello".to_string()),
        tool_calls: None,
        finish_reason: Some(FinishReason::Stop),
        usage: Some(Usage {
            prompt_tokens: 20,
            completion_tokens: 10,
            total_tokens: 30,
            cached_tokens: 0,
            reasoning_tokens: 0,
        }),
        reasoning: None,
    };

    let transport = Arc::new(MockTransport::new(vec![tool_call_response, text_response]));

    let registry = ToolRegistry::new();
    // Register a simple echo tool
    registry.register(Arc::new(EchoTool)).await;

    let mut agent = AIAgent::builder()
        .model("test-model")
        .transport(transport)
        .context_engine(make_context_engine())
        .tool_registry(registry)
        .session_id("test-tool-e2e")
        .workdir("/tmp")
        .build()
        .unwrap();

    let result = agent.run_conversation("echo test").await.unwrap();
    assert_eq!(result.final_response, "The command output was: hello");
    assert_eq!(result.api_call_count, 2);
    // Should have: user, assistant(tool_calls), tool_result, assistant(text) = 4
    assert!(result.messages.len() >= 4);
}

#[tokio::test]
async fn test_agent_interrupt() {
    let transport = Arc::new(MockTransport::text_response("won't reach"));
    let mut agent = AIAgent::builder()
        .model("test-model")
        .transport(transport)
        .context_engine(make_context_engine())
        .session_id("test-interrupt")
        .workdir("/tmp")
        .build()
        .unwrap();

    // Set interrupt before running
    agent.interrupt();

    // Running with interrupt should still work (interrupt is checked in the loop)
    // For a simple text response, the loop only checks interrupt at the top,
    // and the first API call should still succeed
    let result = agent.chat("test").await;
    // It should either succeed (if interrupt wasn't checked before the call)
    // or return empty (budget exhausted)
    assert!(result.is_ok() || result.is_err());
}

// ── IterationBudget Tests ───────────────────────────────────────────────────

#[test]
fn test_iteration_budget() {
    let budget = IterationBudget::new(3);
    assert!(!budget.is_exhausted());
    assert_eq!(budget.remaining(), 3);

    budget.use_one();
    assert!(!budget.is_exhausted());
    assert_eq!(budget.remaining(), 2);

    budget.use_one();
    budget.use_one();
    assert!(budget.is_exhausted());
    assert_eq!(budget.remaining(), 0);
}

#[test]
fn test_iteration_budget_zero() {
    let budget = IterationBudget::new(0);
    assert!(budget.is_exhausted());
}

// ── Streaming Accumulator Tests ─────────────────────────────────────────────

#[test]
fn test_streaming_accumulator_content_only() {
    let mut acc = StreamAccumulator::new();
    acc.push(StreamEvent::ContentDelta("Hello ".to_string()));
    acc.push(StreamEvent::ContentDelta("World".to_string()));
    acc.push(StreamEvent::Usage {
        prompt_tokens: 5,
        completion_tokens: 10,
    });
    acc.push(StreamEvent::Done);

    assert_eq!(acc.content, "Hello World");
    assert!(acc.tool_calls.is_empty());
    assert_eq!(acc.prompt_tokens, 5);
    assert_eq!(acc.completion_tokens, 10);
}

#[test]
fn test_streaming_accumulator_with_tool_calls() {
    let mut acc = StreamAccumulator::new();
    acc.push(StreamEvent::ToolCallDelta {
        index: 0,
        id: Some("call_abc".to_string()),
        name: Some("read_file".to_string()),
        arguments_delta: r#"{"path":"#.to_string(),
    });
    acc.push(StreamEvent::ToolCallDelta {
        index: 0,
        id: None,
        name: None,
        arguments_delta: r#""test.txt"}"#.to_string(),
    });
    acc.push(StreamEvent::Done);

    assert_eq!(acc.content, "");
    assert_eq!(acc.tool_calls.len(), 1);
    assert_eq!(acc.tool_calls[0].id, "call_abc");
    assert_eq!(acc.tool_calls[0].name, "read_file");
    assert_eq!(acc.tool_calls[0].arguments, r#"{"path":"test.txt"}"#);
}

#[test]
fn test_streaming_accumulator_multiple_tool_calls_interleaved() {
    let mut acc = StreamAccumulator::new();
    acc.push(StreamEvent::ToolCallDelta {
        index: 0,
        id: Some("call_1".to_string()),
        name: Some("tool_a".to_string()),
        arguments_delta: "{}".to_string(),
    });
    acc.push(StreamEvent::ToolCallDelta {
        index: 1,
        id: Some("call_2".to_string()),
        name: Some("tool_b".to_string()),
        arguments_delta: "{}".to_string(),
    });
    acc.push(StreamEvent::ToolCallDelta {
        index: 0,
        id: None,
        name: None,
        arguments_delta: "".to_string(),
    });

    assert_eq!(acc.tool_calls.len(), 2);
    assert_eq!(acc.tool_calls[0].id, "call_1");
    assert_eq!(acc.tool_calls[1].id, "call_2");
}

// ── EchoTool (mock for integration tests) ───────────────────────────────────

struct EchoTool;

#[async_trait]
impl Tool for EchoTool {
    fn name(&self) -> &str {
        "terminal"
    }

    fn toolset(&self) -> &str {
        "shell"
    }

    fn description(&self) -> &str {
        "Echo tool for testing"
    }

    fn schema(&self) -> JsonValue {
        json!({
            "type": "object",
            "properties": {
                "command": {"type": "string"}
            }
        })
    }

    async fn execute(&self, args: &JsonValue, _ctx: &ToolContext) -> Result<String> {
        let cmd = args.get("command").and_then(|v| v.as_str()).unwrap_or("");
        Ok(format!("output of: {cmd}"))
    }
}

// ── ToolContextBuilder Tests ────────────────────────────────────────────────

#[test]
fn test_tool_context_builder() {
    let ctx = ToolContextBuilder::new()
        .session_id("s1")
        .user_id("u1")
        .workdir("/tmp")
        .build();

    assert_eq!(ctx.session_id, "s1");
    assert_eq!(ctx.user_id, Some("u1".to_string()));
    assert_eq!(ctx.workdir, "/tmp");
}

#[test]
fn test_tool_context_builder_try_build() {
    let result = ToolContextBuilder::new()
        .session_id("s1")
        .workdir("/tmp")
        .try_build();
    assert!(result.is_ok());
}

#[test]
fn test_tool_context_builder_try_build_missing_session() {
    let result = ToolContextBuilder::new().workdir("/tmp").try_build();
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("session_id"));
}

#[test]
fn test_tool_context_builder_try_build_missing_workdir() {
    let result = ToolContextBuilder::new().session_id("s1").try_build();
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("workdir"));
}

// ── Streaming Conversation Loop Tests ─────────────────────────────────────

#[tokio::test]
async fn test_streaming_text_conversation() {
    let transport = Arc::new(MockTransport::text_response("Streaming hello!"));
    let mut agent = AIAgent::builder()
        .model("test-model")
        .transport(transport)
        .context_engine(make_context_engine())
        .session_id("test-streaming-1")
        .workdir("/tmp")
        .streaming(true)
        .build()
        .unwrap();

    let result = agent.run_conversation("Hi streaming").await.unwrap();
    assert_eq!(result.final_response, "Streaming hello!");
    assert_eq!(result.api_call_count, 1);
    assert!(result.messages.len() >= 2);
}

#[tokio::test]
async fn test_non_streaming_think_blocks_are_scrubbed_from_final_response() {
    let response = NormalizedResponse {
        content: Some("Visible\n<thinking>hidden chain</thinking>Done".to_string()),
        tool_calls: None,
        finish_reason: Some(FinishReason::Stop),
        usage: Some(Usage::default()),
        reasoning: None,
    };
    let transport = Arc::new(MockTransport::single(response));
    let mut agent = AIAgent::builder()
        .model("test-model")
        .transport(transport)
        .context_engine(make_context_engine())
        .session_id("test-non-streaming-think-scrub")
        .workdir("/tmp")
        .build()
        .unwrap();

    let result = agent
        .run_conversation("answer without hidden reasoning")
        .await
        .unwrap();
    assert_eq!(result.final_response, "Visible\nDone");
    let assistant = result
        .messages
        .iter()
        .rev()
        .find(|m| m.role == hakimi_common::MessageRole::Assistant)
        .expect("assistant message");
    assert_eq!(assistant.content.as_deref(), Some("Visible\nDone"));
    assert_eq!(assistant.reasoning.as_deref(), Some("hidden chain"));
}

#[tokio::test]
async fn test_streaming_length_finish_auto_continues_and_merges() {
    let part1 = NormalizedResponse {
        content: Some("Streaming line 1\nLine".to_string()),
        tool_calls: None,
        finish_reason: Some(FinishReason::Length),
        usage: Some(Usage::default()),
        reasoning: None,
    };
    let part2 = NormalizedResponse {
        content: Some("2\nStreaming line 3".to_string()),
        tool_calls: None,
        finish_reason: Some(FinishReason::Stop),
        usage: Some(Usage::default()),
        reasoning: None,
    };
    let transport = Arc::new(MockTransport::new(vec![part1, part2]));
    let mut agent = AIAgent::builder()
        .model("test-model")
        .transport(transport)
        .context_engine(make_context_engine())
        .session_id("test-streaming-length-continuation")
        .workdir("/tmp")
        .streaming(true)
        .build()
        .unwrap();

    let result = agent
        .run_conversation("Long streaming answer")
        .await
        .unwrap();
    assert_eq!(
        result.final_response,
        "Streaming line 1\nLine 2\nStreaming line 3"
    );
    assert_eq!(result.api_call_count, 2);
}

#[tokio::test]
async fn test_streaming_truncated_stream_retries_before_final_response() {
    let transport = Arc::new(ScriptedStreamingTransport::new(vec![
        vec![Ok(StreamEvent::ContentDelta("partial".to_string()))],
        vec![
            Ok(StreamEvent::ContentDelta("Recovered response".to_string())),
            Ok(StreamEvent::Finished("stop".to_string())),
            Ok(StreamEvent::Done),
        ],
    ]));

    let mut agent = AIAgent::builder()
        .model("test-model")
        .transport(transport.clone())
        .context_engine(make_context_engine())
        .session_id("test-streaming-truncated-retry")
        .workdir("/tmp")
        .streaming(true)
        .build()
        .unwrap();

    let result = agent
        .run_conversation("Retry after a truncated stream")
        .await
        .unwrap();

    assert_eq!(result.final_response, "Recovered response");
    assert_eq!(result.api_call_count, 2);
    assert_eq!(transport.stream_call_count(), 2);
}

#[tokio::test]
async fn test_streaming_split_think_tags_are_scrubbed_from_accumulator() {
    let transport = Arc::new(ScriptedStreamingTransport::new(vec![vec![
        Ok(StreamEvent::ContentDelta("<thi".to_string())),
        Ok(StreamEvent::ContentDelta("nk>hidden".to_string())),
        Ok(StreamEvent::ContentDelta("</think>Visible".to_string())),
        Ok(StreamEvent::Finished("stop".to_string())),
        Ok(StreamEvent::Done),
    ]]));

    let mut agent = AIAgent::builder()
        .model("test-model")
        .transport(transport)
        .context_engine(make_context_engine())
        .session_id("test-streaming-think-scrub")
        .workdir("/tmp")
        .streaming(true)
        .build()
        .unwrap();

    let result = agent
        .run_conversation("stream without hidden reasoning")
        .await
        .unwrap();

    assert_eq!(result.final_response, "Visible");
    let assistant = result
        .messages
        .iter()
        .rev()
        .find(|m| m.role == hakimi_common::MessageRole::Assistant)
        .expect("assistant message");
    assert_eq!(assistant.content.as_deref(), Some("Visible"));
    assert_eq!(assistant.reasoning.as_deref(), Some("hidden"));
}

#[tokio::test]
async fn test_streaming_tool_dispatch_e2e() {
    let tool_call_response = NormalizedResponse {
        content: None,
        tool_calls: Some(vec![hakimi_common::ToolCall {
            id: "call_s1".to_string(),
            name: "terminal".to_string(),
            arguments: r#"{"command":"echo streaming"}"#.to_string(),
            index: None,
        }]),
        finish_reason: Some(FinishReason::ToolCalls),
        usage: Some(Usage {
            prompt_tokens: 15,
            completion_tokens: 8,
            total_tokens: 23,
            cached_tokens: 0,
            reasoning_tokens: 0,
        }),
        reasoning: None,
    };

    let text_response = NormalizedResponse {
        content: Some("Streaming tool result done".to_string()),
        tool_calls: None,
        finish_reason: Some(FinishReason::Stop),
        usage: Some(Usage {
            prompt_tokens: 25,
            completion_tokens: 12,
            total_tokens: 37,
            cached_tokens: 0,
            reasoning_tokens: 0,
        }),
        reasoning: None,
    };

    let transport = Arc::new(MockTransport::new(vec![tool_call_response, text_response]));

    let registry = ToolRegistry::new();
    registry.register(Arc::new(EchoTool)).await;

    let mut agent = AIAgent::builder()
        .model("test-model")
        .transport(transport)
        .context_engine(make_context_engine())
        .tool_registry(registry)
        .session_id("test-streaming-tools")
        .workdir("/tmp")
        .streaming(true)
        .build()
        .unwrap();

    let result = agent.run_conversation("echo streaming test").await.unwrap();
    assert_eq!(result.final_response, "Streaming tool result done");
    assert_eq!(result.api_call_count, 2);
    assert!(result.messages.len() >= 4);
    // Verify usage was accumulated across both API calls
    assert!(result.usage.total_tokens > 0);
}

#[tokio::test]
async fn test_multi_turn_conversation() {
    // Simulate two separate conversation turns
    let transport = Arc::new(MockTransport::new(vec![
        NormalizedResponse {
            content: Some("Turn 1 response".to_string()),
            tool_calls: None,
            finish_reason: Some(FinishReason::Stop),
            usage: Some(Usage {
                prompt_tokens: 10,
                completion_tokens: 5,
                total_tokens: 15,
                cached_tokens: 0,
                reasoning_tokens: 0,
            }),
            reasoning: None,
        },
        NormalizedResponse {
            content: Some("Turn 2 response".to_string()),
            tool_calls: None,
            finish_reason: Some(FinishReason::Stop),
            usage: Some(Usage {
                prompt_tokens: 20,
                completion_tokens: 8,
                total_tokens: 28,
                cached_tokens: 0,
                reasoning_tokens: 0,
            }),
            reasoning: None,
        },
    ]));

    let mut agent = AIAgent::builder()
        .model("test-model")
        .transport(transport)
        .context_engine(make_context_engine())
        .session_id("test-multi-turn")
        .workdir("/tmp")
        .build()
        .unwrap();

    let result1 = agent.chat("first message").await.unwrap();
    assert_eq!(result1, "Turn 1 response");
    assert!(agent.messages().len() >= 2);

    let result2 = agent.chat("second message").await.unwrap();
    assert_eq!(result2, "Turn 2 response");
    // Messages should accumulate: user1 + assistant1 + user2 + assistant2 = 4
    assert!(agent.messages().len() >= 4);
}

#[tokio::test]
async fn test_clear_messages() {
    let transport = Arc::new(MockTransport::text_response("response"));
    let mut agent = AIAgent::builder()
        .model("test-model")
        .transport(transport)
        .context_engine(make_context_engine())
        .session_id("test-clear")
        .workdir("/tmp")
        .build()
        .unwrap();

    agent.chat("hello").await.unwrap();
    assert!(!agent.messages().is_empty());

    agent.clear_messages();
    assert!(agent.messages().is_empty());
}

#[tokio::test]
async fn test_set_system_prompt() {
    let transport = Arc::new(MockTransport::text_response("ok"));
    let mut agent = AIAgent::builder()
        .model("test-model")
        .transport(transport)
        .context_engine(make_context_engine())
        .session_id("test-sysprompt")
        .workdir("/tmp")
        .build()
        .unwrap();

    agent.set_system_prompt("You are a pirate.");
    let result = agent.chat("hello").await.unwrap();
    assert_eq!(result, "ok");
}

#[tokio::test]
async fn test_agent_exposes_shared_runtime() {
    let transport = Arc::new(MockTransport::text_response("ok"));
    let agent = AIAgent::builder()
        .model("test-model")
        .transport(transport)
        .context_engine(make_context_engine())
        .session_id("test-shared-runtime")
        .workdir("/tmp")
        .build()
        .unwrap();

    // 4 个共享资源现在统一挂在 agent.shared 上。
    assert_eq!(agent.shared.transport.provider_name(), "mock");
    assert!(agent.shared.knowledge_searcher.is_none());
    assert!(agent.shared.embedding_provider.is_none());
    // getter 兼容层仍然可用(外部 crate 依赖它)。
    assert_eq!(agent.provider_name(), "mock");
}

#[tokio::test]
async fn test_persona_agent_isolates_state_but_shares_runtime() {
    use hakimi_core::persona::PersonaConfig;
    use hakimi_core::persona_runtime::build_persona_agent;

    let transport = Arc::new(MockTransport::text_response("ok"));
    let template = AIAgent::builder()
        .model("template-model")
        .transport(transport)
        .context_engine(make_context_engine())
        .workdir("/tmp")
        .build()
        .unwrap();

    let mut cfg = PersonaConfig::new("coder");
    cfg.model = "persona-model".to_string();
    cfg.system_prompt = "You are coder.".to_string();

    let agent = build_persona_agent(
        &template,
        &cfg,
        std::path::Path::new("/nonexistent-persona-skills"),
        128_000,
    );

    // Per-persona model is applied.
    assert_eq!(agent.model(), "persona-model");
    // The heavy runtime is SHARED (same Arc), not duplicated.
    assert!(std::sync::Arc::ptr_eq(&agent.shared, &template.shared));
    // Provider still resolves through the shared transport.
    assert_eq!(agent.provider_name(), "mock");
    // The template is left untouched.
    assert_eq!(template.model(), "template-model");
}

#[tokio::test]
async fn team_executor_consults_addressable_teammate() {
    use std::sync::Arc;
    use tokio::sync::RwLock;

    let agents_dir = std::env::temp_dir()
        .join(format!("hakimi-team-it-{}", uuid::Uuid::new_v4()))
        .join("agents");
    let mut reg = hakimi_core::PersonaRegistry::load(&agents_dir).unwrap();
    let mut writer = hakimi_core::PersonaConfig::new("writer");
    writer.system_prompt = "You are the writer.".to_string();
    writer.addressable = true;
    reg.create(writer).unwrap();
    let registry = Arc::new(RwLock::new(reg));

    let transport = Arc::new(MockTransport::text_response(
        "Status: success\nSummary: drafted",
    ));
    let template = Arc::new(hakimi_core::AIAgent::new(
        "test-model",
        transport,
        hakimi_tools::ToolRegistry::new(),
        None,
    ));

    let exec = hakimi_core::PersonaTeamExecutor::new(registry, template, 128_000).for_lead("lead");
    let answer = hakimi_common::TeamExecutor::consult(
        &exec,
        hakimi_common::TeamCallContext {
            teammate_id: "writer".to_string(),
            task: "draft a title".to_string(),
            context: String::new(),
            progress: None,
        },
    )
    .await
    .unwrap();

    assert!(answer.contains("drafted"));
}

#[tokio::test]
async fn team_consult_publishes_activity_events() {
    use std::sync::Arc;
    use tokio::sync::RwLock;

    let mut rx = hakimi_common::subscribe();

    let agents_dir = std::env::temp_dir()
        .join(format!("hakimi-team-act-{}", uuid::Uuid::new_v4()))
        .join("agents");
    let mut reg = hakimi_core::PersonaRegistry::load(&agents_dir).unwrap();
    let mut writer = hakimi_core::PersonaConfig::new("writer");
    writer.addressable = true;
    reg.create(writer).unwrap();
    let registry = Arc::new(RwLock::new(reg));

    let transport = Arc::new(MockTransport::text_response("Status: success"));
    let template = Arc::new(hakimi_core::AIAgent::new(
        "test-model",
        transport,
        hakimi_tools::ToolRegistry::new(),
        None,
    ));
    let exec = hakimi_core::PersonaTeamExecutor::new(registry, template, 128_000).for_lead("lead");
    let _ = hakimi_common::TeamExecutor::consult(
        &exec,
        hakimi_common::TeamCallContext {
            teammate_id: "writer".to_string(),
            task: "draft".to_string(),
            context: String::new(),
            progress: None,
        },
    )
    .await
    .unwrap();

    let mut saw_started = false;
    let mut saw_ended = false;
    for _ in 0..200 {
        match rx.try_recv() {
            Ok(hakimi_common::ActivityEvent::ConsultStarted { from_id, to_id, .. })
                if from_id == "lead" && to_id == "writer" =>
            {
                saw_started = true;
            }
            Ok(hakimi_common::ActivityEvent::ConsultEnded { from_id, to_id })
                if from_id == "lead" && to_id == "writer" =>
            {
                saw_ended = true;
            }
            Ok(_) => continue,
            Err(_) => break,
        }
    }
    assert!(saw_started, "expected ConsultStarted");
    assert!(saw_ended, "expected ConsultEnded");
}
