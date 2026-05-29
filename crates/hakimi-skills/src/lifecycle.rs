use std::collections::HashSet;

use crate::skill::{HarnessPhase, Skill};

/// How much of a skill should be rendered into the active prompt context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillRepresentation {
    /// Render the full markdown body.
    Full,
    /// Render a compact checklist/preview extracted from the body.
    Checklist,
    /// Render only name, description and tags.
    Summary,
    /// Keep only the identifier in lifecycle state; do not render content.
    IdOnly,
}

impl SkillRepresentation {
    fn downgrade(self) -> Option<Self> {
        match self {
            Self::Full => Some(Self::Checklist),
            Self::Checklist => Some(Self::Summary),
            Self::Summary => Some(Self::IdOnly),
            Self::IdOnly => None,
        }
    }
}

/// Runtime metadata for a skill currently considered by the agent.
#[derive(Debug, Clone)]
pub struct ActiveSkill {
    pub skill: Skill,
    pub representation: SkillRepresentation,
    pub relevance: f32,
    pub loaded_at_step: u64,
    pub last_used_step: u64,
    pub usage_count: u32,
    pub pinned: bool,
}

impl ActiveSkill {
    fn rendered_cost(&self) -> usize {
        match self.representation {
            SkillRepresentation::Full => self.skill.context_cost(),
            SkillRepresentation::Checklist => self.skill.checklist().len(),
            SkillRepresentation::Summary => self.skill.summary().len(),
            SkillRepresentation::IdOnly => 0,
        }
    }

    fn render(&self) -> Option<String> {
        match self.representation {
            SkillRepresentation::Full => Some(format!(
                "### {}\n{}\n\n{}",
                self.skill.name,
                self.skill.description,
                self.skill.render_body_capped()
            )),
            SkillRepresentation::Checklist => Some(format!(
                "### {}\n{}\n\n{}",
                self.skill.name,
                self.skill.description,
                self.skill.checklist()
            )),
            SkillRepresentation::Summary => Some(self.skill.summary()),
            SkillRepresentation::IdOnly => None,
        }
    }
}

/// A skill that was removed from active context but can be reactivated later.
#[derive(Debug, Clone)]
pub struct EvictedSkill {
    pub name: String,
    pub reason: String,
    pub summary: String,
    pub evicted_at_step: u64,
}

/// Per-run dynamic working set. Skills are activated, downgraded, and evicted
/// rather than appended permanently to the system prompt.
#[derive(Debug, Clone)]
pub struct SkillWorkingSet {
    active: Vec<ActiveSkill>,
    evicted: Vec<EvictedSkill>,
    banned: HashSet<String>,
    step: u64,
    current_phase: HarnessPhase,
    max_active_skills: usize,
    skill_budget_chars: usize,
}

impl Default for SkillWorkingSet {
    fn default() -> Self {
        Self::new()
    }
}

impl SkillWorkingSet {
    pub fn new() -> Self {
        Self {
            active: Vec::new(),
            evicted: Vec::new(),
            banned: HashSet::new(),
            step: 0,
            current_phase: HarnessPhase::Analyze,
            max_active_skills: 6,
            skill_budget_chars: 8_000,
        }
    }

    pub fn with_budget(mut self, max_active_skills: usize, skill_budget_chars: usize) -> Self {
        self.max_active_skills = max_active_skills;
        self.skill_budget_chars = skill_budget_chars;
        self
    }

    pub fn step(&self) -> u64 {
        self.step
    }

    pub fn current_phase(&self) -> HarnessPhase {
        self.current_phase
    }

    pub fn ban(&mut self, name: impl Into<String>) {
        let name = name.into();
        self.banned.insert(name.clone());
        self.deactivate(&name, "banned for this run");
    }

    pub fn observe(&mut self, message: &str, skills: &[Skill]) {
        self.step += 1;
        let next_phase = HarnessPhase::classify(message);
        if next_phase != self.current_phase {
            self.current_phase = next_phase;
            self.evict_phase_mismatches();
        }

        let mut candidates: Vec<(&Skill, f32)> = skills
            .iter()
            .filter(|skill| !self.banned.contains(&skill.name))
            .map(|skill| (skill, skill.relevance_score(message, self.current_phase)))
            .filter(|(_, score)| *score > 0.0)
            .collect();

        candidates.sort_by(|(_, a), (_, b)| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));

