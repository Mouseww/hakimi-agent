use async_trait::async_trait;
use hakimi_common::{HakimiError, Result, ToolContext};
use serde_json::{Value as JsonValue, json};
use crate::Tool;
use hakimi_cron::persistence::PersistentCronStore;
use hakimi_cron::{CronJob, parse_schedule};
use chrono::Utc;

pub struct CronjobTool;

impl CronjobTool {
    pub fn new() -> Self {
        Self {}
    }
    
    fn get_store() -> std::result::Result<PersistentCronStore, HakimiError> {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
        let cron_db_path = std::path::PathBuf::from(home).join(".hakimi").join("cron.db");
        PersistentCronStore::open(&cron_db_path)
            .map_err(|e| HakimiError::Tool(format!("Failed to open cron DB: {e}")))
    }
}

#[async_trait]
impl Tool for CronjobTool {
    fn name(&self) -> &str { "cronjob" }
    fn toolset(&self) -> &str { "cron" }
    fn description(&self) -> &str { "Manage scheduled cron jobs. Actions: create, list, update, pause, resume, remove, run." }
    fn emoji(&self) -> &str { "⏰" }

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
                "deliver": { "type": "string" }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, args: &JsonValue, _ctx: &ToolContext) -> Result<String> {
        let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("list");
        let store = Self::get_store()?;

        match action {
            "list" => {
                let jobs = store.load_all().map_err(|e| HakimiError::Tool(e.to_string()))?;
                if jobs.is_empty() {
                    return Ok("No scheduled cron jobs.".to_string());
                }
                let mut out = "⏰ Scheduled Cron Jobs:\n".to_string();
                for j in jobs {
                    let status = if j.enabled { "🟢 Active" } else { "⏸️ Paused" };
                    out.push_str(&format!("- [{}] ID: `{}` | Schedule: `{:?}` | Prompt: `{}`\n", status, j.id, j.schedule, j.prompt));
                }
                Ok(out)
            }
            "create" => {
                let prompt = args.get("prompt").and_then(|v| v.as_str()).ok_or_else(|| HakimiError::Tool("prompt is required".into()))?;
                let schedule_str = args.get("schedule").and_then(|v| v.as_str()).ok_or_else(|| HakimiError::Tool("schedule is required".into()))?;
                let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("unnamed job").to_string();
                
                let parsed_schedule = parse_schedule(schedule_str).map_err(|e| HakimiError::Tool(e.to_string()))?;
                let next_run = Some(parsed_schedule.next_after(Utc::now()));

                let mut job = CronJob::new(&name, parsed_schedule, prompt);
                job.next_run = next_run;

                if let Some(arr) = args.get("skills").and_then(|v| v.as_array()) {
                    job.skills = arr.iter().filter_map(|v| v.as_str().map(String::from)).collect();
                }
                if let Some(arr) = args.get("enabled_toolsets").and_then(|v| v.as_array()) {
                    job.enabled_toolsets = Some(arr.iter().filter_map(|v| v.as_str().map(String::from)).collect());
                }
                if let Some(arr) = args.get("context_from").and_then(|v| v.as_array()) {
                    job.context_from = arr.iter().filter_map(|v| v.as_str().map(String::from)).collect();
                }
                if let Some(d) = args.get("deliver").and_then(|v| v.as_str()) {
                    job.deliver = Some(d.to_string());
                }

                store.save_job(&job).map_err(|e| HakimiError::Tool(e.to_string()))?;
                Ok(format!("Created cron job `{}` with schedule `{}`. Next run at: {:?}", job.id, schedule_str, job.next_run))
            }
            "remove" => {
                let job_id = args.get("job_id").and_then(|v| v.as_str()).ok_or_else(|| HakimiError::Tool("job_id is required".into()))?;
                let removed = store.remove_job(job_id).map_err(|e| HakimiError::Tool(e.to_string()))?;
                if removed {
                    Ok(format!("Removed cron job: {}", job_id))
                } else {
                    Err(HakimiError::Tool("Job not found".into()))
                }
            }
            "pause" => {
                let job_id = args.get("job_id").and_then(|v| v.as_str()).ok_or_else(|| HakimiError::Tool("job_id is required".into()))?;
                store.set_enabled(job_id, false).map_err(|e| HakimiError::Tool(e.to_string()))?;
                Ok(format!("Paused cron job: {}", job_id))
            }
            "resume" => {
                let job_id = args.get("job_id").and_then(|v| v.as_str()).ok_or_else(|| HakimiError::Tool("job_id is required".into()))?;
                store.set_enabled(job_id, true).map_err(|e| HakimiError::Tool(e.to_string()))?;
                Ok(format!("Resumed cron job: {}", job_id))
            }
            _ => Err(HakimiError::Tool(format!("Unsupported action: {}", action)))
        }
    }
}
