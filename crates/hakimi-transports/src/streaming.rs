use futures::stream::Stream;
use futures::task::{Context, Poll};
use serde_json::Value as JsonValue;
use std::pin::Pin;

/// A parsed SSE event from an LLM streaming response.
#[derive(Debug, Clone)]
pub enum StreamEvent {
    /// A delta of text content.
    ContentDelta(String),
    /// A tool call being built incrementally.
    ToolCallDelta {
        index: usize,
        id: Option<String>,
        name: Option<String>,
        arguments_delta: String,
    },
    /// Usage information (usually sent at the end).
    Usage {
        prompt_tokens: u32,
        completion_tokens: u32,
    },
    /// A delta of reasoning content (reasoning models like DeepSeek R1, QwQ).
    ReasoningDelta(String),
    /// Stream finished with a provider finish reason such as `stop`, `length`,
    /// `tool_calls`, or `max_tokens`.
    Finished(String),
    /// Stream finished.
    Done,
}

/// Accumulates streaming deltas into a complete response.
#[derive(Debug, Default)]
pub struct StreamAccumulator {
    pub content: String,
    pub reasoning: String,
    pub tool_calls: Vec<AccumulatedToolCall>,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Default, Clone)]
pub struct AccumulatedToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

impl StreamAccumulator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, event: StreamEvent) {
        match event {
            StreamEvent::ContentDelta(text) => self.content.push_str(&text),
            StreamEvent::ReasoningDelta(text) => self.reasoning.push_str(&text),
            StreamEvent::ToolCallDelta {
                index,
                id,
                name,
                arguments_delta,
            } => {
                while self.tool_calls.len() <= index {
                    self.tool_calls.push(AccumulatedToolCall::default());
                }
                let tc = &mut self.tool_calls[index];
                if let Some(id) = id {
                    tc.id = id;
                }
                if let Some(name) = name {
                    tc.name = name;
                }
                tc.arguments.push_str(&arguments_delta);
            }
            StreamEvent::Usage {
                prompt_tokens,
                completion_tokens,
            } => {
                self.prompt_tokens = prompt_tokens;
                self.completion_tokens = completion_tokens;
            }
            StreamEvent::Finished(reason) => {
                self.finish_reason = Some(reason);
            }
            StreamEvent::Done => {}
        }
    }
}

// ── SSE line buffer helpers ─────────────────────────────────────────────────

fn strip_prefix<'a>(line: &'a [u8], prefix: &[u8]) -> Option<&'a [u8]> {
    if line.len() >= prefix.len() && &line[..prefix.len()] == prefix {
        Some(&line[prefix.len()..])
    } else {
        None
    }
}

// ── OpenAI SSE parsing ──────────────────────────────────────────────────────

/// Parse an OpenAI-format streaming chunk JSON into a list of `StreamEvent`s.
///
/// OpenAI format: `choices[0].delta.{content, tool_calls}`.
pub fn parse_openai_chunk(json_str: &str) -> Vec<StreamEvent> {
    if json_str.trim() == "[DONE]" {
        return vec![StreamEvent::Done];
    }

    let parsed: Result<JsonValue, _> = serde_json::from_str(json_str);
    let Ok(val) = parsed else {
        return vec![];
    };

    let mut events = Vec::new();

    if let Some(choices) = val["choices"].as_array() {
        for choice in choices {
            let delta = &choice["delta"];

            // Content delta.
            if let Some(content) = delta["content"].as_str()
                && !content.is_empty()
            {
                events.push(StreamEvent::ContentDelta(content.to_string()));
            }

            // Reasoning content delta (reasoning models: DeepSeek R1, QwQ, etc.).
            if let Some(reasoning) = delta["reasoning_content"].as_str()
                && !reasoning.is_empty()
            {
                events.push(StreamEvent::ReasoningDelta(reasoning.to_string()));
            }

            // Tool call deltas.
            if let Some(tool_calls) = delta["tool_calls"].as_array() {
                for tc in tool_calls {
                    let index = tc["index"].as_u64().unwrap_or(0) as usize;
                    let id = tc["id"].as_str().map(String::from);
                    let name = tc["function"]["name"].as_str().map(String::from);
                    let arguments_delta = tc["function"]["arguments"]
                        .as_str()
                        .unwrap_or("")
                        .to_string();

                    events.push(StreamEvent::ToolCallDelta {
                        index,
                        id,
                        name,
                        arguments_delta,
                    });
                }
            }

            if let Some(reason) = choice["finish_reason"].as_str()
                && !reason.is_empty()
            {
                events.push(StreamEvent::Finished(reason.to_string()));
            }
        }
    }

    // Some providers send usage at the top level even with empty choices.
    if let Some(usage) = val.get("usage")
        && let (Some(prompt), Some(completion)) = (
            usage["prompt_tokens"].as_u64(),
            usage["completion_tokens"].as_u64(),
        )
    {
        events.push(StreamEvent::Usage {
            prompt_tokens: prompt as u32,
            completion_tokens: completion as u32,
        });
    }

    events
}