        for (skill, score) in candidates.into_iter().take(self.max_active_skills) {
            self.activate_or_update(skill.clone(), score);
        }

        self.decay_unused();
        self.enforce_budget();
    }

    pub fn observe_tool_result(&mut self, tool_result: &str, skills: &[Skill]) {
        self.observe(tool_result, skills);
    }

    pub fn render_context(&self) -> String {
        let rendered: Vec<String> = self.active.iter().filter_map(ActiveSkill::render).collect();

        if rendered.is_empty() {
            return String::new();
        }

        format!(
            "## Runtime Skills\n\
The following skills are the current working set for phase `{}`. They are dynamic: use them when relevant, ignore them when not relevant, and expect skills to be added, downgraded, or removed as the task phase changes.\n\n{}",
            self.current_phase.as_str(),
            rendered.join("\n\n")
        )
    }

    pub fn active_skill_names(&self) -> Vec<String> {
        self.active.iter().map(|s| s.skill.name.clone()).collect()
    }

    pub fn evicted(&self) -> &[EvictedSkill] {
        &self.evicted
    }

    fn activate_or_update(&mut self, skill: Skill, relevance: f32) {
        if let Some(active) = self.active.iter_mut().find(|s| s.skill.name == skill.name) {
            active.relevance = active.relevance.max(relevance);
            active.last_used_step = self.step;
            active.usage_count = active.usage_count.saturating_add(1);
            if active.representation == SkillRepresentation::IdOnly {
                active.representation = preferred_representation(&active.skill, self.current_phase);
            }
            return;
        }

        self.active.push(ActiveSkill {
            representation: preferred_representation(&skill, self.current_phase),
            skill,
            relevance,
            loaded_at_step: self.step,
            last_used_step: self.step,
            usage_count: 1,
            pinned: false,
        });
    }

    fn deactivate(&mut self, name: &str, reason: &str) {
        let mut idx = 0;
        while idx < self.active.len() {
            if self.active[idx].skill.name == name && !self.active[idx].pinned {
                let active = self.active.remove(idx);
                self.evicted.push(EvictedSkill {
                    name: active.skill.name.clone(),
                    reason: reason.to_string(),
                    summary: active.skill.summary(),
                    evicted_at_step: self.step,
                });
            } else {
                idx += 1;
            }
        }
    }

    fn evict_phase_mismatches(&mut self) {
        let current_phase = self.current_phase;
        let mut idx = 0;
        while idx < self.active.len() {
            let should_evict = !self.active[idx].pinned
                && !self.active[idx].skill.applies_to_phase(current_phase)
                && self.step.saturating_sub(self.active[idx].last_used_step)
                    >= self.active[idx].skill.ttl_steps as u64;

            if should_evict {
                let active = self.active.remove(idx);
                self.evicted.push(EvictedSkill {
                    name: active.skill.name.clone(),
                    reason: format!("phase changed to {}", current_phase.as_str()),
                    summary: active.skill.summary(),
                    evicted_at_step: self.step,
                });
            } else {
                idx += 1;
            }
        }
    }

    fn decay_unused(&mut self) {
        let current_phase = self.current_phase;
        for active in &mut self.active {
            if active.last_used_step < self.step {
                active.relevance *= if active.skill.applies_to_phase(current_phase) {
                    0.92
                } else {
                    0.65
                };
            }
        }

        let mut idx = 0;
        while idx < self.active.len() {
            let age = self.step.saturating_sub(self.active[idx].last_used_step);
            let expired = !self.active[idx].pinned
                && age >= self.active[idx].skill.ttl_steps as u64
                && self.active[idx].relevance < 0.25;
            if expired {
                let active = self.active.remove(idx);
                self.evicted.push(EvictedSkill {
                    name: active.skill.name.clone(),
                    reason: "expired due to low relevance".to_string(),
                    summary: active.skill.summary(),
                    evicted_at_step: self.step,
                });
            } else {
                idx += 1;
            }
        }
    }

    fn enforce_budget(&mut self) {
        while self.active.len() > self.max_active_skills {
            if !self.evict_lowest("active skill limit exceeded") {
                break;
            }
        }

        loop {
            let total: usize = self.active.iter().map(ActiveSkill::rendered_cost).sum();
            if total <= self.skill_budget_chars {
                break;
            }

            if self.downgrade_lowest() {
                continue;
            }

            if !self.evict_lowest("skill context budget exceeded") {
                break;
            }
        }
    }

    fn downgrade_lowest(&mut self) -> bool {
        let Some(idx) = self.lowest_scored_index(true) else {
            return false;
        };
        let Some(next) = self.active[idx].representation.downgrade() else {
            return false;
        };
        self.active[idx].representation = next;
        true
    }

    fn evict_lowest(&mut self, reason: &str) -> bool {
        let Some(idx) = self.lowest_scored_index(false) else {
            return false;
        };
        let active = self.active.remove(idx);
        self.evicted.push(EvictedSkill {
            name: active.skill.name.clone(),
            reason: reason.to_string(),
            summary: active.skill.summary(),
            evicted_at_step: self.step,
        });
        true
    }

    fn lowest_scored_index(&self, require_downgradable: bool) -> Option<usize> {
        self.active
            .iter()
            .enumerate()
            .filter(|(_, skill)| !skill.pinned)
            .filter(|(_, skill)| {
                !require_downgradable || skill.representation.downgrade().is_some()
            })
            .min_by(|(_, a), (_, b)| {
                score(a, self.step, self.current_phase)
                    .partial_cmp(&score(b, self.step, self.current_phase))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(idx, _)| idx)
    }
}

