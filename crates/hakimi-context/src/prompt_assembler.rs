//! Budget-aware assembly of the system prompt.
//!
//! `prompt_builder` knows how to *produce* each block (identity, environment,
//! platform hints, skills, memory, project context). This module knows how to
//! *fit* them together.
//!
//! The runtime used to concatenate `base_prompt + skill_context` ad hoc in
//! `build_send_messages`, and every entry point (CLI / Server / TUI) injected
//! persistent memory on its own — or not at all. Concatenation has no ceiling:
//! as soon as memory, skills and project context all grow, the prompt silently
//! crowds out the conversation.
//!
//! Here every block declares a priority and a floor. The assembler renders
//! high-priority blocks first, squeezes a block down to its floor before
//! dropping it, and records what it dropped so diagnostics can explain a
//! thin prompt.

use crate::prompt_builder::build_platform_hints;

/// Default global character budget for the assembled system prompt.
const DEFAULT_TOTAL_BUDGET: usize = 24_000;

/// Hard floor for the global budget; smaller values would starve identity.
const MIN_TOTAL_BUDGET: usize = 1_000;

/// Named priority tiers. Higher wins when the budget is tight.
pub mod priority {
    /// Agent identity / persona. Never dropped first.
    pub const IDENTITY: u8 = 100;
    /// OS / host / workdir hints.
    pub const ENVIRONMENT: u8 = 80;
    /// Per-platform output formatting rules.
    pub const OUTPUT_STYLE: u8 = 70;
    /// Active task plan for the current session. Outranks memory: an in-flight
    /// plan is what the model must act on next.
    pub const PLAN: u8 = 65;
    /// Long-term memory and user profile.
    pub const MEMORY: u8 = 60;
    /// Runtime working-set skills.
    pub const SKILLS: u8 = 50;
    /// AGENTS.md / CLAUDE.md / project rules.
    pub const PROJECT_CONTEXT: u8 = 40;
    /// Low-value runtime notes (compression notices, telemetry).
    pub const RUNTIME_NOTES: u8 = 20;
}

/// One injectable block of the system prompt.
#[derive(Debug, Clone)]
pub struct PromptSection {
    /// Stable identifier used for diagnostics.
    pub name: String,
    /// Higher priority sections are kept first when the budget is tight.
    pub priority: u8,
    /// Rendered content.
    pub content: String,
    /// Minimum characters preserved when squeezing this section.
    pub min_chars: usize,
    /// Hard cap applied to this section before global budgeting.
    pub max_chars: usize,
}

impl PromptSection {
    /// Create a section with no per-section limits.
    pub fn new(name: impl Into<String>, priority: u8, content: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            priority,
            content: content.into(),
            min_chars: 0,
            max_chars: usize::MAX,
        }
    }

    /// Apply per-section squeeze limits.
    pub fn with_limits(mut self, min_chars: usize, max_chars: usize) -> Self {
        self.min_chars = min_chars;
        self.max_chars = max_chars.max(min_chars);
        self
    }

    /// Character count of the section body.
    pub fn char_count(&self) -> usize {
        self.content.chars().count()
    }

    /// Whether the section has no meaningful content.
    pub fn is_empty(&self) -> bool {
        self.content.trim().is_empty()
    }
}