// ── Anthropic SSE parsing ───────────────────────────────────────────────────

// ── Gemini SSE parsing ──────────────────────────────────────────────────────

/// Parse a Gemini-format streaming chunk JSON into a list of `StreamEvent`s.
///
/// Gemini format: `candidates[0].content.parts[].{text, functionCall}`,
/// `usageMetadata.{promptTokenCount, candidatesTokenCount}`.
pub fn parse_gemini_chunk(json_str: &str) -> Vec<StreamEvent> {
    let parsed: Result<JsonValue, _> = serde_json::from_str(json_str);
    let Ok(val) = parsed else {
        return vec![];
    };

    let mut events = Vec::new();

    if let Some(candidates) = val["candidates"].as_array() {
        for candidate in candidates {
            if let Some(parts) = candidate["content"]["parts"].as_array() {
                for part in parts {
                    // Text content delta.
                    if let Some(text) = part["text"].as_str()
                        && !text.is_empty()
                    {
                        events.push(StreamEvent::ContentDelta(text.to_string()));
                    }

                    // Function call deltas.
                    if let Some(fc) = part.get("functionCall") {
                        let name = fc["name"].as_str().map(String::from);
                        let args = fc
                            .get("args")
                            .cloned()
                            .unwrap_or(JsonValue::Object(serde_json::Map::new()));
                        let arguments_str =
                            serde_json::to_string(&args).unwrap_or_else(|_| "{}".to_string());
                        events.push(StreamEvent::ToolCallDelta {
                            index: 0, // Gemini sends complete function calls, not streamed deltas
                            id: None,
                            name,
                            arguments_delta: arguments_str,
                        });
                    }
                }
            }

            if let Some(reason @ ("STOP" | "MAX_TOKENS" | "SAFETY" | "RECITATION" | "OTHER")) =
                candidate["finishReason"].as_str()
            {
                events.push(StreamEvent::Finished(reason.to_string()));
                events.push(StreamEvent::Done);
            }
        }
    }

    // Usage metadata.
    if let Some(usage) = val.get("usageMetadata") {
        let prompt_tokens = usage["promptTokenCount"].as_u64().unwrap_or(0) as u32;
        let completion_tokens = usage["candidatesTokenCount"].as_u64().unwrap_or(0) as u32;
        if prompt_tokens > 0 || completion_tokens > 0 {
            events.push(StreamEvent::Usage {
                prompt_tokens,
                completion_tokens,
            });
        }
    }

    events
}

/// Parse a Gemini SSE event. Gemini doesn't use typed event lines,
/// so the event_type is ignored and we delegate to `parse_gemini_chunk`.
pub fn parse_gemini_event(_event_type: &str, json_str: &str) -> Vec<StreamEvent> {
    parse_gemini_chunk(json_str)
}