fn preferred_representation(skill: &Skill, phase: HarnessPhase) -> SkillRepresentation {
    if skill.context_cost() <= 1_200 && skill.applies_to_phase(phase) {
        SkillRepresentation::Full
    } else if skill.applies_to_phase(phase) {
        SkillRepresentation::Checklist
    } else {
        SkillRepresentation::Summary
    }
}

fn score(skill: &ActiveSkill, step: u64, phase: HarnessPhase) -> f32 {
    let phase_match = if skill.skill.applies_to_phase(phase) {
        1.0
    } else {
        0.35
    };
    let recency = 1.0 / (1.0 + step.saturating_sub(skill.last_used_step) as f32);
    let usage = (skill.usage_count as f32).ln_1p();
    let cost_penalty = skill.rendered_cost() as f32 / 4_000.0;

    skill.relevance * 3.0 + phase_match * 2.0 + recency + usage - cost_penalty
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skill(name: &str, tags: &[&str], phases: Vec<HarnessPhase>, body: &str) -> Skill {
        Skill {
            name: name.to_string(),
            description: format!("{name} description"),
            content: body.to_string(),
            trigger: None,
            tags: tags.iter().map(|tag| tag.to_string()).collect(),
            phases,
            ttl_steps: 1,
            max_context_chars: None,
            provenance: crate::skill::SkillProvenance::default(),
            metadata: crate::skill::SkillMetadata::default(),
        }
    }

    #[test]
    fn evicts_stale_phase_mismatch() {
        let skills = vec![
            skill(
                "repo-search",
                &["search"],
                vec![HarnessPhase::Analyze],
                "# Repo Search\n- Search the codebase",
            ),
            skill(
                "safe-editing",
                &["modify"],
                vec![HarnessPhase::Implement],
                "# Safe Editing\n- Make a minimal patch",
            ),
        ];
        let mut set = SkillWorkingSet::new();

        set.observe("search the codebase", &skills);
        assert_eq!(set.active_skill_names(), vec!["repo-search".to_string()]);

        set.observe("modify and implement the fix", &skills);
        let active = set.active_skill_names();
        assert!(active.contains(&"safe-editing".to_string()));
        assert!(!active.contains(&"repo-search".to_string()));
        assert!(set.evicted().iter().any(|s| s.name == "repo-search"));
    }

    #[test]
    fn enforces_budget_by_downgrading_or_evicting() {
        let skills = vec![skill(
            "large-rust-skill",
            &["rust"],
            vec![HarnessPhase::Analyze],
            &"# Large\n".repeat(1_000),
        )];
        let mut set = SkillWorkingSet::new().with_budget(1, 80);

        set.observe("rust analyze", &skills);
        assert!(set.render_context().len() <= 500);
    }
}
