// Analyze message complexity to inform model tier selection.
//
// The analyzer uses multiple signals to estimate task complexity:
// - Keywords indicating reasoning, planning, or multi-step work
// - Message length as a proxy for task scope
// - Question depth (nested questions, conditional logic)
// - Code presence and complexity
// - Request for creativity, research, or analysis
//
// The output score (0-100) is NOT a definitive measure but a starting
// point for the learner to refine over time based on user feedback.

use once_cell::sync::Lazy;
use std::collections::HashSet;

/// Complexity score: 0-100
/// - 0-30: Light tasks (simple queries, factual Q&A, basic file operations)
/// - 31-70: Primary tasks (moderate reasoning, coding, multi-step workflows)
/// - 71-100: Reasoning tasks (complex planning, deep analysis, novel problem-solving)
pub type ComplexityScore = u8;

/// Keywords that signal higher complexity tasks
static HIGH_COMPLEXITY_KEYWORDS: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    [
        // Planning & design
        "设计",
        "规划",
        "架构",
        "方案",
        "plan",
        "design",
        "architect",
        "strategy",
        // Deep reasoning
        "分析",
        "优化",
        "重构",
        "analyze",
        "optimize",
        "refactor",
        "rethink",
        // Multi-step workflows
        "实现",
        "集成",
        "迁移",
        "implement",
        "integrate",
        "migrate",
        "port",
        // Research & exploration
        "调研",
        "研究",
        "探索",
        "research",
        "investigate",
        "explore",
        // Complex problem-solving
        "调试",
        "排查",
        "解决",
        "debug",
        "troubleshoot",
        "solve",
        // Creative work
        "创造",
        "生成",
        "构建",
        "create",
        "generate",
        "build from scratch",
    ]
    .into_iter()
    .collect()
});

/// Keywords that signal moderate complexity
static MEDIUM_COMPLEXITY_KEYWORDS: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    [
        "修改",
        "更新",
        "添加",
        "改进",
        "modify",
        "update",
        "add",
        "improve",
        "解释",
        "说明",
        "介绍",
        "explain",
        "describe",
        "introduce",
        "比较",
        "对比",
        "review",
        "compare",
        "contrast",
        "测试",
        "验证",
        "检查",
        "test",
        "verify",
        "check",
    ]
    .into_iter()
    .collect()
});

/// Keywords that signal low complexity
static LOW_COMPLEXITY_KEYWORDS: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    [
        "是", "什么", "吗", "多少", "哪", "when", "what", "where", "who", "which", "显示", "列出",
        "查看", "show", "list", "view", "display", "帮", "help", "hi", "hello", "你好", "在吗",
    ]
    .into_iter()
    .collect()
});

/// Code-related patterns (regex-lite or simple heuristics)
fn contains_code_block(text: &str) -> bool {
    text.contains("```") || text.contains("fn ") || text.contains("def ") || text.contains("class ")
}

/// Multiple questions or conditional logic ("如果...那么", "if...then")
fn has_nested_logic(text: &str) -> bool {
    let question_marks = text.matches('?').count() + text.matches('?').count();
    let conditionals = text.matches("如果").count()
        + text.matches("那么").count()
        + text.matches("if ").count()
        + text.matches("then").count()
        + text.matches("else").count();
    question_marks >= 2 || conditionals >= 2
}

/// Main analyzer: returns a score 0-100
pub fn analyze_complexity(text: &str) -> ComplexityScore {
    let text_lower = text.to_lowercase();
    let char_count = text.chars().count();

    // Base score from length (longer messages often indicate complex tasks)
    let mut score: u8 = match char_count {
        0..=50 => 10,
        51..=150 => 25,
        151..=400 => 40,
        _ => 50,
    };

    // Keyword signals
    let high_matches = HIGH_COMPLEXITY_KEYWORDS
        .iter()
        .filter(|&&kw| text_lower.contains(kw))
        .count();
    let medium_matches = MEDIUM_COMPLEXITY_KEYWORDS
        .iter()
        .filter(|&&kw| text_lower.contains(kw))
        .count();
    let low_matches = LOW_COMPLEXITY_KEYWORDS
        .iter()
        .filter(|&&kw| text_lower.contains(kw))
        .count();

    score += (high_matches * 15) as u8;
    score += (medium_matches * 8) as u8;  // Increased from 5 to 8
    score = score.saturating_sub((low_matches * 10) as u8);

    // Boost for code presence
    if contains_code_block(text) {
        score += 15;
    }

    // Boost for nested logic
    if has_nested_logic(text) {
        score += 10;
    }

    // Cap at 100
    score.min(100)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_query() {
        let score = analyze_complexity("什么是 Rust?");
        assert!(score < 40, "Simple query should be < 40, got {}", score);
    }

    #[test]
    fn test_moderate_task() {
        let score = analyze_complexity("修改 auth.rs 文件，添加 JWT 验证逻辑");
        assert!(
            (30..=70).contains(&score),
            "Moderate task should be 30-70, got {}",
            score
        );
    }

    #[test]
    fn test_complex_task() {
        let score = analyze_complexity(
            "设计一个智能模型调度系统，实现根据消息复杂度自动选择 Light/Primary/Reasoning 三层模型架构，包含学习引擎和用户反馈训练机制",
        );
        assert!(score > 60, "Complex task should be > 60, got {}", score);
    }

    #[test]
    fn test_code_task() {
        let score = analyze_complexity(
            "```rust\nfn main() {\n  println!(\"hello\");\n}\n```\n这段代码有什么问题？",
        );
        assert!(score >= 20, "Code review should boost score, got {}", score);
    }
}
