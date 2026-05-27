/// State machine that strips reasoning blocks from streamed text.
///
/// Some reasoning models embed their chain-of-thought inside tags in the
/// content field. This scrubber removes those blocks so they can be stored
/// separately and not shown to the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScrubberState {
    /// Currently outside a reasoning block; text passes through.
    Normal,
    /// Currently inside a reasoning block; text is being suppressed.
    InThinkTag,
}

const OPEN_TAGS: [&str; 5] = [
    "<think>",
    "<thinking>",
    "<reasoning>",
    "<thought>",
    "<REASONING_SCRATCHPAD>",
];
const CLOSE_TAGS: [&str; 5] = [
    "</think>",
    "</thinking>",
    "</reasoning>",
    "</thought>",
    "</REASONING_SCRATCHPAD>",
];
const MAX_TAG_LEN: usize = 24; // "</REASONING_SCRATCHPAD>"

/// Streaming reasoning-block scrubber.
#[derive(Debug, Clone)]
pub struct ThinkScrubber {
    state: ScrubberState,
    /// Accumulated reasoning text extracted from think blocks.
    reasoning: String,
    /// Held-back partial tag suffix split across streaming chunks.
    pending: String,
    /// Whether the last visible emission ended on a line boundary.
    last_emitted_ended_newline: bool,
}

impl Default for ThinkScrubber {
    fn default() -> Self {
        Self::new()
    }
}

impl ThinkScrubber {
    pub fn new() -> Self {
        Self {
            state: ScrubberState::Normal,
            reasoning: String::new(),
            pending: String::new(),
            last_emitted_ended_newline: true,
        }
    }

    /// Returns the accumulated reasoning text stripped from think blocks.
    pub fn reasoning(&self) -> &str {
        &self.reasoning
    }

    /// Returns `true` if the scrubber is currently inside a think block.
    pub fn in_think_block(&self) -> bool {
        self.state == ScrubberState::InThinkTag
    }

    /// Flush held-back text at end of stream.
    ///
    /// Unterminated reasoning blocks are discarded; leaking hidden reasoning is
    /// worse than dropping partial prose from a malformed response.
    pub fn flush(&mut self) -> String {
        if self.state == ScrubberState::InThinkTag {
            self.pending.clear();
            self.state = ScrubberState::Normal;
            return String::new();
        }

        let tail = std::mem::take(&mut self.pending);
        let tail = strip_orphan_close_tags(&tail);
        self.record_emission(&tail);
        tail
    }

    /// Strip reasoning blocks from a complete response.
    pub fn strip_all(input: &str) -> String {
        Self::strip_all_with_reasoning(input).0
    }

    /// Strip reasoning blocks from a complete response and return the captured
    /// hidden reasoning separately.
    pub fn strip_all_with_reasoning(input: &str) -> (String, String) {
        let mut scrubber = Self::new();
        let (mut cleaned, _) = scrubber.process(input);
        cleaned.push_str(&scrubber.flush());
        (cleaned, scrubber.reasoning().to_string())
    }

