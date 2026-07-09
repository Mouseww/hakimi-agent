//! Home Assistant REST tools.
//!
//! Mirrors Hermes' HA tool surface while keeping the implementation native to
//! Hakimi's async Rust tool trait.

use async_trait::async_trait;
use hakimi_common::{HakimiError, Result, ToolContext};
use serde_json::{Map as JsonMap, Value as JsonValue, json};
use tracing::debug;

use crate::Tool;

const DEFAULT_HASS_URL: &str = "http://homeassistant.local:8123";
const TOOLSET: &str = "homeassistant";
const EMOJI: &str = "\u{1f3e0}";
const MAX_RESULT_SIZE: usize = 200 * 1024;
const BLOCKED_DOMAINS: &[&str] = &[
    "shell_command",
    "command_line",
    "python_script",
    "pyscript",
    "hassio",
    "rest_command",
];

#[derive(Clone, Debug)]
struct HomeAssistantConfig {
    base_url: String,
    token: String,
}

impl HomeAssistantConfig {
    fn from_env() -> Result<Self> {
        let base_url = std::env::var("HASS_URL")
            .unwrap_or_else(|_| DEFAULT_HASS_URL.to_string())
            .trim_end_matches('/')
            .to_string();
        if !base_url.starts_with("http://") && !base_url.starts_with("https://") {
            return Err(HakimiError::ToolSimple(
                "HASS_URL must start with http:// or https://".into(),
            ));
        }

        let token = std::env::var("HASS_TOKEN")
            .map(|v| v.trim().to_string())
            .unwrap_or_default();
        if token.is_empty() {
            return Err(HakimiError::ToolSimple(
                "HASS_TOKEN environment variable is required".into(),
            ));
        }

        Ok(Self { base_url, token })
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }
}

fn homeassistant_available() -> bool {
    std::env::var("HASS_TOKEN")
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false)
}

fn ha_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| {
            HakimiError::ToolSimple(format!("failed to create Home Assistant client: {e}"))
        })
}

async fn ha_get(path: &str) -> Result<JsonValue> {
    let config = HomeAssistantConfig::from_env()?;
    let client = ha_client()?;
    let url = config.url(path);
    let response = client
        .get(&url)
        .bearer_auth(&config.token)
        .header("content-type", "application/json")
        .send()
        .await
        .map_err(|e| HakimiError::ToolSimple(format!("Home Assistant request failed: {e}")))?;

    let status = response.status();
    if !status.is_success() {
        return Err(HakimiError::ToolSimple(format!(
            "Home Assistant request failed with status: {status}"
        )));
    }

    response.json::<JsonValue>().await.map_err(|e| {
        HakimiError::ToolSimple(format!("failed to parse Home Assistant response: {e}"))
    })
}

async fn ha_post(path: &str, payload: &JsonValue) -> Result<JsonValue> {
    let config = HomeAssistantConfig::from_env()?;
    let client = ha_client()?;
    let url = config.url(path);
    let response = client
        .post(&url)
        .bearer_auth(&config.token)
        .header("content-type", "application/json")
        .json(payload)
        .send()
        .await
        .map_err(|e| HakimiError::ToolSimple(format!("Home Assistant request failed: {e}")))?;

    let status = response.status();
    if !status.is_success() {
        return Err(HakimiError::ToolSimple(format!(
            "Home Assistant service call failed with status: {status}"
        )));
    }

    response.json::<JsonValue>().await.map_err(|e| {
        HakimiError::ToolSimple(format!("failed to parse Home Assistant response: {e}"))
    })
}

fn required_string<'a>(args: &'a JsonValue, key: &str) -> Result<&'a str> {
    args.get(key)
        .and_then(|v| v.as_str())
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| HakimiError::ToolSimple(format!("missing required parameter: {key}")))
}

fn optional_string<'a>(args: &'a JsonValue, key: &str) -> Option<&'a str> {
    args.get(key)
        .and_then(|v| v.as_str())
        .filter(|v| !v.is_empty())
}

fn is_valid_entity_id(value: &str) -> bool {
    let Some((domain, object)) = value.split_once('.') else {
        return false;
    };
    is_valid_entity_domain(domain) && is_valid_entity_object(object)
}

fn is_valid_entity_domain(value: &str) -> bool {
    let mut chars = value.chars();
    matches!(chars.next(), Some(c) if c == '_' || c.is_ascii_lowercase())
        && chars.all(|c| c == '_' || c.is_ascii_lowercase() || c.is_ascii_digit())
}

