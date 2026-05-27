use crate::Tool;
use async_trait::async_trait;
use chrono::Utc;
use hakimi_common::{HakimiError, Result, ToolContext};
use hakimi_cron::persistence::PersistentCronStore;
use hakimi_cron::{CronJob, CronRepeat, parse_schedule, validate_cron_prompt};
use serde_json::{Value as JsonValue, json};

pub struct CronjobTool;

impl Default for CronjobTool {
    fn default() -> Self {
        Self::new()
    }
}

impl CronjobTool {
    pub fn new() -> Self {
        Self {}
    }

    fn get_store() -> std::result::Result<PersistentCronStore, HakimiError> {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
        let cron_db_path = std::path::PathBuf::from(home)
            .join(".hakimi")
            .join("cron.db");
        PersistentCronStore::open(&cron_db_path)
            .map_err(|e| HakimiError::Tool(format!("Failed to open cron DB: {e}")))
    }

    fn string_array(args: &JsonValue, key: &str) -> Option<Vec<String>> {
        args.get(key).and_then(|v| v.as_array()).map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::trim))
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect()
        })
    }

    fn repeat_arg(args: &JsonValue) -> Result<Option<Option<u32>>> {
        let Some(value) = args.get("repeat") else {
            return Ok(None);
        };
        if value.is_null() {
            return Ok(Some(None));
        }
        if let Some(repeat) = value.as_i64() {
            if repeat <= 0 {
                return Ok(Some(None));
            }
            return u32::try_from(repeat)
                .map(|repeat| Some(Some(repeat)))
                .map_err(|_| HakimiError::Tool("repeat is too large".into()));
        }
        Err(HakimiError::Tool("repeat must be an integer".into()))
    }
}

#[async_trait]
impl Tool for CronjobTool {
    fn name(&self) -> &str {
        "cronjob"
    }
    fn toolset(&self) -> &str {
        "cron"
    }
    fn description(&self) -> &str {
        "Manage scheduled cron jobs. Actions: create, list, update, pause, resume, remove, run."
    }
    fn emoji(&self) -> &str {
        "⏰"
    }

