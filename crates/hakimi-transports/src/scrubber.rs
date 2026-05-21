/// State machine that strips `<think>...</think>` blocks from streamed text.
///
/// Some reasoning models embed their chain-of-thought inside think tags in the
/// content field.  This scrubber removes those blocks so they can be stored
/// separately and not shown to the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScrubberState {
    /// Currently outside a think block — text passes through.
    Normal,
    /// Currently inside a `<think>` block — text is being suppressed.
    InThinkTag,
}

/// Streaming think-block scrubber.
#[derive(Debug, Clone)]
pub struct ThinkScrubber {
    state: ScrubberState,
    /// Accumulated reasoning text extracted from think blocks.
    reasoning: String,
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

    /// Feed a chunk of streamed text through the scrubber.
    ///
    /// Returns `(cleaned, is_in_think)` where:
    /// - `cleaned` is the chunk with any think-tag portions removed
    /// - `is_in_think` indicates whether we're currently inside a think block
    pub fn process(&mut self, chunk: &str) -> (String, bool) {
        let mut output = String::with_capacity(chunk.len());
        let mut remaining = chunk;

        while !remaining.is_empty() {
            match self.state {
                ScrubberState::Normal => {
                    if let Some(start) = remaining.find("<think>") {
                        // Push text before the tag.
                        output.push_str(&remaining[..start]);
                        remaining = &remaining[start + "<think>".len()..];
                        self.state = ScrubberState::InThinkTag;
                    } else {
                        output.push_str(remaining);
                        break;
                    }
                }
                ScrubberState::InThinkTag => {
                    if let Some(end) = remaining.find("</think>") {
                        // Capture reasoning text.
                        self.reasoning.push_str(&remaining[..end]);
                        remaining = &remaining[end + "</think>".len()..];
                        self.state = ScrubberState::Normal;
                    } else {
                        // Entire chunk is inside a think block.
                        self.reasoning.push_str(remaining);
                        break;
                    }
                }
            }
        }

        (output, self.state == ScrubberState::InThinkTag)
    }
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
        let (out1, in1) = scrubber.process("Hello<think>rea");
        assert_eq!(out1, "Hello");
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
        let (out1, in1) = scrubber.process("Hello<");
        assert_eq!(out1, "Hello<");
        assert!(!in1);

        let (out2, in2) = scrubber.process("<think>reasoning</think>Done");
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
}
