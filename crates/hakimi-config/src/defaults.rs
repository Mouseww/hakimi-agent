use std::collections::HashMap;
use std::sync::LazyLock;

/// Build the default configuration as a serde_json::Value.
pub fn default_config_value() -> serde_json::Value {
    serde_json::json!({
        "model": {
            "default": "",
            "provider": "auto",
            "base_url": ""
        },
        "terminal": {
            "env_type": "local",
            "cwd": ".",
            "timeout": 60,
            "docker_image": "",
            "docker_forward_env": [],
            "docker_volumes": []
        },
        "agent": {
            "max_turns": 90,
            "verbose": false,
            "system_prompt": "",
            "reasoning_effort": "",
            "service_tier": "",
            "disabled_toolsets": []
        },
        "compression": {
            "enabled": true,
            "threshold": 0.50,
            "target_ratio": 0.20,
            "engine": "smart",
            "model": "",
            "context_length": 128000
        },
        "display": {
            "compact": false,
            "streaming": true,
            "skin": "default"
        },
        "delegation": {
            "max_iterations": 45,
            "model": "",
            "provider": "",
            "base_url": "",
            "api_key": ""
        },
        "embedding": {
            "enabled": true,
            "provider": "openai-compatible",
            "base_url": "same-as-llm",
            "api_key": "same-as-llm",
            "model": "BAAI/bge-m3",
            "dimension": 1024,
            "batch_size": 32,
            "normalize": true
        },
        "tools": {
            "tool_search": {
                "enabled": "auto",
                "threshold_pct": 10.0,
                "search_default_limit": 5,
                "max_search_limit": 20
            },
            "output": {
                "max_bytes": 50000
            }
        }
    })
}

/// Flat key-value map of all default settings for quick lookup.
pub static DEFAULT_FLAT: LazyLock<HashMap<String, serde_json::Value>> = LazyLock::new(|| {
    let mut map = HashMap::new();
    let defaults = default_config_value();
    if let Some(obj) = defaults.as_object() {
        for (section, section_val) in obj {
            if let Some(inner) = section_val.as_object() {
                for (key, value) in inner {
                    map.insert(format!("{section}.{key}"), value.clone());
                }
            }
        }
    }
    map
});
