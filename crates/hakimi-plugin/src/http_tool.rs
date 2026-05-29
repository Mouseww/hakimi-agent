use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use hakimi_common::{HakimiError, Result, ToolContext};
use hakimi_tools::Tool;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use tracing::debug;

/// Configuration for a single HTTP-backed tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpToolConfig {
    /// Tool name (used for dispatch).
    pub name: String,
    /// HTTP method (GET, POST, PUT, DELETE, PATCH).
    pub method: String,
    /// URL endpoint. Supports `{param}` placeholders that are interpolated from arguments.
    pub endpoint: String,
    /// Human-readable description.
    pub description: String,
    /// JSON Schema for the tool's parameters.
    #[serde(default = "default_schema")]
    pub parameters: JsonValue,
    /// Optional static headers to send with every request.
    #[serde(default)]
    pub headers: HashMap<String, String>,
    /// Optional default body template (JSON). Placeholders `{param}` are interpolated.
    #[serde(default)]
    pub body_template: Option<JsonValue>,
}

fn default_schema() -> JsonValue {
    serde_json::json!({ "type": "object", "properties": {} })
}

/// Configuration for a plugin that exposes HTTP endpoints as tools.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpPluginConfig {
    /// Plugin name.
    pub name: String,
    /// Optional semver-style plugin version.
    #[serde(default)]
    pub version: Option<String>,
    /// Optional human-readable plugin description.
    #[serde(default)]
    pub description: Option<String>,
    /// List of HTTP tool definitions.
    pub tools: Vec<HttpToolConfig>,
}

/// A tool that wraps an HTTP endpoint, implementing the [`Tool`] trait.
pub struct HttpTool {
    config: HttpToolConfig,
    client: reqwest::Client,
}

impl HttpTool {
    /// Create a new HTTP tool from a configuration.
    pub fn new(config: HttpToolConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
        }
    }

    /// Interpolate `{param}` placeholders in `template` using the JSON arguments.
    fn interpolate(template: &str, args: &JsonValue) -> String {
        let mut result = template.to_string();
        if let Some(obj) = args.as_object() {
            for (key, value) in obj {
                let placeholder = format!("{{{key}}}");
                let replacement = match value {
                    JsonValue::String(s) => s.clone(),
                    other => other.to_string(),
                };
                result = result.replace(&placeholder, &replacement);
            }
        }
        result
    }
}

#[async_trait]
impl Tool for HttpTool {
    fn name(&self) -> &str {
        &self.config.name
    }

    fn toolset(&self) -> &str {
        "http"
    }

    fn description(&self) -> &str {
        &self.config.description
    }

    fn emoji(&self) -> &str {
        "\u{1f310}" // 🌐
    }

    fn schema(&self) -> JsonValue {
        self.config.parameters.clone()
    }

    async fn execute(&self, args: &JsonValue, _ctx: &ToolContext) -> Result<String> {
        let url = Self::interpolate(&self.config.endpoint, args);
        let method = self.config.method.to_uppercase();

        debug!(tool = %self.config.name, %method, %url, "executing http tool");

        let mut builder = match method.as_str() {
            "GET" => self.client.get(&url),
            "POST" => self.client.post(&url),
            "PUT" => self.client.put(&url),
            "DELETE" => self.client.delete(&url),
            "PATCH" => self.client.patch(&url),
            other => {
                return Err(HakimiError::Tool(format!(
                    "unsupported HTTP method: {other}"
                )));
            }
        };

        // Add static headers
        for (key, value) in &self.config.headers {
            builder = builder.header(key.as_str(), value.as_str());
        }

        // Add body for non-GET requests
        if method != "GET" {
            if let Some(ref body_tmpl) = self.config.body_template {
                let body_str = Self::interpolate(&body_tmpl.to_string(), args);
                if let Ok(body_val) = serde_json::from_str::<JsonValue>(&body_str) {
                    builder = builder.json(&body_val);
                } else {
                    builder = builder.body(body_str);
                }
            } else if !args.is_null() {
                builder = builder.json(args);
            }
        }

        let response = builder
            .send()
            .await
            .map_err(|e| HakimiError::Tool(format!("HTTP request failed: {e}")))?;

        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| HakimiError::Tool(format!("failed to read HTTP response: {e}")))?;

        if !status.is_success() {
            return Err(HakimiError::Tool(format!(
                "HTTP {} {}: status {status}: {body}",
                self.config.method, self.config.endpoint,
            )));
        }

        Ok(body)
    }
}

/// An HTTP-backed plugin that wraps multiple HTTP endpoints as tools.
pub struct HttpToolPlugin {
    plugin_name: String,
    version: String,
    description: String,
    tools: Vec<Arc<dyn Tool>>,
}

impl HttpToolPlugin {
    /// Create an HTTP tool plugin from a YAML/JSON config value.
    pub fn from_config(config: &HttpPluginConfig) -> Self {
        let tools = config
            .tools
            .iter()
            .map(|tc| Arc::new(HttpTool::new(tc.clone())) as Arc<dyn Tool>)
            .collect();
        Self {
            plugin_name: config.name.clone(),
            version: config
                .version
                .clone()
                .unwrap_or_else(|| "0.2.1".to_string()),
            description: config
                .description
                .clone()
                .unwrap_or_else(|| "HTTP endpoint plugin".to_string()),
            tools,
        }
    }
}

impl Plugin for HttpToolPlugin {
    fn name(&self) -> &str {
        &self.plugin_name
    }

    fn version(&self) -> &str {
        &self.version
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn tools(&self) -> Vec<Arc<dyn Tool>> {
        self.tools.clone()
    }

    fn init(&mut self, _config: &JsonValue) -> Result<()> {
        // No additional initialization needed — tools are built at construction time.
        Ok(())
    }
}

use crate::Plugin;