/// Truncate on a character boundary, never a byte boundary.
fn truncate_chars(text: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max_chars.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Per-platform output style guidance.
///
/// This is the single source of truth for "how should the reply be formatted
/// on the surface the user is actually reading". It delegates to the table in
/// [`crate::prompt_builder::build_platform_hints`] so there is exactly one
/// place to add a platform.
pub fn platform_output_style(platform: &str) -> String {
    let key = platform.trim().to_ascii_lowercase();
    build_platform_hints()
        .get(key.as_str())
        .map(|hint| (*hint).to_string())
        .unwrap_or_default()
}

/// Collects prompt sections and renders them under a global character budget.
#[derive(Debug, Clone)]
pub struct PromptAssembler {
    sections: Vec<PromptSection>,
    total_budget: usize,
    dropped: Vec<String>,
}

impl Default for PromptAssembler {
    fn default() -> Self {
        Self::new()
    }
}

impl PromptAssembler {
    /// Create an assembler with the default budget.
    pub fn new() -> Self {
        Self {
            sections: Vec::new(),
            total_budget: DEFAULT_TOTAL_BUDGET,
            dropped: Vec::new(),
        }
    }

    /// Override the global character budget.
    pub fn with_budget(mut self, budget: usize) -> Self {
        self.total_budget = budget.max(MIN_TOTAL_BUDGET);
        self
    }

    /// Register a section. Empty sections are ignored.
    pub fn push(&mut self, section: PromptSection) {
        if !section.is_empty() {
            self.sections.push(section);
        }
    }

    /// Builder-style registration.
    pub fn section(mut self, section: PromptSection) -> Self {
        self.push(section);
        self
    }

    /// Convenience: register a plain content block.
    pub fn add(&mut self, name: &str, priority: u8, content: impl Into<String>) {
        self.push(PromptSection::new(name, priority, content));
    }

    /// Names of sections that were dropped in the last render.
    pub fn dropped_sections(&self) -> &[String] {
        &self.dropped
    }

    /// Number of registered sections.
    pub fn len(&self) -> usize {
        self.sections.len()
    }

    /// Whether no sections are registered.
    pub fn is_empty(&self) -> bool {
        self.sections.is_empty()
    }

    /// Render the system prompt.
    ///
    /// Sections are visited in descending priority. Each is capped by its own
    /// `max_chars`; a section that no longer fits the remaining budget is
    /// squeezed down to `min_chars`, and dropped entirely only when even that
    /// floor does not fit. The output keeps the original registration order so
    /// the prompt shape stays stable across turns.
    pub fn render(&mut self) -> String {
        self.dropped.clear();

        let mut order: Vec<usize> = (0..self.sections.len()).collect();
        order.sort_by(|a, b| self.sections[*b].priority.cmp(&self.sections[*a].priority));

        let mut budget_left = self.total_budget;
        let mut rendered: Vec<(usize, String)> = Vec::new();

        for idx in order {
            let (name, mut text, min_chars, max_chars) = {
                let section = &self.sections[idx];
                (
                    section.name.clone(),
                    section.content.trim().to_string(),
                    section.min_chars,
                    section.max_chars,
                )
            };

            if text.chars().count() > max_chars {
                text = truncate_chars(&text, max_chars);
            }

            let cost = text.chars().count();
            if cost <= budget_left {
                budget_left -= cost;
                rendered.push((idx, text));
                continue;
            }

            let floor = min_chars.min(max_chars);
            if floor > 0 && budget_left >= floor {
                let squeezed = truncate_chars(&text, budget_left);
                budget_left = budget_left.saturating_sub(squeezed.chars().count());
                rendered.push((idx, squeezed));
            } else {
                self.dropped.push(name);
            }
        }

        rendered.sort_by_key(|(idx, _)| *idx);
        rendered
            .into_iter()
            .map(|(_, text)| text)
            .collect::<Vec<_>>()
            .join("\n\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_sections_are_ignored() {
        let mut assembler = PromptAssembler::new();
        assembler.add("blank", priority::IDENTITY, "   ");
        assert!(assembler.is_empty());
    }

    #[test]
    fn renders_in_registration_order() {
        let mut assembler = PromptAssembler::new();
        assembler.add("identity", priority::IDENTITY, "You are Hakimi.");
        assembler.add("skills", priority::SKILLS, "## Skills\n- one");
        let out = assembler.render();
        assert!(out.starts_with("You are Hakimi."));
        assert!(out.contains("## Skills"));
    }

    #[test]
    fn low_priority_section_is_squeezed_before_identity() {
        let mut assembler = PromptAssembler::new().with_budget(1_000);
        assembler.add("identity", priority::IDENTITY, "I".repeat(600));
        assembler.push(
            PromptSection::new("skills", priority::SKILLS, "S".repeat(900)).with_limits(100, 900),
        );
        let out = assembler.render();
        assert!(out.starts_with(&"I".repeat(600)));
        assert!(out.chars().count() <= 1_004);
    }

    #[test]
    fn section_without_floor_is_dropped_when_budget_is_gone() {
        let mut assembler = PromptAssembler::new().with_budget(1_000);
        assembler.add("identity", priority::IDENTITY, "I".repeat(999));
        assembler.add("notes", priority::RUNTIME_NOTES, "N".repeat(500));
        let _ = assembler.render();
        assert!(assembler.dropped_sections().contains(&"notes".to_string()));
    }

    #[test]
    fn truncation_is_character_safe() {
        let mut assembler = PromptAssembler::new().with_budget(1_000);
        assembler.push(
            PromptSection::new("cjk", priority::IDENTITY, "上下文管理".repeat(50))
                .with_limits(10, 10),
        );
        let out = assembler.render();
        assert!(out.chars().count() <= 10);
        assert!(out.ends_with('…'));
    }

    #[test]
    fn plan_outranks_memory_when_budget_is_tight() {
        let mut assembler = PromptAssembler::new().with_budget(1_000);
        assembler.add("identity", priority::IDENTITY, "I".repeat(400));
        assembler
            .push(PromptSection::new("plan", priority::PLAN, "P".repeat(300)).with_limits(50, 300));
        // No floor: memory is expendable, the active plan is not.
        assembler.add("memory", priority::MEMORY, "M".repeat(600));

        let out = assembler.render();
        assert!(out.contains(&"P".repeat(300)));
        assert!(!out.contains('M'));
        assert!(assembler.dropped_sections().contains(&"memory".to_string()));
    }

    #[test]
    fn telegram_style_mentions_no_tables() {
        let style = platform_output_style("telegram");
        assert!(style.contains("NO table syntax"));
    }

    #[test]
    fn platform_lookup_is_case_insensitive() {
        assert!(!platform_output_style("  Telegram  ").is_empty());
    }

    #[test]
    fn unknown_platform_yields_no_style_block() {
        assert!(platform_output_style("carrier-pigeon").is_empty());
    }
}