fn is_valid_entity_object(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|c| c == '_' || c.is_ascii_lowercase() || c.is_ascii_digit())
}

fn is_valid_service_name(value: &str) -> bool {
    let mut chars = value.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_lowercase())
        && chars.all(|c| c == '_' || c.is_ascii_lowercase() || c.is_ascii_digit())
}

fn validate_entity_id(entity_id: &str) -> Result<()> {
    if is_valid_entity_id(entity_id) {
        Ok(())
    } else {
        Err(HakimiError::ToolSimple(format!(
            "invalid entity_id format: {entity_id}"
        )))
    }
}

fn validate_service_name(kind: &str, value: &str) -> Result<()> {
    if is_valid_service_name(value) {
        Ok(())
    } else {
        Err(HakimiError::ToolSimple(format!(
            "invalid {kind} format: {value}"
        )))
    }
}

fn ensure_domain_allowed(domain: &str) -> Result<()> {
    if BLOCKED_DOMAINS.contains(&domain) {
        return Err(HakimiError::ToolSimple(format!(
            "service domain '{domain}' is blocked for security"
        )));
    }
    Ok(())
}

fn summarize_entities(states: &[JsonValue], domain: Option<&str>, area: Option<&str>) -> JsonValue {
    let area = area.map(|v| v.to_ascii_lowercase());
    let mut entities = Vec::new();

    for state in states {
        let Some(entity_id) = state.get("entity_id").and_then(|v| v.as_str()) else {
            continue;
        };
        if let Some(domain) = domain
            && !entity_id.starts_with(&format!("{domain}."))
        {
            continue;
        }

        let attributes = state.get("attributes").and_then(|v| v.as_object());
        if let Some(area) = &area {
            let friendly_name = attributes
                .and_then(|a| a.get("friendly_name"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            let attr_area = attributes
                .and_then(|a| a.get("area"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if !friendly_name.contains(area) && !attr_area.contains(area) {
                continue;
            }
        }

        entities.push(json!({
            "entity_id": entity_id,
            "state": state.get("state").and_then(|v| v.as_str()).unwrap_or(""),
            "friendly_name": attributes
                .and_then(|a| a.get("friendly_name"))
                .and_then(|v| v.as_str())
                .unwrap_or(""),
        }));
    }

    json!({
        "count": entities.len(),
        "entities": entities,
    })
}

fn summarize_services(services: &[JsonValue], domain: Option<&str>) -> JsonValue {
    let mut domains = Vec::new();

    for service_domain in services {
        let Some(name) = service_domain.get("domain").and_then(|v| v.as_str()) else {
            continue;
        };
        if let Some(domain) = domain
            && name != domain
        {
            continue;
        }

        let mut summarized_services = JsonMap::new();
        if let Some(service_map) = service_domain.get("services").and_then(|v| v.as_object()) {
            for (service_name, service_info) in service_map {
                let mut entry = JsonMap::new();
                entry.insert(
                    "description".to_string(),
                    service_info
                        .get("description")
                        .cloned()
                        .unwrap_or_else(|| json!("")),
                );

                if let Some(fields) = service_info.get("fields").and_then(|v| v.as_object()) {
                    let mut field_map = JsonMap::new();
                    for (field_name, field_info) in fields {
                        if let Some(description) = field_info.get("description") {
                            field_map.insert(field_name.clone(), description.clone());
                        }
                    }
                    if !field_map.is_empty() {
                        entry.insert("fields".to_string(), JsonValue::Object(field_map));
                    }
                }

                summarized_services.insert(service_name.clone(), JsonValue::Object(entry));
            }
        }

        domains.push(json!({
            "domain": name,
            "services": summarized_services,
        }));
    }

    json!({
        "count": domains.len(),
        "domains": domains,
    })
}

fn service_payload(entity_id: Option<&str>, data: Option<&JsonValue>) -> Result<JsonValue> {
    let mut payload = JsonMap::new();

    match data {
        Some(JsonValue::Object(map)) => payload.extend(map.clone()),
        Some(JsonValue::String(raw)) if raw.trim().is_empty() => {}
        Some(JsonValue::String(raw)) => {
            let parsed: JsonValue = serde_json::from_str(raw).map_err(|e| {
                HakimiError::ToolSimple(format!("invalid JSON string in data parameter: {e}"))
            })?;
            let JsonValue::Object(map) = parsed else {
                return Err(HakimiError::ToolSimple(
                    "data JSON string must decode to an object".into(),
                ));
            };
            payload.extend(map);
        }
        Some(JsonValue::Null) | None => {}
        Some(_) => {
            return Err(HakimiError::ToolSimple(
                "data must be an object or a JSON object string".into(),
            ));
        }
    }

    if let Some(entity_id) = entity_id {
        payload.insert("entity_id".to_string(), json!(entity_id));
    }

    Ok(JsonValue::Object(payload))
}

fn parse_service_response(domain: &str, service: &str, response: JsonValue) -> JsonValue {
    let affected_entities: Vec<JsonValue> = response
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|state| {
            let entity_id = state.get("entity_id").and_then(|v| v.as_str())?;
            Some(json!({
                "entity_id": entity_id,
                "state": state.get("state").and_then(|v| v.as_str()).unwrap_or(""),
            }))
        })
        .collect();

    json!({
        "success": true,
        "service": format!("{domain}.{service}"),
        "affected_entities": affected_entities,
    })
}

/// List Home Assistant entities, optionally filtered by domain or area.
pub struct HaListEntitiesTool;

#[async_trait]
impl Tool for HaListEntitiesTool {
    fn name(&self) -> &str {
        "ha_list_entities"
    }

    fn toolset(&self) -> &str {
        TOOLSET
    }

    fn description(&self) -> &str {
        "List Home Assistant entities, optionally filtered by domain or area."
    }

    fn emoji(&self) -> &str {
        EMOJI
    }

    fn schema(&self) -> JsonValue {
        json!({
            "type": "object",
            "properties": {
                "domain": {
                    "type": "string",
                    "description": "Optional entity domain filter such as light, switch, climate, sensor, binary_sensor, cover, fan, or media_player."
                },
                "area": {
                    "type": "string",
                    "description": "Optional room or area filter matched against friendly_name and area attributes."
                }
            }
        })
    }

    fn check_available(&self) -> bool {
        homeassistant_available()
    }

    fn max_result_size(&self) -> Option<usize> {
        Some(MAX_RESULT_SIZE)
    }

    async fn execute(&self, args: &JsonValue, _ctx: &ToolContext) -> Result<String> {
        let domain = optional_string(args, "domain");
        if let Some(domain) = domain {
            validate_service_name("domain", domain)?;
        }
        let area = optional_string(args, "area");
        debug!(domain = ?domain, area = ?area, "Home Assistant list entities request");

        let states = ha_get("/api/states").await?;
        let states = states.as_array().ok_or_else(|| {
            HakimiError::ToolSimple("Home Assistant states response was not an array".into())
        })?;
        Ok(json!({"result": summarize_entities(states, domain, area)}).to_string())
    }
}

/// Get detailed state for one Home Assistant entity.
pub struct HaGetStateTool;

#[async_trait]
impl Tool for HaGetStateTool {
    fn name(&self) -> &str {
        "ha_get_state"
    }

    fn toolset(&self) -> &str {
        TOOLSET
    }

    fn description(&self) -> &str {
        "Get the detailed Home Assistant state and attributes for one entity."
    }

    fn emoji(&self) -> &str {
        EMOJI
    }

    fn schema(&self) -> JsonValue {
        json!({
            "type": "object",
            "properties": {
                "entity_id": {
                    "type": "string",
                    "description": "Entity ID to query, for example light.living_room or sensor.temperature."
                }
            },
            "required": ["entity_id"]
        })
    }

    fn check_available(&self) -> bool {
        homeassistant_available()
    }

    fn max_result_size(&self) -> Option<usize> {
        Some(MAX_RESULT_SIZE)
    }

    async fn execute(&self, args: &JsonValue, _ctx: &ToolContext) -> Result<String> {
        let entity_id = required_string(args, "entity_id")?;
        validate_entity_id(entity_id)?;
        debug!(entity_id = %entity_id, "Home Assistant get state request");

        let data = ha_get(&format!("/api/states/{entity_id}")).await?;
        Ok(json!({
            "result": {
                "entity_id": data.get("entity_id").cloned().unwrap_or_else(|| json!(entity_id)),
                "state": data.get("state").cloned().unwrap_or_else(|| json!("")),
                "attributes": data.get("attributes").cloned().unwrap_or_else(|| json!({})),
                "last_changed": data.get("last_changed").cloned().unwrap_or(JsonValue::Null),
                "last_updated": data.get("last_updated").cloned().unwrap_or(JsonValue::Null),
            }
        })
        .to_string())
    }
}

/// List Home Assistant services, optionally filtered by domain.
pub struct HaListServicesTool;

#[async_trait]
impl Tool for HaListServicesTool {
    fn name(&self) -> &str {
        "ha_list_services"
    }

    fn toolset(&self) -> &str {
        TOOLSET
    }

    fn description(&self) -> &str {
        "List Home Assistant services and compact field descriptions."
    }

    fn emoji(&self) -> &str {
        EMOJI
    }

    fn schema(&self) -> JsonValue {
        json!({
            "type": "object",
            "properties": {
                "domain": {
                    "type": "string",
                    "description": "Optional service domain filter, for example light, climate, or switch."
                }
            }
        })
    }

    fn check_available(&self) -> bool {
        homeassistant_available()
    }

    fn max_result_size(&self) -> Option<usize> {
        Some(MAX_RESULT_SIZE)
    }

    async fn execute(&self, args: &JsonValue, _ctx: &ToolContext) -> Result<String> {
        let domain = optional_string(args, "domain");
        if let Some(domain) = domain {
            validate_service_name("domain", domain)?;
        }
        debug!(domain = ?domain, "Home Assistant list services request");

        let services = ha_get("/api/services").await?;
        let services = services.as_array().ok_or_else(|| {
            HakimiError::ToolSimple("Home Assistant services response was not an array".into())
        })?;
        Ok(json!({"result": summarize_services(services, domain)}).to_string())
    }
}

/// Call a Home Assistant service to control a device or scene.
pub struct HaCallServiceTool;

#[async_trait]
impl Tool for HaCallServiceTool {
    fn name(&self) -> &str {
        "ha_call_service"
    }

    fn toolset(&self) -> &str {
        TOOLSET
    }

    fn description(&self) -> &str {
        "Call a Home Assistant service such as light.turn_on, switch.turn_off, or climate.set_temperature."
    }

    fn emoji(&self) -> &str {
        EMOJI
    }

    fn schema(&self) -> JsonValue {
        json!({
            "type": "object",
            "properties": {
                "domain": {
                    "type": "string",
                    "description": "Service domain, for example light, switch, climate, cover, media_player, fan, scene, or script."
                },
                "service": {
                    "type": "string",
                    "description": "Service name, for example turn_on, turn_off, toggle, set_temperature, or set_hvac_mode."
                },
                "entity_id": {
                    "type": "string",
                    "description": "Optional target entity ID, for example light.living_room."
                },
                "data": {
                    "description": "Optional additional service data as an object or JSON object string.",
                    "oneOf": [
                        {"type": "object"},
                        {"type": "string"}
                    ]
                }
            },
            "required": ["domain", "service"]
        })
    }

    fn check_available(&self) -> bool {
        homeassistant_available()
    }

    fn max_result_size(&self) -> Option<usize> {
        Some(MAX_RESULT_SIZE)
    }

    async fn execute(&self, args: &JsonValue, _ctx: &ToolContext) -> Result<String> {
        let domain = required_string(args, "domain")?;
        let service = required_string(args, "service")?;
        validate_service_name("domain", domain)?;
        validate_service_name("service", service)?;
        ensure_domain_allowed(domain)?;

        let entity_id = optional_string(args, "entity_id");
        if let Some(entity_id) = entity_id {
            validate_entity_id(entity_id)?;
        }

        let payload = service_payload(entity_id, args.get("data"))?;
        debug!(
            domain = %domain,
            service = %service,
            has_entity = entity_id.is_some(),
            "Home Assistant call service request"
        );

        let response = ha_post(&format!("/api/services/{domain}/{service}"), &payload).await?;
        Ok(json!({"result": parse_service_response(domain, service, response)}).to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_metadata() {
        let tools: Vec<Box<dyn Tool>> = vec![
            Box::new(HaListEntitiesTool),
            Box::new(HaGetStateTool),
            Box::new(HaListServicesTool),
            Box::new(HaCallServiceTool),
        ];

        let names: Vec<&str> = tools.iter().map(|tool| tool.name()).collect();
        assert_eq!(
            names,
            vec![
                "ha_list_entities",
                "ha_get_state",
                "ha_list_services",
                "ha_call_service",
            ]
        );
        assert!(tools.iter().all(|tool| tool.toolset() == TOOLSET));
    }

    #[test]
    fn test_entity_id_validation() {
        assert!(is_valid_entity_id("light.living_room"));
        assert!(is_valid_entity_id("sensor.temperature_1"));
        assert!(!is_valid_entity_id("Light.living_room"));
        assert!(!is_valid_entity_id("light/living_room"));
        assert!(!is_valid_entity_id("light."));
        assert!(!is_valid_entity_id("light.living-room"));
    }

    #[test]
    fn test_service_name_validation_blocks_path_traversal() {
        assert!(is_valid_service_name("turn_on"));
        assert!(is_valid_service_name("set_temperature2"));
        assert!(!is_valid_service_name("../states"));
        assert!(!is_valid_service_name("shell_command/../light"));
        assert!(!is_valid_service_name("TurnOn"));
    }

    #[test]
    fn test_blocked_domains() {
        assert!(ensure_domain_allowed("light").is_ok());
        assert!(ensure_domain_allowed("shell_command").is_err());
        assert!(ensure_domain_allowed("rest_command").is_err());
    }

    #[test]
    fn test_service_payload_object_and_entity_precedence() {
        let data = json!({"entity_id": "light.old", "brightness": 128});
        let payload = service_payload(Some("light.living_room"), Some(&data)).unwrap();
        assert_eq!(payload["entity_id"], "light.living_room");
        assert_eq!(payload["brightness"], 128);
    }

    #[test]
    fn test_service_payload_json_string() {
        let data = json!(r#"{"temperature":22,"hvac_mode":"heat"}"#);
        let payload = service_payload(Some("climate.hall"), Some(&data)).unwrap();
        assert_eq!(payload["entity_id"], "climate.hall");
        assert_eq!(payload["temperature"], 22);
        assert_eq!(payload["hvac_mode"], "heat");
    }

    #[test]
    fn test_service_payload_rejects_non_object_string() {
        let data = json!("[1,2,3]");
        assert!(service_payload(None, Some(&data)).is_err());
    }

    #[test]
    fn test_summarize_entities_filters_domain_and_area() {
        let states = vec![
            json!({
                "entity_id": "light.kitchen",
                "state": "on",
                "attributes": {"friendly_name": "Kitchen Lights", "area": "Kitchen"}
            }),
            json!({
                "entity_id": "switch.kitchen_fan",
                "state": "off",
                "attributes": {"friendly_name": "Kitchen Fan"}
            }),
            json!({
                "entity_id": "light.bedroom",
                "state": "off",
                "attributes": {"friendly_name": "Bedroom Lamp"}
            }),
        ];

        let summary = summarize_entities(&states, Some("light"), Some("kitchen"));
        assert_eq!(summary["count"], 1);
        assert_eq!(summary["entities"][0]["entity_id"], "light.kitchen");
    }

    #[test]
    fn test_summarize_services_compacts_fields() {
        let services = vec![json!({
            "domain": "light",
            "services": {
                "turn_on": {
                    "description": "Turn on one or more lights.",
                    "fields": {
                        "brightness": {"description": "Brightness from 0 to 255."}
                    }
                }
            }
        })];

        let summary = summarize_services(&services, Some("light"));
        assert_eq!(summary["count"], 1);
        assert_eq!(
            summary["domains"][0]["services"]["turn_on"]["fields"]["brightness"],
            "Brightness from 0 to 255."
        );
    }

    #[test]
    fn test_parse_service_response() {
        let parsed = parse_service_response(
            "light",
            "turn_on",
            json!([
                {"entity_id": "light.kitchen", "state": "on"},
                {"entity_id": "switch.ignored", "state": "off"}
            ]),
        );
        assert_eq!(parsed["success"], true);
        assert_eq!(parsed["service"], "light.turn_on");
        assert_eq!(parsed["affected_entities"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn test_schemas_expose_required_fields() {
        let get_state = HaGetStateTool.schema();
        assert_eq!(get_state["required"][0], "entity_id");

        let call_service = HaCallServiceTool.schema();
        assert_eq!(call_service["required"][0], "domain");
        assert_eq!(call_service["required"][1], "service");
    }
}