    /// Feed a chunk of streamed text through the scrubber.
    ///
    /// Returns `(cleaned, is_in_think)` where:
    /// - `cleaned` is the chunk with any think-tag portions removed
    /// - `is_in_think` indicates whether we're currently inside a think block
    pub fn process(&mut self, chunk: &str) -> (String, bool) {
        if chunk.is_empty() {
            return (String::new(), self.state == ScrubberState::InThinkTag);
        }

        let mut output = Vec::new();
        let mut remaining = std::mem::take(&mut self.pending);
        remaining.push_str(chunk);

        while !remaining.is_empty() {
            match self.state {
                ScrubberState::Normal => {
                    let closed_pair = find_earliest_closed_pair(&remaining);
                    let open_at_boundary =
                        find_open_at_boundary(&remaining, &output, self.last_emitted_ended_newline);

                    if let Some(pair) = closed_pair
                        && open_at_boundary.is_none_or(|open| pair.open_start <= open.start)
                    {
                        self.push_visible(&mut output, &remaining[..pair.open_start]);
                        self.reasoning
                            .push_str(&remaining[pair.inner_start..pair.inner_end]);
                        remaining = remaining[pair.close_end..].to_string();
                        continue;
                    }

                    if let Some(open) = open_at_boundary {
                        self.push_visible(&mut output, &remaining[..open.start]);
                        remaining = remaining[open.end..].to_string();
                        self.state = ScrubberState::InThinkTag;
                        continue;
                    }

                    let held = max_partial_suffix(&remaining, &OPEN_TAGS)
                        .max(max_partial_suffix(&remaining, &CLOSE_TAGS));
                    if held > 0 {
                        let split = remaining.len() - held;
                        let visible = &remaining[..split];
                        self.push_visible(&mut output, visible);
                        self.pending = remaining[split..].to_string();
                    } else {
                        self.push_visible(&mut output, &remaining);
                    }
                    break;
                }
                ScrubberState::InThinkTag => {
                    if let Some(close) = find_first_tag(&remaining, &CLOSE_TAGS, 0) {
                        self.reasoning.push_str(&remaining[..close.start]);
                        remaining = remaining[close.end..].to_string();
                        self.state = ScrubberState::Normal;
                    } else {
                        let held = max_partial_suffix(&remaining, &CLOSE_TAGS);
                        if held > 0 {
                            let split = remaining.len() - held;
                            self.reasoning.push_str(&remaining[..split]);
                            self.pending = remaining[split..].to_string();
                        } else {
                            self.reasoning.push_str(&remaining);
                        }
                        break;
                    }
                }
            }
        }

        (output.concat(), self.state == ScrubberState::InThinkTag)
    }

    fn push_visible(&mut self, output: &mut Vec<String>, text: &str) {
        let text = strip_orphan_close_tags(text);
        if text.is_empty() {
            return;
        }
        self.record_emission(&text);
        output.push(text);
    }