/// Parse an Anthropic SSE event by event type and JSON data.
///
/// Anthropic uses typed events: `message_start`, `content_block_start`,
/// `content_block_delta`, `message_delta`, `message_stop`, etc.
pub fn parse_anthropic_event(event_type: &str, json_str: &str) -> Vec<StreamEvent> {
    let parsed: Result<JsonValue, _> = serde_json::from_str(json_str);
    let Ok(val) = parsed else {
        return vec![];
    };

    let mut events = Vec::new();

    match event_type {
        "content_block_delta" => {
            let delta = &val["delta"];
            match delta["type"].as_str() {
                Some("text_delta") => {
                    if let Some(text) = delta["text"].as_str()
                        && !text.is_empty()
                    {
                        events.push(StreamEvent::ContentDelta(text.to_string()));
                    }
                }
                Some("input_json_delta") => {
                    if let Some(json_fragment) = delta["partial_json"].as_str() {
                        let index = val["index"].as_u64().unwrap_or(0) as usize;
                        events.push(StreamEvent::ToolCallDelta {
                            index,
                            id: None,
                            name: None,
                            arguments_delta: json_fragment.to_string(),
                        });
                    }
                }
                _ => {}
            }
        }
        "content_block_start" => {
            let block = &val["content_block"];
            if block["type"].as_str() == Some("tool_use") {
                let index = val["index"].as_u64().unwrap_or(0) as usize;
                let id = block["id"].as_str().map(String::from);
                let name = block["name"].as_str().map(String::from);
                events.push(StreamEvent::ToolCallDelta {
                    index,
                    id,
                    name,
                    arguments_delta: String::new(),
                });
            }
        }
        "message_delta" => {
            if let Some(reason) = val["delta"]["stop_reason"].as_str() {
                events.push(StreamEvent::Finished(reason.to_string()));
            }
            let usage = &val["usage"];
            if let Some(output_tokens) = usage["output_tokens"].as_u64() {
                events.push(StreamEvent::Usage {
                    prompt_tokens: 0, // Provided in message_start
                    completion_tokens: output_tokens as u32,
                });
            }
        }
        "message_start" => {
            let usage = &val["message"]["usage"];
            if let Some(input_tokens) = usage["input_tokens"].as_u64() {
                events.push(StreamEvent::Usage {
                    prompt_tokens: input_tokens as u32,
                    completion_tokens: 0,
                });
            }
        }
        "message_stop" => {
            events.push(StreamEvent::Done);
        }
        _ => {
            // Ignore ping, error, and other event types.
        }
    }

    events
}

/// Parse an Anthropic SSE data payload by inferring the event type from the JSON `"type"` field.
#[cfg(test)]
fn parse_anthropic_chunk(json_str: &str) -> Vec<StreamEvent> {
    let parsed: Result<JsonValue, _> = serde_json::from_str(json_str);
    let Ok(val) = parsed else {
        return vec![];
    };

    let event_type = val["type"].as_str().unwrap_or("");
    parse_anthropic_event(event_type, json_str)
}

// ── Full SSE buffer (supports both OpenAI and Anthropic) ────────────────────

/// Extended SSE buffer that captures both `event:` and `data:` lines,
/// yielding `(Option<String>, String)` pairs of (event_type, data_payload).
///
/// This is needed for Anthropic's SSE format where event types are on
/// separate `event:` lines preceding the `data:` lines.
#[derive(Debug, Default)]
pub struct SseFullBuffer {
    carry: Vec<u8>,
    pending_event_type: Option<String>,
}

impl SseFullBuffer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed a chunk and return `(event_type, data_payload)` pairs.
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<(Option<String>, String)> {
        let mut combined = Vec::with_capacity(self.carry.len() + chunk.len());
        combined.extend_from_slice(&self.carry);
        combined.extend_from_slice(chunk);

        let mut results = Vec::new();
        let mut last_split = 0;

        for i in 0..combined.len() {
            if combined[i] == b'\n' {
                let line = &combined[last_split..i];
                last_split = i + 1;

                if line.is_empty() {
                    // Blank line = event boundary. Reset pending event type.
                    self.pending_event_type = None;
                    continue;
                }

                if let Some(rest) = strip_prefix(line, b"event: ") {
                    let event_type = String::from_utf8_lossy(rest).trim().to_string();
                    self.pending_event_type = Some(event_type);
                } else if let Some(rest) = strip_prefix(line, b"data: ") {
                    let payload = String::from_utf8_lossy(rest).trim().to_string();
                    if !payload.is_empty() {
                        results.push((self.pending_event_type.take(), payload));
                    }
                }
                // Ignore other SSE fields (id:, retry:, comments).
            }
        }

        self.carry = combined[last_split..].to_vec();
        results
    }
}

// ── SSE event stream ────────────────────────────────────────────────────────

/// Wraps a `reqwest` byte stream into a `Stream<Item = Result<StreamEvent, String>>`.
///
/// Handles SSE framing for both OpenAI and Anthropic formats:
/// - Splits bytes into lines, capturing `event:` and `data:` fields
/// - Detects `data: [DONE]` (OpenAI) or `message_stop` events (Anthropic)
/// - Parses JSON data payloads into [`StreamEvent`]s
pub struct SseEventStream {
    inner: Pin<Box<dyn Stream<Item = std::result::Result<bytes::Bytes, reqwest::Error>> + Send>>,
    buffer: SseFullBuffer,
    done: bool,
    pending: Vec<StreamEvent>,
    mode: SseMode,
}