    fn schema(&self) -> JsonValue {
        json!({
            "type": "object",
            "properties": {
                "action": { "type": "string", "enum": ["create", "list", "update", "pause", "resume", "remove", "run"] },
                "job_id": { "type": "string" },
                "name": { "type": "string" },
                "schedule": { "type": "string" },
                "prompt": { "type": "string" },
                "skills": { "type": "array", "items": { "type": "string" } },
                "enabled_toolsets": { "type": "array", "items": { "type": "string" } },
                "context_from": { "type": "array", "items": { "type": "string" } },
                "deliver": { "type": "string" },
                "repeat": { "type": "integer" }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, args: &JsonValue, _ctx: &ToolContext) -> Result<String> {
        let action = args
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("list");
        let store = Self::get_store()?;

        match action {
            "list" => {
                let jobs = store
                    .load_all()
                    .map_err(|e| HakimiError::Tool(e.to_string()))?;
                if jobs.is_empty() {
                    return Ok("No scheduled cron jobs.".to_string());
                }
                let mut out = "⏰ Scheduled Cron Jobs:\n".to_string();
                for j in jobs {
                    let status = if j.enabled {
                        "🟢 Active"
                    } else {
                        "⏸️ Paused"
                    };
                    out.push_str(&format!(
                        "- [{}] ID: `{}` | Schedule: `{:?}` | Repeat: `{}` | Prompt: `{}`\n",
                        status,
                        j.id,
                        j.schedule,
                        j.repeat
                            .times
                            .map(|times| format!("{}/{}", j.repeat.completed, times))
                            .unwrap_or_else(|| "∞".to_string()),
                        j.prompt
                    ));
                }
                Ok(out)
            }
            "create" => {
                let prompt = args
                    .get("prompt")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| HakimiError::Tool("prompt is required".into()))?;
                validate_cron_prompt(prompt).map_err(|e| HakimiError::Tool(e.to_string()))?;
                let schedule_str = args
                    .get("schedule")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| HakimiError::Tool("schedule is required".into()))?;
                let name = args
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unnamed job")
                    .to_string();

                let parsed_schedule =
                    parse_schedule(schedule_str).map_err(|e| HakimiError::Tool(e.to_string()))?;
                let next_run = Some(parsed_schedule.next_after(Utc::now()));

                let mut job = CronJob::new(&name, parsed_schedule, prompt);
                job.next_run = next_run;
                if let Some(repeat) = Self::repeat_arg(args)? {
                    job.repeat = CronRepeat::new(repeat);
                }

                if let Some(arr) = args.get("skills").and_then(|v| v.as_array()) {
                    job.skills = arr
                        .iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect();
                }
                if let Some(arr) = args.get("enabled_toolsets").and_then(|v| v.as_array()) {
                    job.enabled_toolsets = Some(
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect(),
                    );
                }
                if let Some(arr) = args.get("context_from").and_then(|v| v.as_array()) {
                    job.context_from = arr
                        .iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect();
                }
                if let Some(d) = args.get("deliver").and_then(|v| v.as_str()) {
                    job.deliver = Some(d.to_string());
                }

                store
                    .save_job(&job)
                    .map_err(|e| HakimiError::Tool(e.to_string()))?;
                Ok(format!(
                    "Created cron job `{}` with schedule `{}`. Next run at: {:?}",
                    job.id, schedule_str, job.next_run
                ))
            }
            "update" => {
                let job_id = args
                    .get("job_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| HakimiError::Tool("job_id is required".into()))?;
                let mut job = store
                    .get_job(job_id)
                    .map_err(|e| HakimiError::Tool(e.to_string()))?
                    .ok_or_else(|| HakimiError::Tool("Job not found".into()))?;
                let mut changed = false;

                if let Some(name) = args.get("name").and_then(|v| v.as_str()) {
                    job.name = name.trim().to_string();
                    changed = true;
                }
                if let Some(prompt) = args.get("prompt").and_then(|v| v.as_str()) {
                    validate_cron_prompt(prompt).map_err(|e| HakimiError::Tool(e.to_string()))?;
                    job.prompt = prompt.to_string();
                    changed = true;
                }
                if let Some(schedule_str) = args.get("schedule").and_then(|v| v.as_str()) {
                    let parsed_schedule = parse_schedule(schedule_str)
                        .map_err(|e| HakimiError::Tool(e.to_string()))?;
                    job.schedule = parsed_schedule;
                    job.next_run = Some(job.schedule.next_after(Utc::now()));
                    changed = true;
                }
                if let Some(skills) = Self::string_array(args, "skills") {
                    job.skills = skills;
                    changed = true;
                }
                if let Some(toolsets) = Self::string_array(args, "enabled_toolsets") {
                    job.enabled_toolsets = if toolsets.is_empty() {
                        None
                    } else {
                        Some(toolsets)
                    };
                    changed = true;
                }
                if let Some(context_from) = Self::string_array(args, "context_from") {
                    job.context_from = context_from;
                    changed = true;
                }
                if args.get("deliver").is_some() {
                    job.deliver = args
                        .get("deliver")
                        .and_then(|v| v.as_str())
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(String::from);
                    changed = true;
                }
                if let Some(repeat) = Self::repeat_arg(args)? {
                    job.repeat = CronRepeat::new(repeat);
                    changed = true;
                }

                if !changed {
                    return Err(HakimiError::Tool("No updates provided".into()));
                }

                store
                    .update_job(&job)
                    .map_err(|e| HakimiError::Tool(e.to_string()))?;
                Ok(format!(
                    "Updated cron job `{}` ({}). Next run at: {:?}",
                    job.id, job.name, job.next_run
                ))
            }
            "remove" => {
                let job_id = args
                    .get("job_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| HakimiError::Tool("job_id is required".into()))?;
                let removed = store
                    .remove_job(job_id)
                    .map_err(|e| HakimiError::Tool(e.to_string()))?;
                if removed {
                    Ok(format!("Removed cron job: {}", job_id))
                } else {
                    Err(HakimiError::Tool("Job not found".into()))
                }
            }
            "pause" => {
                let job_id = args
                    .get("job_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| HakimiError::Tool("job_id is required".into()))?;
                store
                    .set_enabled(job_id, false)
                    .map_err(|e| HakimiError::Tool(e.to_string()))?;
                Ok(format!("Paused cron job: {}", job_id))
            }
            "resume" => {
                let job_id = args
                    .get("job_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| HakimiError::Tool("job_id is required".into()))?;
                store
                    .set_enabled(job_id, true)
                    .map_err(|e| HakimiError::Tool(e.to_string()))?;
                Ok(format!("Resumed cron job: {}", job_id))
            }
            "run" => {
                let job_id = args
                    .get("job_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| HakimiError::Tool("job_id is required".into()))?;
                let jobs = store
                    .load_all()
                    .map_err(|e| HakimiError::Tool(e.to_string()))?;
                let job = jobs
                    .into_iter()
                    .find(|job| job.id == job_id)
                    .ok_or_else(|| HakimiError::Tool("Job not found".into()))?;
                validate_cron_prompt(&job.prompt).map_err(|e| HakimiError::Tool(e.to_string()))?;
                let now = Utc::now();
                store
                    .trigger_now(&job.id, now)
                    .map_err(|e| HakimiError::Tool(e.to_string()))?;
                Ok(format!(
                    "Triggered cron job `{}` ({}) for the next scheduler tick at {}",
                    job.id,
                    job.name,
                    now.to_rfc3339()
                ))
            }
            _ => Err(HakimiError::Tool(format!("Unsupported action: {}", action))),
        }
    }
}