    fn record_emission(&mut self, text: &str) {
        if !text.is_empty() {
            self.last_emitted_ended_newline = text.ends_with('\n');
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct TagMatch {
    start: usize,
    end: usize,
}

#[derive(Debug, Clone, Copy)]
struct ClosedPair {
    open_start: usize,
    inner_start: usize,
    inner_end: usize,
    close_end: usize,
}

fn find_first_tag(buf: &str, tags: &[&str], start: usize) -> Option<TagMatch> {
    let bytes = buf.as_bytes();
    let mut best: Option<TagMatch> = None;
    for tag in tags {
        let tag_bytes = tag.as_bytes();
        if tag_bytes.is_empty() || bytes.len() < tag_bytes.len() {
            continue;
        }
        let mut i = start;
        while i + tag_bytes.len() <= bytes.len() {
            if bytes[i] == b'<' && bytes[i..i + tag_bytes.len()].eq_ignore_ascii_case(tag_bytes) {
                let candidate = TagMatch {
                    start: i,
                    end: i + tag_bytes.len(),
                };
                if best.is_none_or(|current| candidate.start < current.start) {
                    best = Some(candidate);
                }
                break;
            }
            i += 1;
        }
    }
    best
}

fn find_earliest_closed_pair(buf: &str) -> Option<ClosedPair> {
    let mut best: Option<ClosedPair> = None;
    for (open_tag, close_tag) in OPEN_TAGS.iter().zip(CLOSE_TAGS.iter()) {
        let Some(open) = find_first_tag(buf, &[*open_tag], 0) else {
            continue;
        };
        let Some(close) = find_first_tag(buf, &[*close_tag], open.end) else {
            continue;
        };
        let candidate = ClosedPair {
            open_start: open.start,
            inner_start: open.end,
            inner_end: close.start,
            close_end: close.end,
        };
        if best.is_none_or(|current| candidate.open_start < current.open_start) {
            best = Some(candidate);
        }
    }
    best
}

fn find_open_at_boundary(
    buf: &str,
    already_emitted: &[String],
    last_emitted_ended_newline: bool,
) -> Option<TagMatch> {
    let mut best: Option<TagMatch> = None;
    for tag in OPEN_TAGS {
        let mut search_start = 0;
        while let Some(candidate) = find_first_tag(buf, &[tag], search_start) {
            if is_block_boundary(
                buf,
                candidate.start,
                already_emitted,
                last_emitted_ended_newline,
            ) {
                if best.is_none_or(|current| candidate.start < current.start) {
                    best = Some(candidate);
                }
                break;
            }
            search_start = candidate.start + 1;
        }
    }
    best
}

fn is_block_boundary(
    buf: &str,
    idx: usize,
    already_emitted: &[String],
    last_emitted_ended_newline: bool,
) -> bool {
    if idx == 0 {
        return already_emitted
            .last()
            .map_or(last_emitted_ended_newline, |s| s.ends_with('\n'));
    }

    let preceding = &buf[..idx];
    if let Some(last_newline) = preceding.rfind('\n') {
        preceding[last_newline + 1..].trim().is_empty()
    } else {
        let prior_newline = already_emitted
            .last()
            .map_or(last_emitted_ended_newline, |s| s.ends_with('\n'));
        prior_newline && preceding.trim().is_empty()
    }
}

fn max_partial_suffix(buf: &str, tags: &[&str]) -> usize {
    let bytes = buf.as_bytes();
    let max_check = bytes.len().min(MAX_TAG_LEN - 1);
    for len in (1..=max_check).rev() {
        let suffix = &bytes[bytes.len() - len..];
        for tag in tags {
            let tag_bytes = tag.as_bytes();
            if tag_bytes.len() > len && suffix.eq_ignore_ascii_case(&tag_bytes[..len]) {
                return len;
            }
        }
    }
    0
}

fn strip_orphan_close_tags(text: &str) -> String {
    if !text.contains("</") {
        return text.to_string();
    }

    let mut output = String::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        let mut matched_len = None;
        if text.as_bytes()[i] == b'<' {
            for tag in CLOSE_TAGS {
                let tag_bytes = tag.as_bytes();
                if i + tag_bytes.len() <= text.len()
                    && text.as_bytes()[i..i + tag_bytes.len()].eq_ignore_ascii_case(tag_bytes)
                {
                    matched_len = Some(tag_bytes.len());
                    break;
                }
            }
        }

        if let Some(tag_len) = matched_len {
            i += tag_len;
            while i < text.len() {
                let ch = text[i..].chars().next().expect("valid utf-8");
                if matches!(ch, ' ' | '\t' | '\n' | '\r') {
                    i += ch.len_utf8();
                } else {
                    break;
                }
            }
            continue;
        }

        let ch = text[i..].chars().next().expect("valid utf-8");
        output.push(ch);
        i += ch.len_utf8();
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_think_tags() {
        let mut scrubber = ThinkScrubber::new();
        let (out, in_think) = scrubber.process("Hello world");
        assert_eq!(out, "Hello world");
        assert!(!in_think);
    }

    #[test]
    fn test_full_think_block() {
        let mut scrubber = ThinkScrubber::new();
        let input = "Before<think>reasoning</think>After";
        let (out, in_think) = scrubber.process(input);
        assert_eq!(out, "BeforeAfter");
        assert!(!in_think);
        assert_eq!(scrubber.reasoning(), "reasoning");
    }

    #[test]
    fn test_chunked_think_block() {
        let mut scrubber = ThinkScrubber::new();
        let (out1, in1) = scrubber.process("Hello\n<think>rea");
        assert_eq!(out1, "Hello\n");
        assert!(in1);

        let (out2, in2) = scrubber.process("soning</think>World");
        assert_eq!(out2, "World");
        assert!(!in2);
        assert_eq!(scrubber.reasoning(), "reasoning");
    }

    #[test]
    fn test_multiple_think_blocks() {
        let mut scrubber = ThinkScrubber::new();
        let (out, in_think) = scrubber.process("A<think>first</think>B<think>second</think>C");
        assert_eq!(out, "ABC");
        assert!(!in_think);
        assert_eq!(scrubber.reasoning(), "firstsecond");
    }

    #[test]
    fn test_think_tag_only() {
        let mut scrubber = ThinkScrubber::new();
        let (out, in_think) = scrubber.process("<think>all reasoning</think>");
        assert_eq!(out, "");
        assert!(!in_think);
        assert_eq!(scrubber.reasoning(), "all reasoning");
    }

    #[test]
    fn test_empty_think_tag() {
        let mut scrubber = ThinkScrubber::new();
        let (out, in_think) = scrubber.process("A<think></think>B");
        assert_eq!(out, "AB");
        assert!(!in_think);
        assert_eq!(scrubber.reasoning(), "");
    }

    #[test]
    fn test_think_tag_split_across_boundary() {
        let mut scrubber = ThinkScrubber::new();
        // Split the opening tag itself across chunks
        let (out1, in1) = scrubber.process("<thi");
        assert_eq!(out1, "");
        assert!(!in1);

        let (out2, in2) = scrubber.process("nk>reasoning</think>Done");
        assert_eq!(out2, "Done");
        assert!(!in2);
        assert_eq!(scrubber.reasoning(), "reasoning");
    }

    #[test]
    fn test_default_scrubber() {
        let scrubber = ThinkScrubber::default();
        assert_eq!(scrubber.reasoning(), "");
        assert!(!scrubber.in_think_block());
    }

    #[test]
    fn test_partial_close_tag_in_chunk() {
        let mut scrubber = ThinkScrubber::new();
        scrubber.process("<think>start of thought");
        let (out, in_think) = scrubber.process(" middle</think>end");
        assert_eq!(out, "end");
        assert!(!in_think);
        assert_eq!(scrubber.reasoning(), "start of thought middle");
    }

    #[test]
    fn test_thinking_and_reasoning_tag_variants() {
        let input = "A<thinking>first</thinking>B<reasoning>second</reasoning>C";
        let (out, reasoning) = ThinkScrubber::strip_all_with_reasoning(input);
        assert_eq!(out, "ABC");
        assert_eq!(reasoning, "firstsecond");
    }

    #[test]
    fn test_case_insensitive_reasoning_scratchpad() {
        let input = "A<reasoning_scratchpad>hidden</REASONING_SCRATCHPAD>B";
        let (out, reasoning) = ThinkScrubber::strip_all_with_reasoning(input);
        assert_eq!(out, "AB");
        assert_eq!(reasoning, "hidden");
    }

    #[test]
    fn test_open_tag_mention_mid_line_is_preserved() {
        let mut scrubber = ThinkScrubber::new();
        let (out, in_think) = scrubber.process("Use <think> tags in docs");
        let mut final_out = out;
        final_out.push_str(&scrubber.flush());

        assert_eq!(final_out, "Use <think> tags in docs");
        assert!(!in_think);
        assert_eq!(scrubber.reasoning(), "");
    }

    #[test]
    fn test_closed_pair_mid_line_is_suppressed() {
        let input = "Answer: <thought>hidden</thought>visible";
        let (out, reasoning) = ThinkScrubber::strip_all_with_reasoning(input);
        assert_eq!(out, "Answer: visible");
        assert_eq!(reasoning, "hidden");
    }

    #[test]
    fn test_flush_discards_unterminated_reasoning_block() {
        let mut scrubber = ThinkScrubber::new();
        let (out, in_think) = scrubber.process("<reasoning>hidden");
        assert_eq!(out, "");
        assert!(in_think);
        assert_eq!(scrubber.flush(), "");
        assert!(!scrubber.in_think_block());
        assert_eq!(scrubber.reasoning(), "hidden");
    }

    #[test]
    fn test_strip_all_emits_non_tag_partial_tail() {
        let mut scrubber = ThinkScrubber::new();
        let (out, in_think) = scrubber.process("Visible <th");
        assert_eq!(out, "Visible ");
        assert!(!in_think);
        assert_eq!(scrubber.flush(), "<th");
    }
}
