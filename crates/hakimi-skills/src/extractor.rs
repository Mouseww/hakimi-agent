use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PatternType {
    ToolSequence,
    ErrorFixCycle,
    SearchRefine,
    FileEditCycle,
    DelegatePattern,
    ConfigPattern,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedPattern {
    pub pattern_type: PatternType,
    pub description: String,
    pub tool_sequence: Vec<String>,
    pub frequency: usize,
    pub confidence: f32,
    pub examples: Vec<String>,
}

pub struct SkillExtractor {
    min_pattern_frequency: usize,
    min_confidence: f32,
}

impl SkillExtractor {
    /// Create a new extractor with default thresholds.
    pub fn new() -> Self {
        Self {
            min_pattern_frequency: 2,
            min_confidence: 0.5,
        }
    }

    /// Create an extractor with custom thresholds.
    pub fn with_thresholds(min_frequency: usize, min_confidence: f32) -> Self {
        Self {
            min_pattern_frequency: min_frequency,
            min_confidence,
        }
    }

    /// Extract patterns from a session's (role, tool_calls) pairs.
    pub fn extract_from_session(
        &self,
        messages: &[(String, Vec<String>)],
    ) -> Vec<ExtractedPattern> {
        let mut patterns = Vec::new();

        // Collect all tool call sequences
        let tool_sequences: Vec<Vec<String>> = messages
            .iter()
            .filter(|(_, tools)| !tools.is_empty())
            .map(|(_, tools)| tools.clone())
            .collect();

        // Detect repeated tool sequences
        if let Some(p) = self.detect_tool_sequences(&tool_sequences) {
            patterns.push(p);
        }

        // Detect error-fix cycles
        if let Some(p) = self.detect_error_fix_cycle(messages) {
            patterns.push(p);
        }

        // Detect search-refine loops
        if let Some(p) = self.detect_search_refine(messages) {
            patterns.push(p);
        }

        // Detect file edit cycles
        if let Some(p) = self.detect_file_edit_cycle(messages) {
            patterns.push(p);
        }

        // Detect delegate patterns
        if let Some(p) = self.detect_delegate_pattern(messages) {
            patterns.push(p);
        }

        // Detect config patterns
        if let Some(p) = self.detect_config_pattern(messages) {
            patterns.push(p);
        }

        // Filter by thresholds
        patterns
            .into_iter()
            .filter(|p| {
                p.frequency >= self.min_pattern_frequency && p.confidence >= self.min_confidence
            })
            .collect()
    }

    fn detect_tool_sequences(&self, tool_sequences: &[Vec<String>]) -> Option<ExtractedPattern> {
        if tool_sequences.len() < 2 {
            return None;
        }

        // Find repeated 2+ tool call sequences
        use std::collections::HashMap;
        let mut seq_counts: HashMap<Vec<String>, usize> = HashMap::new();
        let mut examples = Vec::new();

        for seq in tool_sequences {
            if seq.len() >= 2 {
                let key = seq.clone();
                *seq_counts.entry(key.clone()).or_insert(0) += 1;
                if examples.len() < 3 {
                    examples.push(format!("{:?}", seq));
                }
            }
        }

        let best = seq_counts.iter().max_by_key(|(_, count)| *count)?;

        if *best.1 < self.min_pattern_frequency {
            return None;
        }

        Some(ExtractedPattern {
            pattern_type: PatternType::ToolSequence,
            description: format!("Repeated tool call sequence: {:?}", best.0),
            tool_sequence: best.0.clone(),
            frequency: *best.1,
            confidence: self.score_from_frequency(*best.1, tool_sequences.len()),
            examples,
        })
    }

    fn detect_error_fix_cycle(
        &self,
        messages: &[(String, Vec<String>)],
    ) -> Option<ExtractedPattern> {
        // Look for patterns: tool call -> error (role="user"/feedback) -> diagnostic tool -> fix tool
        let mut cycles = 0;
        let mut tool_seq = Vec::new();
        let mut examples = Vec::new();

        let tool_msgs: Vec<&(String, Vec<String>)> =
            messages.iter().filter(|(_, t)| !t.is_empty()).collect();

        if tool_msgs.len() < 3 {
            return None;
        }

        for window in tool_msgs.windows(3) {
            let (_, tools_a) = &window[0];
            let (_role_b, tools_b) = &window[1];
            let (_, tools_c) = &window[2];

            // Heuristic: error tool call, then diagnostic, then fix
            let has_error_tool = tools_a.iter().any(|t| {
                let lower = t.to_lowercase();
                lower.contains("error") || lower.contains("test") || lower.contains("run")
            });
            let has_diagnostic = tools_b.iter().any(|t| {
                let lower = t.to_lowercase();
                lower.contains("read") || lower.contains("search") || lower.contains("grep")
            });
            let has_fix = tools_c.iter().any(|t| {
                let lower = t.to_lowercase();
                lower.contains("write") || lower.contains("edit") || lower.contains("patch")
            });

            if has_error_tool && has_diagnostic && has_fix {
                cycles += 1;
                tool_seq = tools_a
                    .iter()
                    .chain(tools_b.iter())
                    .chain(tools_c.iter())
                    .cloned()
                    .collect();
                if examples.len() < 3 {
                    examples.push(format!("{:?} -> {:?} -> {:?}", tools_a, tools_b, tools_c));
                }
            }
        }

        if cycles < self.min_pattern_frequency {
            return None;
        }

        Some(ExtractedPattern {
            pattern_type: PatternType::ErrorFixCycle,
            description: "Error -> diagnosis -> fix cycle detected".to_string(),
            tool_sequence: tool_seq,
            frequency: cycles,
            confidence: self.score_from_frequency(cycles, tool_msgs.len()),
            examples,
        })
    }

    fn detect_search_refine(&self, messages: &[(String, Vec<String>)]) -> Option<ExtractedPattern> {
        let mut cycles = 0;
        let mut examples = Vec::new();

        let tool_msgs: Vec<&(String, Vec<String>)> =
            messages.iter().filter(|(_, t)| !t.is_empty()).collect();

        if tool_msgs.len() < 2 {
            return None;
        }

        for window in tool_msgs.windows(2) {
            let (_, tools_a) = &window[0];
            let (_, tools_b) = &window[1];

            let has_search = tools_a.iter().any(|t| {
                let lower = t.to_lowercase();
                lower.contains("search") || lower.contains("grep") || lower.contains("find")
            });
            let has_refine = tools_b.iter().any(|t| {
                let lower = t.to_lowercase();
                lower.contains("search")
                    || lower.contains("grep")
                    || lower.contains("find")
                    || lower.contains("read")
            });

            if has_search && has_refine && tools_a != tools_b {
                cycles += 1;
                if examples.len() < 3 {
                    examples.push(format!("{:?} -> {:?}", tools_a, tools_b));
                }
            }
        }

        if cycles < self.min_pattern_frequency {
            return None;
        }

        Some(ExtractedPattern {
            pattern_type: PatternType::SearchRefine,
            description: "Search -> refine -> search again loop detected".to_string(),
            tool_sequence: vec!["search".into(), "refine".into(), "search".into()],
            frequency: cycles,
            confidence: self.score_from_frequency(cycles, tool_msgs.len()),
            examples,
        })
    }

    fn detect_file_edit_cycle(
        &self,
        messages: &[(String, Vec<String>)],
    ) -> Option<ExtractedPattern> {
        let mut cycles = 0;
        let mut examples = Vec::new();

        let tool_msgs: Vec<&(String, Vec<String>)> =
            messages.iter().filter(|(_, t)| !t.is_empty()).collect();

        if tool_msgs.len() < 3 {
            return None;
        }

        for window in tool_msgs.windows(3) {
            let (_, tools_a) = &window[0];
            let (_, tools_b) = &window[1];
            let (_, tools_c) = &window[2];

            let has_read = tools_a.iter().any(|t| {
                let lower = t.to_lowercase();
                lower.contains("read") || lower.contains("cat")
            });
            let has_edit = tools_b.iter().any(|t| {
                let lower = t.to_lowercase();
                lower.contains("edit")
                    || lower.contains("write")
                    || lower.contains("patch")
                    || lower.contains("sed")
            });
            let has_test = tools_c.iter().any(|t| {
                let lower = t.to_lowercase();
                lower.contains("test")
                    || lower.contains("run")
                    || lower.contains("check")
                    || lower.contains("read")
            });

            if has_read && has_edit && has_test {
                cycles += 1;
                if examples.len() < 3 {
                    examples.push(format!("{:?} -> {:?} -> {:?}", tools_a, tools_b, tools_c));
                }
            }
        }

        if cycles < self.min_pattern_frequency {
            return None;
        }

        Some(ExtractedPattern {
            pattern_type: PatternType::FileEditCycle,
            description: "Read -> edit -> test cycle detected".to_string(),
            tool_sequence: vec!["read".into(), "edit".into(), "test".into()],
            frequency: cycles,
            confidence: self.score_from_frequency(cycles, tool_msgs.len()),
            examples,
        })
    }

    fn detect_delegate_pattern(
        &self,
        messages: &[(String, Vec<String>)],
    ) -> Option<ExtractedPattern> {
        let mut cycles = 0;
        let mut examples = Vec::new();

        for (_, tools) in messages {
            if tools.iter().any(|t| {
                let lower = t.to_lowercase();
                lower.contains("delegate") || lower.contains("spawn") || lower.contains("subagent")
            }) {
                cycles += 1;
                if examples.len() < 3 {
                    examples.push(format!("{:?}", tools));
                }
            }
        }

        if cycles < self.min_pattern_frequency {
            return None;
        }

        Some(ExtractedPattern {
            pattern_type: PatternType::DelegatePattern,
            description: "Task delegation pattern detected".to_string(),
            tool_sequence: vec!["delegate".into(), "aggregate".into()],
            frequency: cycles,
            confidence: self.score_from_frequency(cycles, messages.len()),
            examples,
        })
    }

    fn detect_config_pattern(
        &self,
        messages: &[(String, Vec<String>)],
    ) -> Option<ExtractedPattern> {
        let mut cycles = 0;
        let mut examples = Vec::new();

        let tool_msgs: Vec<&(String, Vec<String>)> =
            messages.iter().filter(|(_, t)| !t.is_empty()).collect();

        if tool_msgs.len() < 2 {
            return None;
        }

        for window in tool_msgs.windows(2) {
            let (_, tools_a) = &window[0];
            let (_, tools_b) = &window[1];

            let has_read_config = tools_a.iter().any(|t| {
                let lower = t.to_lowercase();
                (lower.contains("read") || lower.contains("cat"))
                    && (lower.contains("config")
                        || lower.contains("toml")
                        || lower.contains("yaml"))
            });
            let has_modify = tools_b.iter().any(|t| {
                let lower = t.to_lowercase();
                lower.contains("write") || lower.contains("edit") || lower.contains("patch")
            });

            if has_read_config && has_modify {
                cycles += 1;
                if examples.len() < 3 {
                    examples.push(format!("{:?} -> {:?}", tools_a, tools_b));
                }
            }
        }

        if cycles < self.min_pattern_frequency {
            return None;
        }

        Some(ExtractedPattern {
            pattern_type: PatternType::ConfigPattern,
            description: "Config read -> modify -> validate pattern detected".to_string(),
            tool_sequence: vec!["read_config".into(), "modify".into(), "validate".into()],
            frequency: cycles,
            confidence: self.score_from_frequency(cycles, tool_msgs.len()),
            examples,
        })
    }

    fn score_from_frequency(&self, frequency: usize, total: usize) -> f32 {
        if total == 0 {
            return 0.0;
        }
        let ratio = frequency as f32 / total as f32;
        // Clamp to [0.0, 1.0] and apply a boost for higher absolute frequency
        let base = ratio.min(1.0);
        let boost = (frequency as f32).ln().max(0.0) * 0.1;
        (base + boost).min(1.0)
    }

    /// Generate a YAML frontmatter + markdown skill from a pattern.
    pub fn generate_skill(pattern: &ExtractedPattern, name: &str) -> String {
        let tags = match pattern.pattern_type {
            PatternType::ToolSequence => vec!["tools", "automation"],
            PatternType::ErrorFixCycle => vec!["debugging", "error-fix"],
            PatternType::SearchRefine => vec!["search", "refinement"],
            PatternType::FileEditCycle => vec!["file-edit", "development"],
            PatternType::DelegatePattern => vec!["delegation", "orchestration"],
            PatternType::ConfigPattern => vec!["config", "setup"],
        };

        let mut yaml = String::new();
        yaml.push_str("---\n");
        yaml.push_str(&format!("name: {}\n", name));
        yaml.push_str(&format!("description: {}\n", pattern.description));
        yaml.push_str("tags:\n");
        for tag in &tags {
            yaml.push_str(&format!("  - {}\n", tag));
        }
        yaml.push_str("---\n\n");

        yaml.push_str(&format!("# {}\n\n", name));
        yaml.push_str(&format!("**Pattern Type:** {:?}\n\n", pattern.pattern_type));
        yaml.push_str(&format!("**Frequency:** {}\n\n", pattern.frequency));
        yaml.push_str(&format!("**Confidence:** {:.2}\n\n", pattern.confidence));
        yaml.push_str("## Tool Sequence\n\n");
        yaml.push_str("```\n");
        yaml.push_str(&pattern.tool_sequence.join(" -> "));
        yaml.push_str("\n```\n\n");

        if !pattern.examples.is_empty() {
            yaml.push_str("## Examples\n\n");
            for (i, example) in pattern.examples.iter().enumerate() {
                yaml.push_str(&format!("{}. {}\n", i + 1, example));
            }
        }

        yaml
    }

    /// Score a pattern's confidence.
    pub fn score_pattern(pattern: &ExtractedPattern) -> f32 {
        let base = pattern.confidence;
        let freq_boost = (pattern.frequency as f32).ln().max(0.0) * 0.05;
        let type_weight = match pattern.pattern_type {
            PatternType::ErrorFixCycle => 0.1,
            PatternType::FileEditCycle => 0.08,
            PatternType::ToolSequence => 0.05,
            PatternType::SearchRefine => 0.05,
            PatternType::DelegatePattern => 0.03,
            PatternType::ConfigPattern => 0.03,
        };
        (base + freq_boost + type_weight).min(1.0)
    }

    /// Merge similar patterns by combining those with the same pattern_type.
    pub fn merge_patterns(patterns: &[ExtractedPattern]) -> Vec<ExtractedPattern> {
        use std::collections::BTreeMap;

        let mut groups: BTreeMap<String, Vec<&ExtractedPattern>> = BTreeMap::new();
        for p in patterns {
            let key = format!("{:?}", p.pattern_type);
            groups.entry(key).or_default().push(p);
        }

        groups
            .into_values()
            .map(|group| {
                let first = group[0];
                let total_freq: usize = group.iter().map(|p| p.frequency).sum();
                let avg_conf: f32 =
                    group.iter().map(|p| p.confidence).sum::<f32>() / group.len() as f32;
                let all_examples: Vec<String> =
                    group.iter().flat_map(|p| p.examples.clone()).collect();
                let all_tools: Vec<String> =
                    group.iter().flat_map(|p| p.tool_sequence.clone()).collect();

                ExtractedPattern {
                    pattern_type: first.pattern_type.clone(),
                    description: first.description.clone(),
                    tool_sequence: all_tools,
                    frequency: total_freq,
                    confidence: avg_conf,
                    examples: all_examples,
                }
            })
            .collect()
    }
}

impl Default for SkillExtractor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_tool_sequence() {
        let extractor = SkillExtractor::new();
        let messages = vec![
            ("user".into(), vec!["read".into(), "edit".into()]),
            ("assistant".into(), vec!["read".into(), "edit".into()]),
            ("user".into(), vec!["read".into(), "edit".into()]),
            ("assistant".into(), vec!["search".into()]),
        ];

        let patterns = extractor.extract_from_session(&messages);
        let tool_seq = patterns
            .iter()
            .find(|p| p.pattern_type == PatternType::ToolSequence);
        assert!(tool_seq.is_some());
        let ts = tool_seq.unwrap();
        assert!(ts.frequency >= 2);
        assert!(ts.tool_sequence.contains(&"read".into()));
    }

    #[test]
    fn test_extract_error_fix_cycle() {
        let extractor = SkillExtractor::with_thresholds(2, 0.3);
        let messages = vec![
            ("assistant".into(), vec!["run_tests".into()]),
            ("assistant".into(), vec!["read_file".into()]),
            ("assistant".into(), vec!["edit_file".into()]),
            ("assistant".into(), vec!["run_tests".into()]),
            ("assistant".into(), vec!["read_file".into()]),
            ("assistant".into(), vec!["edit_file".into()]),
        ];

        let patterns = extractor.extract_from_session(&messages);
        let efc = patterns
            .iter()
            .find(|p| p.pattern_type == PatternType::ErrorFixCycle);
        assert!(efc.is_some());
        assert!(efc.unwrap().frequency >= 2);
    }

    #[test]
    fn test_extract_search_refine() {
        let extractor = SkillExtractor::with_thresholds(2, 0.3);
        let messages = vec![
            ("assistant".into(), vec!["search_files".into()]),
            ("assistant".into(), vec!["grep_content".into()]),
            ("assistant".into(), vec!["search_files".into()]),
            ("assistant".into(), vec!["grep_content".into()]),
            ("assistant".into(), vec!["search_files".into()]),
            ("assistant".into(), vec!["find_pattern".into()]),
        ];

        let patterns = extractor.extract_from_session(&messages);
        let sr = patterns
            .iter()
            .find(|p| p.pattern_type == PatternType::SearchRefine);
        assert!(sr.is_some());
    }

    #[test]
    fn test_extract_file_edit_cycle() {
        let extractor = SkillExtractor::with_thresholds(2, 0.3);
        let messages = vec![
            ("assistant".into(), vec!["read_file".into()]),
            ("assistant".into(), vec!["edit_file".into()]),
            ("assistant".into(), vec!["run_tests".into()]),
            ("assistant".into(), vec!["read_file".into()]),
            ("assistant".into(), vec!["edit_file".into()]),
            ("assistant".into(), vec!["check_output".into()]),
        ];

        let patterns = extractor.extract_from_session(&messages);
        let fec = patterns
            .iter()
            .find(|p| p.pattern_type == PatternType::FileEditCycle);
        assert!(fec.is_some());
        assert!(fec.unwrap().frequency >= 2);
    }

    #[test]
    fn test_generate_skill_yaml() {
        let pattern = ExtractedPattern {
            pattern_type: PatternType::ToolSequence,
            description: "Repeated read-edit sequence".to_string(),
            tool_sequence: vec!["read".into(), "edit".into()],
            frequency: 3,
            confidence: 0.75,
            examples: vec!["[\"read\", \"edit\"]".into()],
        };

        let skill = SkillExtractor::generate_skill(&pattern, "read-edit-skill");

        assert!(skill.starts_with("---\n"));
        assert!(skill.contains("name: read-edit-skill"));
        assert!(skill.contains("description: Repeated read-edit sequence"));
        assert!(skill.contains("tags:"));
        assert!(skill.contains("  - tools"));
        assert!(skill.contains("---\n\n# read-edit-skill"));
    }

    #[test]
    fn test_generate_skill_content() {
        let pattern = ExtractedPattern {
            pattern_type: PatternType::ErrorFixCycle,
            description: "Error -> fix cycle".to_string(),
            tool_sequence: vec!["run".into(), "read".into(), "edit".into()],
            frequency: 5,
            confidence: 0.85,
            examples: vec![
                "[\"run\", \"read\", \"edit\"]".into(),
                "[\"test\", \"grep\", \"write\"]".into(),
            ],
        };

        let skill = SkillExtractor::generate_skill(&pattern, "error-fix-cycle");

        assert!(skill.contains("**Pattern Type:** ErrorFixCycle"));
        assert!(skill.contains("**Frequency:** 5"));
        assert!(skill.contains("**Confidence:** 0.85"));
        assert!(skill.contains("## Tool Sequence"));
        assert!(skill.contains("run -> read -> edit"));
        assert!(skill.contains("## Examples"));
        assert!(skill.contains("1. [\"run\", \"read\", \"edit\"]"));
        assert!(skill.contains("2. [\"test\", \"grep\", \"write\"]"));
    }

    #[test]
    fn test_score_pattern() {
        let pattern = ExtractedPattern {
            pattern_type: PatternType::ErrorFixCycle,
            description: "test".to_string(),
            tool_sequence: vec![],
            frequency: 5,
            confidence: 0.7,
            examples: vec![],
        };

        let score = SkillExtractor::score_pattern(&pattern);
        // base 0.7 + ln(5)*0.05 ≈ 0.08 + type_weight 0.1 = ~0.88
        assert!(score > 0.7);
        assert!(score <= 1.0);
    }

    #[test]
    fn test_merge_similar_patterns() {
        let patterns = vec![
            ExtractedPattern {
                pattern_type: PatternType::ToolSequence,
                description: "seq A".to_string(),
                tool_sequence: vec!["a".into(), "b".into()],
                frequency: 3,
                confidence: 0.6,
                examples: vec!["ex1".into()],
            },
            ExtractedPattern {
                pattern_type: PatternType::ToolSequence,
                description: "seq B".to_string(),
                tool_sequence: vec!["c".into(), "d".into()],
                frequency: 2,
                confidence: 0.8,
                examples: vec!["ex2".into()],
            },
            ExtractedPattern {
                pattern_type: PatternType::ErrorFixCycle,
                description: "error fix".to_string(),
                tool_sequence: vec!["run".into()],
                frequency: 4,
                confidence: 0.7,
                examples: vec!["ex3".into()],
            },
        ];

        let merged = SkillExtractor::merge_patterns(&patterns);
        assert_eq!(merged.len(), 2); // ToolSequence merged, ErrorFixCycle separate

        let tool_seq = merged
            .iter()
            .find(|p| p.pattern_type == PatternType::ToolSequence)
            .unwrap();
        assert_eq!(tool_seq.frequency, 5); // 3 + 2
        assert!((tool_seq.confidence - 0.7).abs() < 0.01); // (0.6 + 0.8) / 2
        assert_eq!(tool_seq.examples.len(), 2);
    }

    #[test]
    fn test_min_frequency_filter() {
        let extractor = SkillExtractor::with_thresholds(5, 0.1);
        let messages = vec![
            ("assistant".into(), vec!["read".into(), "edit".into()]),
            ("assistant".into(), vec!["read".into(), "edit".into()]),
        ];

        let patterns = extractor.extract_from_session(&messages);
        // frequency=2 < min=5, should be filtered out
        assert!(patterns.is_empty());
    }

    #[test]
    fn test_empty_session() {
        let extractor = SkillExtractor::new();
        let messages: Vec<(String, Vec<String>)> = vec![];
        let patterns = extractor.extract_from_session(&messages);
        assert!(patterns.is_empty());
    }

    #[test]
    fn test_serialization_roundtrip() {
        let pattern = ExtractedPattern {
            pattern_type: PatternType::FileEditCycle,
            description: "test pattern".to_string(),
            tool_sequence: vec!["read".into(), "edit".into()],
            frequency: 3,
            confidence: 0.65,
            examples: vec!["example1".into()],
        };

        let json = serde_json::to_string(&pattern).unwrap();
        let restored: ExtractedPattern = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.pattern_type, PatternType::FileEditCycle);
        assert_eq!(restored.description, "test pattern");
        assert_eq!(restored.tool_sequence, vec!["read", "edit"]);
        assert_eq!(restored.frequency, 3);
        assert!((restored.confidence - 0.65).abs() < 0.001);
        assert_eq!(restored.examples, vec!["example1"]);
    }
}
