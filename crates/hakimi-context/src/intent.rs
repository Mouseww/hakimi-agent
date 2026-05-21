use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum Intent {
    InformationSeeking,
    TaskExecution,
    CreativeGeneration,
    Debugging,
    Planning,
    SocialChat,
    MetaQuery,
    CodeReview,
    Research,
    Configuration,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentPrediction {
    pub primary: Intent,
    pub confidence: f32,
    pub secondary: Vec<(Intent, f32)>,
    pub predicted_actions: Vec<String>,
    pub context_hints: Vec<String>,
}

#[allow(dead_code)]
struct PatternRule {
    pattern: String,
    intent: Intent,
    weight: f32,
}

pub struct IntentClassifier {
    keyword_rules: HashMap<String, Vec<(Intent, f32)>>,
    pattern_rules: Vec<PatternRule>,
}

impl Default for IntentClassifier {
    fn default() -> Self {
        Self::new()
    }
}

impl IntentClassifier {
    pub fn new() -> Self {
        let mut keyword_rules = HashMap::new();

        // InformationSeeking keywords
        for kw in &["how", "what", "why", "where", "when", "who"] {
            keyword_rules.insert(kw.to_string(), vec![(Intent::InformationSeeking, 0.6)]);
        }

        // Debugging keywords
        for kw in &["fix", "error", "bug", "broken", "crash", "fail", "debug"] {
            keyword_rules.insert(kw.to_string(), vec![(Intent::Debugging, 0.8)]);
        }

        // CreativeGeneration keywords
        for kw in &["write", "create", "generate", "compose", "draft"] {
            keyword_rules.insert(kw.to_string(), vec![(Intent::CreativeGeneration, 0.7)]);
        }

        // Planning keywords
        for kw in &["plan", "steps", "break down", "organize", "outline"] {
            keyword_rules.insert(kw.to_string(), vec![(Intent::Planning, 0.7)]);
        }

        // SocialChat keywords
        for kw in &[
            "hello",
            "hi",
            "hey",
            "thanks",
            "thank you",
            "bye",
            "goodbye",
        ] {
            keyword_rules.insert(kw.to_string(), vec![(Intent::SocialChat, 0.9)]);
        }

        // MetaQuery keywords
        for kw in &[
            "what can you",
            "who are you",
            "your capabilities",
            "what do you do",
        ] {
            keyword_rules.insert(kw.to_string(), vec![(Intent::MetaQuery, 0.9)]);
        }

        // CodeReview keywords
        for kw in &["review", "critique", "code review", "feedback on code"] {
            keyword_rules.insert(kw.to_string(), vec![(Intent::CodeReview, 0.7)]);
        }

        // Research keywords
        for kw in &["research", "investigate", "compare", "study", "analyze"] {
            keyword_rules.insert(kw.to_string(), vec![(Intent::Research, 0.7)]);
        }

        // Configuration keywords
        for kw in &["configure", "config", "setup", "settings", "set up"] {
            keyword_rules.insert(kw.to_string(), vec![(Intent::Configuration, 0.7)]);
        }

        // TaskExecution keywords
        for kw in &["run", "execute", "install", "deploy", "build", "do"] {
            keyword_rules.insert(kw.to_string(), vec![(Intent::TaskExecution, 0.7)]);
        }

        let pattern_rules = vec![
            // Messages starting with ``` -> CodeReview
            PatternRule {
                pattern: "```".to_string(),
                intent: Intent::CodeReview,
                weight: 0.7,
            },
        ];

        Self {
            keyword_rules,
            pattern_rules,
        }
    }

    pub fn classify(&self, message: &str) -> IntentPrediction {
        self.classify_with_context(message, &[])
    }

    pub fn classify_with_context(
        &self,
        message: &str,
        recent_tools: &[String],
    ) -> IntentPrediction {
        let lower = message.to_lowercase();

        // Accumulate scores per intent
        let mut scores: HashMap<Intent, f32> = HashMap::new();

        // 1) Keyword matching (check multi-word keywords first, longer first)
        let mut sorted_keywords: Vec<(&String, &Vec<(Intent, f32)>)> =
            self.keyword_rules.iter().collect();
        sorted_keywords.sort_by_key(|a| std::cmp::Reverse(a.0.len()));

        for (keyword, intents) in &sorted_keywords {
            if keyword.contains(' ') {
                // Multi-word keywords: substring match
                if lower.contains(keyword.as_str()) {
                    for (intent, weight) in *intents {
                        let entry = scores.entry(intent.clone()).or_insert(0.0);
                        *entry += weight;
                    }
                }
            } else {
                // Single-word keywords: word-boundary match
                let words: Vec<&str> = lower
                    .split(|c: char| !c.is_alphanumeric() && c != '\'')
                    .collect();
                if words.contains(&keyword.as_str()) {
                    for (intent, weight) in *intents {
                        let entry = scores.entry(intent.clone()).or_insert(0.0);
                        *entry += weight;
                    }
                }
            }
        }

        // 2) Pattern matching
        for rule in &self.pattern_rules {
            if rule.pattern == "```" && message.contains("```") {
                let entry = scores.entry(Intent::CodeReview).or_insert(0.0);
                *entry += rule.weight;
            }
        }

        // Question marks -> InformationSeeking boost
        if message.contains('?') {
            let entry = scores.entry(Intent::InformationSeeking).or_insert(0.0);
            *entry += 0.2;
        }

        // Imperative verbs at start -> TaskExecution boost
        let imperative_verbs = [
            "run", "do", "execute", "install", "build", "make", "set", "get", "create", "delete",
            "remove", "move", "copy", "start", "stop",
        ];
        let first_word = lower.split_whitespace().next().unwrap_or("");
        if imperative_verbs.contains(&first_word) {
            let entry = scores.entry(Intent::TaskExecution).or_insert(0.0);
            *entry += 0.3;
        }

        // 3) Tool context boosts
        for tool in recent_tools {
            let tool_lower = tool.to_lowercase();
            if tool_lower == "terminal" || tool_lower == "patch" {
                let entry = scores.entry(Intent::Debugging).or_insert(0.0);
                *entry += 0.2;
            }
            if tool_lower == "web_search" {
                let entry = scores.entry(Intent::Research).or_insert(0.0);
                *entry += 0.3;
            }
        }

        // Normalize all scores to 0.0-1.0
        let max_score = scores.values().copied().fold(0.0_f32, f32::max);
        if max_score > 1.0 {
            for score in scores.values_mut() {
                *score /= max_score;
            }
        }

        // Sort by score descending
        let mut sorted: Vec<(Intent, f32)> = scores.into_iter().collect();
        sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        if sorted.is_empty() {
            // Default to SocialChat with low confidence
            let prediction = IntentPrediction {
                primary: Intent::SocialChat,
                confidence: 0.3,
                secondary: vec![],
                predicted_actions: vec![],
                context_hints: vec![],
            };
            return prediction;
        }

        let primary = sorted[0].0.clone();
        let confidence = sorted[0].1;
        let secondary: Vec<(Intent, f32)> = sorted.into_iter().skip(1).collect();

        let mut prediction = IntentPrediction {
            primary,
            confidence,
            secondary,
            predicted_actions: vec![],
            context_hints: vec![],
        };

        let actions = self.predict_actions(&prediction);
        prediction.predicted_actions = actions;

        prediction
    }

    pub fn add_keyword_rule(&mut self, keyword: &str, intent: Intent, weight: f32) {
        self.keyword_rules
            .entry(keyword.to_lowercase())
            .or_default()
            .push((intent, weight));
    }

    pub fn predict_actions(&self, intent: &IntentPrediction) -> Vec<String> {
        match intent.primary {
            Intent::InformationSeeking => vec![
                "web_search".to_string(),
                "read_file".to_string(),
                "session_search".to_string(),
            ],
            Intent::TaskExecution => vec![
                "terminal".to_string(),
                "write_file".to_string(),
                "patch".to_string(),
            ],
            Intent::CreativeGeneration => {
                vec!["write_file".to_string(), "code_exec".to_string()]
            }
            Intent::Debugging => vec![
                "terminal".to_string(),
                "patch".to_string(),
                "search_files".to_string(),
                "read_file".to_string(),
            ],
            Intent::Planning => vec!["todo".to_string(), "session_search".to_string()],
            Intent::SocialChat => vec![],
            Intent::MetaQuery => vec!["memory".to_string(), "session_search".to_string()],
            Intent::CodeReview => vec![
                "read_file".to_string(),
                "search_files".to_string(),
                "terminal".to_string(),
            ],
            Intent::Research => vec![
                "web_search".to_string(),
                "read_file".to_string(),
                "session_search".to_string(),
            ],
            Intent::Configuration => {
                vec!["read_file".to_string(), "write_file".to_string()]
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_information_seeking() {
        let c = IntentClassifier::new();
        let p = c.classify("how does memory allocation work?");
        assert_eq!(p.primary, Intent::InformationSeeking);
        assert!(p.confidence > 0.0);
    }

    #[test]
    fn test_classify_task_execution() {
        let c = IntentClassifier::new();
        let p = c.classify("run the test suite for me");
        assert_eq!(p.primary, Intent::TaskExecution);
    }

    #[test]
    fn test_classify_debugging() {
        let c = IntentClassifier::new();
        let p = c.classify("fix this error in my code");
        assert_eq!(p.primary, Intent::Debugging);
    }

    #[test]
    fn test_classify_creative() {
        let c = IntentClassifier::new();
        let p = c.classify("create a new function for sorting");
        assert_eq!(p.primary, Intent::CreativeGeneration);
    }

    #[test]
    fn test_classify_planning() {
        let c = IntentClassifier::new();
        let p = c.classify("plan the steps for deploying this app");
        assert_eq!(p.primary, Intent::Planning);
    }

    #[test]
    fn test_classify_social() {
        let c = IntentClassifier::new();
        let p = c.classify("hello there!");
        assert_eq!(p.primary, Intent::SocialChat);
    }

    #[test]
    fn test_classify_meta() {
        let c = IntentClassifier::new();
        let p = c.classify("what can you do?");
        assert_eq!(p.primary, Intent::MetaQuery);
    }

    #[test]
    fn test_classify_code_review() {
        let c = IntentClassifier::new();
        let p = c.classify("review this code for improvements");
        assert_eq!(p.primary, Intent::CodeReview);
    }

    #[test]
    fn test_classify_with_tool_context() {
        let c = IntentClassifier::new();
        let tools = vec!["web_search".to_string()];
        let p = c.classify_with_context("research this topic", &tools);
        assert_eq!(p.primary, Intent::Research);
        assert!(p.confidence > 0.0);
    }

    #[test]
    fn test_confidence_range() {
        let c = IntentClassifier::new();
        let p = c.classify("hello");
        assert!(p.confidence >= 0.0 && p.confidence <= 1.0);
    }

    #[test]
    fn test_secondary_intents() {
        let c = IntentClassifier::new();
        let p = c.classify("how do I fix this bug? research the error");
        // Should have at least one secondary
        assert!(!p.secondary.is_empty());
    }

    #[test]
    fn test_predict_actions() {
        let c = IntentClassifier::new();
        let p = c.classify("debug this crash");
        assert!(!p.predicted_actions.is_empty());
        assert!(p.predicted_actions.contains(&"terminal".to_string()));
    }

    #[test]
    fn test_custom_keyword_rule() {
        let mut c = IntentClassifier::new();
        c.add_keyword_rule("yolo", Intent::Configuration, 0.5);
        let p = c.classify("yolo my config looks fine");
        assert_eq!(p.primary, Intent::Configuration);
    }

    #[test]
    fn test_empty_message() {
        let c = IntentClassifier::new();
        let p = c.classify("");
        assert_eq!(p.primary, Intent::SocialChat);
        assert!(p.confidence >= 0.0 && p.confidence <= 1.0);
    }

    #[test]
    fn test_mixed_signals() {
        let c = IntentClassifier::new();
        let p = c.classify("why does this error happen when I run the code?");
        // Should have a primary intent and confidence
        assert!(p.confidence > 0.0);
        // Should have some secondary intents since there are mixed signals
        assert!(!p.secondary.is_empty());
    }
}