/// Which provider format the SSE stream should parse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SseMode {
    OpenAi,
    Anthropic,
    Gemini,
    /// Auto-detect mode: will switch to the correct mode after seeing the first event.
    Auto,
}

impl SseEventStream {
    /// Create a new SSE event stream in OpenAI mode.
    pub fn openai(
        inner: Pin<
            Box<dyn Stream<Item = std::result::Result<bytes::Bytes, reqwest::Error>> + Send>,
        >,
    ) -> Self {
        Self {
            inner,
            buffer: SseFullBuffer::new(),
            done: false,
            pending: Vec::new(),
            mode: SseMode::OpenAi,
        }
    }

    /// Create a new SSE event stream in Anthropic mode.
    pub fn anthropic(
        inner: Pin<
            Box<dyn Stream<Item = std::result::Result<bytes::Bytes, reqwest::Error>> + Send>,
        >,
    ) -> Self {
        Self {
            inner,
            buffer: SseFullBuffer::new(),
            done: false,
            pending: Vec::new(),
            mode: SseMode::Anthropic,
        }
    }

    /// Create a new SSE event stream in Gemini mode.
    pub fn gemini(
        inner: Pin<
            Box<dyn Stream<Item = std::result::Result<bytes::Bytes, reqwest::Error>> + Send>,
        >,
    ) -> Self {
        Self {
            inner,
            buffer: SseFullBuffer::new(),
            done: false,
            pending: Vec::new(),
            mode: SseMode::Gemini,
        }
    }

    /// Create a new SSE event stream with automatic format detection.
    ///
    /// The stream will auto-detect whether the provider is using Anthropic or OpenAI
    /// format by examining the first event. This is useful for OpenAI-compatible
    /// endpoints that may return Anthropic-formatted SSE events.
    pub fn auto(
        inner: Pin<
            Box<dyn Stream<Item = std::result::Result<bytes::Bytes, reqwest::Error>> + Send>,
        >,
    ) -> Self {
        Self {
            inner,
            buffer: SseFullBuffer::new(),
            done: false,
            pending: Vec::new(),
            mode: SseMode::Auto,
        }
    }

    fn poll_inner(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Option<std::result::Result<(), String>>> {
        use futures::StreamExt;

        loop {
            match self.inner.poll_next_unpin(cx) {
                Poll::Ready(Some(Ok(chunk))) => {
                    let pairs = self.buffer.feed(&chunk);

                    for (event_type, payload) in pairs {
                        // Auto-detect format on first event.
                        if self.mode == SseMode::Auto {
                            self.mode = detect_sse_format(&event_type, &payload);
                            tracing::info!(
                                detected_mode = ?self.mode,
                                "auto-detected SSE format from first event"
                            );
                        }

                        if self.mode != SseMode::Anthropic && payload == "[DONE]" {
                            self.done = true;
                            self.pending.push(StreamEvent::Done);
                            break;
                        }

                        match self.mode {
                            SseMode::Anthropic => {
                                let et = event_type.as_deref().unwrap_or("");
                                let events = parse_anthropic_event(et, &payload);
                                self.pending.extend(events);
                            }
                            SseMode::Gemini => {
                                let et = event_type.as_deref().unwrap_or("");
                                let events = parse_gemini_event(et, &payload);
                                self.pending.extend(events);
                            }
                            SseMode::OpenAi => {
                                let events = parse_openai_chunk(&payload);
                                self.pending.extend(events);
                            }
                            SseMode::Auto => {
                                // Should not reach here after first event, but fallback to OpenAI.
                                let events = parse_openai_chunk(&payload);
                                self.pending.extend(events);
                            }
                        }
                    }

                    if !self.pending.is_empty() {
                        return Poll::Ready(Some(Ok(())));
                    }
                    // No complete events yet — continue polling.
                }
                Poll::Ready(Some(Err(e))) => {
                    return Poll::Ready(Some(Err(format!("SSE stream error: {e}"))));
                }
                Poll::Ready(None) => {
                    self.done = true;
                    return Poll::Ready(None);
                }
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

impl Stream for SseEventStream {
    type Item = std::result::Result<StreamEvent, String>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // Yield pending events first.
        if !self.pending.is_empty() {
            let event = self.pending.remove(0);
            return Poll::Ready(Some(Ok(event)));
        }

        if self.done {
            return Poll::Ready(None);
        }

        // Poll inner stream to fill pending buffer.
        match self.poll_inner(cx) {
            Poll::Ready(Some(Ok(()))) => {
                if !self.pending.is_empty() {
                    let event = self.pending.remove(0);
                    Poll::Ready(Some(Ok(event)))
                } else {
                    // Shouldn't happen, but handle gracefully.
                    Poll::Ready(None)
                }
            }
            Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(e))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

// ── Auto-detection helper ──────────────────────────────────────────────────

/// Detect SSE format from the first event.
///
/// Returns the detected mode based on event type and payload structure:
/// - Anthropic: has explicit `event:` lines or `"type":"message_start"` in payload
/// - Gemini: payload contains `"candidates":[...]` structure
/// - OpenAI: default fallback (no explicit event type, contains `"choices":[...]`)
fn detect_sse_format(event_type: &Option<String>, payload: &str) -> SseMode {
    // Anthropic uses explicit event types like "message_start", "content_block_start".
    if let Some(et) = event_type {
        let et_lower = et.to_lowercase();
        if et_lower.contains("message_start")
            || et_lower.contains("content_block")
            || et_lower.contains("message_delta")
            || et_lower.contains("message_stop")
        {
            return SseMode::Anthropic;
        }
    }

    // Check payload structure for Anthropic format markers.
    if payload.contains("\"type\":\"message_start\"")
        || payload.contains("\"type\":\"content_block_start\"")
        || payload.contains("\"type\":\"content_block_delta\"")
        || payload.contains("\"type\":\"message_delta\"")
    {
        return SseMode::Anthropic;
    }

    // Check payload structure for Gemini.
    if payload.contains("\"candidates\"") {
        return SseMode::Gemini;
    }

    // OpenAI format typically has "choices" array.
    // Default to OpenAI if no other format detected.
    SseMode::OpenAi
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sse_full_buffer_simple() {
        let mut buf = SseFullBuffer::new();
        let chunk = b"data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\n\n";
        let pairs = buf.feed(chunk);
        assert_eq!(pairs.len(), 1);
        assert!(pairs[0].1.contains("Hello"));
    }

    #[test]
    fn test_sse_full_buffer_split_chunk() {
        let mut buf = SseFullBuffer::new();
        let pairs1 = buf.feed(b"data: {\"choices\":[{\"delta\":{\"conte");
        assert!(pairs1.is_empty());
        let pairs2 = buf.feed(b"nt\":\"Hello\"}}]}\n\n");
        assert_eq!(pairs2.len(), 1);
        assert!(pairs2[0].1.contains("Hello"));
    }

    #[test]
    fn test_sse_full_buffer_done() {
        let mut buf = SseFullBuffer::new();
        let pairs = buf.feed(b"data: [DONE]\n\n");
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].1, "[DONE]");
    }

    #[test]
    fn test_sse_full_buffer_event_and_data() {
        let mut buf = SseFullBuffer::new();
        let chunk = b"event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hi\"}}\n\n";
        let pairs = buf.feed(chunk);
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].0.as_deref(), Some("content_block_delta"));
        assert!(pairs[0].1.contains("Hi"));
    }

    #[test]
    fn test_sse_full_buffer_multiple_events() {
        let mut buf = SseFullBuffer::new();
        let chunk = b"data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\" world\"}}]}\n\n";
        let pairs = buf.feed(chunk);
        assert_eq!(pairs.len(), 2);
        assert!(pairs[0].1.contains("Hello"));
        assert!(pairs[1].1.contains("world"));
    }

    #[test]
    fn test_sse_full_buffer_ignores_comments() {
        let mut buf = SseFullBuffer::new();
        let chunk =
            b": this is a comment\ndata: {\"choices\":[{\"delta\":{\"content\":\"Hi\"}}]}\n\n";
        let pairs = buf.feed(chunk);
        assert_eq!(pairs.len(), 1);
        assert!(pairs[0].1.contains("Hi"));
    }

    #[test]
    fn test_parse_openai_content_delta() {
        let json = r#"{"choices":[{"delta":{"content":"Hello world"},"index":0}]}"#;
        let events = parse_openai_chunk(json);
        assert_eq!(events.len(), 1);
        match &events[0] {
            StreamEvent::ContentDelta(s) => assert_eq!(s, "Hello world"),
            _ => panic!("expected ContentDelta"),
        }
    }

    #[test]
    fn test_parse_openai_tool_call_delta() {
        let json = r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"read_file","arguments":""}}]},"index":0}]}"#;
        let events = parse_openai_chunk(json);
        assert_eq!(events.len(), 1);
        match &events[0] {
            StreamEvent::ToolCallDelta {
                index, id, name, ..
            } => {
                assert_eq!(*index, 0);
                assert_eq!(id.as_deref(), Some("call_1"));
                assert_eq!(name.as_deref(), Some("read_file"));
            }
            _ => panic!("expected ToolCallDelta"),
        }
    }

    #[test]
    fn test_parse_openai_usage() {
        let json = r#"{"choices":[],"usage":{"prompt_tokens":10,"completion_tokens":20}}"#;
        let events = parse_openai_chunk(json);
        assert_eq!(events.len(), 1);
        match &events[0] {
            StreamEvent::Usage {
                prompt_tokens,
                completion_tokens,
            } => {
                assert_eq!(*prompt_tokens, 10);
                assert_eq!(*completion_tokens, 20);
            }
            _ => panic!("expected Usage"),
        }
    }

    #[test]
    fn test_parse_openai_finish_reason_length() {
        let json = r#"{"choices":[{"delta":{},"index":0,"finish_reason":"length"}]}"#;
        let events = parse_openai_chunk(json);
        assert_eq!(events.len(), 1);
        match &events[0] {
            StreamEvent::Finished(reason) => assert_eq!(reason, "length"),
            _ => panic!("expected Finished"),
        }
    }

    #[test]
    fn test_parse_openai_empty_content_ignored() {
        let json = r#"{"choices":[{"delta":{"content":""},"index":0}]}"#;
        let events = parse_openai_chunk(json);
        assert!(events.is_empty());
    }

    #[test]
    fn test_parse_anthropic_text_delta() {
        let json = r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}"#;
        let events = parse_anthropic_event("content_block_delta", json);
        assert_eq!(events.len(), 1);
        match &events[0] {
            StreamEvent::ContentDelta(s) => assert_eq!(s, "Hello"),
            _ => panic!("expected ContentDelta"),
        }
    }

    #[test]
    fn test_parse_anthropic_tool_use_start() {
        let json = r#"{"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_123","name":"read_file","input":{}}}"#;
        let events = parse_anthropic_event("content_block_start", json);
        assert_eq!(events.len(), 1);
        match &events[0] {
            StreamEvent::ToolCallDelta {
                index, id, name, ..
            } => {
                assert_eq!(*index, 1);
                assert_eq!(id.as_deref(), Some("toolu_123"));
                assert_eq!(name.as_deref(), Some("read_file"));
            }
            _ => panic!("expected ToolCallDelta"),
        }
    }

    #[test]
    fn test_parse_anthropic_message_delta_stop_reason_max_tokens() {
        let json = r#"{"type":"message_delta","delta":{"stop_reason":"max_tokens"},"usage":{"output_tokens":42}}"#;
        let events = parse_anthropic_event("message_delta", json);
        assert!(
            events.iter().any(
                |event| matches!(event, StreamEvent::Finished(reason) if reason == "max_tokens")
            )
        );
    }

    #[test]
    fn test_parse_anthropic_input_json_delta() {
        let json = r#"{"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"path\":"}}"#;
        let events = parse_anthropic_event("content_block_delta", json);
        assert_eq!(events.len(), 1);
        match &events[0] {
            StreamEvent::ToolCallDelta {
                index,
                arguments_delta,
                ..
            } => {
                assert_eq!(*index, 1);
                assert_eq!(arguments_delta, "{\"path\":");
            }
            _ => panic!("expected ToolCallDelta"),
        }
    }

    #[test]
    fn test_parse_anthropic_message_stop() {
        let json = r#"{"type":"message_stop"}"#;
        let events = parse_anthropic_event("message_stop", json);
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], StreamEvent::Done));
    }

    #[test]
    fn test_parse_anthropic_message_start_usage() {
        let json = r#"{"type":"message_start","message":{"id":"msg_1","type":"message","role":"assistant","content":[],"model":"claude-3","stop_reason":null,"usage":{"input_tokens":25,"output_tokens":1}}}"#;
        let events = parse_anthropic_event("message_start", json);
        assert_eq!(events.len(), 1);
        match &events[0] {
            StreamEvent::Usage { prompt_tokens, .. } => {
                assert_eq!(*prompt_tokens, 25);
            }
            _ => panic!("expected Usage"),
        }
    }

    #[test]
    fn test_parse_anthropic_chunk_inferred_type() {
        let json = r#"{"type":"message_stop"}"#;
        let events = parse_anthropic_chunk(json);
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], StreamEvent::Done));
    }

    #[test]
    fn test_accumulator() {
        let mut acc = StreamAccumulator::new();
        acc.push(StreamEvent::ContentDelta("Hello ".to_string()));
        acc.push(StreamEvent::ContentDelta("world".to_string()));
        acc.push(StreamEvent::ToolCallDelta {
            index: 0,
            id: Some("call_1".to_string()),
            name: Some("read_file".to_string()),
            arguments_delta: r#"{"path"#.to_string(),
        });
        acc.push(StreamEvent::ToolCallDelta {
            index: 0,
            id: None,
            name: None,
            arguments_delta: r#"":"foo.txt"}"#.to_string(),
        });
        acc.push(StreamEvent::Usage {
            prompt_tokens: 10,
            completion_tokens: 20,
        });

        assert_eq!(acc.content, "Hello world");
        assert_eq!(acc.tool_calls.len(), 1);
        assert_eq!(acc.tool_calls[0].id, "call_1");
        assert_eq!(acc.tool_calls[0].name, "read_file");
        assert_eq!(acc.tool_calls[0].arguments, r#"{"path":"foo.txt"}"#);
        assert_eq!(acc.prompt_tokens, 10);
        assert_eq!(acc.completion_tokens, 20);
    }

    #[test]
    fn test_accumulator_multiple_tool_calls() {
        let mut acc = StreamAccumulator::new();
        acc.push(StreamEvent::ToolCallDelta {
            index: 0,
            id: Some("call_1".to_string()),
            name: Some("read_file".to_string()),
            arguments_delta: "{}".to_string(),
        });
        acc.push(StreamEvent::ToolCallDelta {
            index: 1,
            id: Some("call_2".to_string()),
            name: Some("write_file".to_string()),
            arguments_delta: "{}".to_string(),
        });
        acc.push(StreamEvent::ToolCallDelta {
            index: 2,
            id: Some("call_3".to_string()),
            name: Some("bash".to_string()),
            arguments_delta: "{}".to_string(),
        });

        assert_eq!(acc.tool_calls.len(), 3);
        assert_eq!(acc.tool_calls[0].id, "call_1");
        assert_eq!(acc.tool_calls[1].id, "call_2");
        assert_eq!(acc.tool_calls[2].id, "call_3");
    }

    // ── Gemini SSE parsing tests ────────────────────────────────────────────

    #[test]
    fn test_parse_gemini_text_delta() {
        let json = r#"{"candidates":[{"content":{"parts":[{"text":"Hello"}]}}]}"#;
        let events = parse_gemini_chunk(json);
        assert_eq!(events.len(), 1);
        match &events[0] {
            StreamEvent::ContentDelta(s) => assert_eq!(s, "Hello"),
            _ => panic!("expected ContentDelta"),
        }
    }

    #[test]
    fn test_parse_gemini_function_call() {
        let json = r#"{"candidates":[{"content":{"parts":[{"functionCall":{"name":"read_file","args":{"path":"/tmp/test.txt"}}}]}}]}"#;
        let events = parse_gemini_chunk(json);
        assert_eq!(events.len(), 1);
        match &events[0] {
            StreamEvent::ToolCallDelta {
                name,
                arguments_delta,
                ..
            } => {
                assert_eq!(name.as_deref(), Some("read_file"));
                assert!(arguments_delta.contains("/tmp/test.txt"));
            }
            _ => panic!("expected ToolCallDelta"),
        }
    }

    #[test]
    fn test_parse_gemini_usage() {
        let json = r#"{"candidates":[],"usageMetadata":{"promptTokenCount":10,"candidatesTokenCount":20,"totalTokenCount":30}}"#;
        let events = parse_gemini_chunk(json);
        assert_eq!(events.len(), 1);
        match &events[0] {
            StreamEvent::Usage {
                prompt_tokens,
                completion_tokens,
            } => {
                assert_eq!(*prompt_tokens, 10);
                assert_eq!(*completion_tokens, 20);
            }
            _ => panic!("expected Usage"),
        }
    }

    #[test]
    fn test_parse_gemini_finish_reason_stop() {
        let json =
            r#"{"candidates":[{"content":{"parts":[{"text":"Done"}]},"finishReason":"STOP"}]}"#;
        let events = parse_gemini_chunk(json);
        assert_eq!(events.len(), 3);
        assert!(matches!(events[0], StreamEvent::ContentDelta(_)));
        assert!(matches!(events[1], StreamEvent::Finished(ref reason) if reason == "STOP"));
        assert!(matches!(events[2], StreamEvent::Done));
    }

    #[test]
    fn test_parse_gemini_finish_reason_max_tokens() {
        let json = r#"{"candidates":[{"content":{"parts":[{"text":"Truncated"}]},"finishReason":"MAX_TOKENS"}]}"#;
        let events = parse_gemini_chunk(json);
        assert!(
            events
                .iter()
                .any(|e| matches!(e, StreamEvent::Finished(reason) if reason == "MAX_TOKENS"))
        );
        assert!(events.iter().any(|e| matches!(e, StreamEvent::Done)));
    }

    #[test]
    fn test_parse_gemini_mixed_content_and_usage() {
        let json = r#"{"candidates":[{"content":{"parts":[{"text":"Hello world"}]},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":5,"candidatesTokenCount":3}}"#;
        let events = parse_gemini_chunk(json);
        assert_eq!(events.len(), 4); // ContentDelta, Finished, Done, Usage
        assert!(matches!(events[0], StreamEvent::ContentDelta(_)));
        assert!(matches!(events[1], StreamEvent::Finished(ref reason) if reason == "STOP"));
        assert!(matches!(events[2], StreamEvent::Done));
        assert!(matches!(events[3], StreamEvent::Usage { .. }));
    }

    #[test]
    fn test_parse_gemini_empty_content_ignored() {
        let json = r#"{"candidates":[{"content":{"parts":[{"text":""}]}}]}"#;
        let events = parse_gemini_chunk(json);
        assert!(events.is_empty());
    }

    #[test]
    fn test_parse_gemini_invalid_json() {
        let events = parse_gemini_chunk("not valid json");
        assert!(events.is_empty());
    }

    #[test]
    fn test_parse_gemini_no_candidates() {
        let json = r#"{"candidates":[]}"#;
        let events = parse_gemini_chunk(json);
        assert!(events.is_empty());
    }

    #[test]
    fn test_parse_gemini_function_call_no_args() {
        let json =
            r#"{"candidates":[{"content":{"parts":[{"functionCall":{"name":"get_time"}}]}}]}"#;
        let events = parse_gemini_chunk(json);
        assert_eq!(events.len(), 1);
        match &events[0] {
            StreamEvent::ToolCallDelta {
                name,
                arguments_delta,
                ..
            } => {
                assert_eq!(name.as_deref(), Some("get_time"));
                assert_eq!(arguments_delta, "{}");
            }
            _ => panic!("expected ToolCallDelta"),
        }
    }

    #[test]
    fn test_parse_gemini_event_delegates_to_chunk() {
        let json = r#"{"candidates":[{"content":{"parts":[{"text":"Hi"}]}}]}"#;
        let events = parse_gemini_event("ignored", json);
        assert_eq!(events.len(), 1);
        match &events[0] {
            StreamEvent::ContentDelta(s) => assert_eq!(s, "Hi"),
            _ => panic!("expected ContentDelta"),
        }
    }

    #[test]
    fn test_detect_sse_format_anthropic() {
        // Test with explicit event type
        let event_type = Some("message_start".to_string());
        let payload = r#"{"type":"message_start"}"#;
        assert_eq!(detect_sse_format(&event_type, payload), SseMode::Anthropic);

        // Test with payload-only detection (no event type)
        let event_type = None;
        let payload = r#"{"type":"message_start","message":{"id":"msg_123"}}"#;
        assert_eq!(detect_sse_format(&event_type, payload), SseMode::Anthropic);

        let payload = r#"{"type":"content_block_start","index":0}"#;
        assert_eq!(detect_sse_format(&event_type, payload), SseMode::Anthropic);

        let payload = r#"{"type":"content_block_delta","delta":{"text":"Hi"}}"#;
        assert_eq!(detect_sse_format(&event_type, payload), SseMode::Anthropic);
    }

    #[test]
    fn test_detect_sse_format_gemini() {
        let event_type = None;
        let payload = r#"{"candidates":[{"content":{"parts":[{"text":"Hi"}]}}]}"#;
        assert_eq!(detect_sse_format(&event_type, payload), SseMode::Gemini);
    }

    #[test]
    fn test_detect_sse_format_openai() {
        let event_type = None;
        let payload = r#"{"choices":[{"delta":{"content":"Hi"}}]}"#;
        assert_eq!(detect_sse_format(&event_type, payload), SseMode::OpenAi);
    }
}
