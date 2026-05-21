//! Built-in catalog of popular MCP servers for one-click enable.

use serde::{Deserialize, Serialize};

/// An environment variable required by an MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvVar {
    pub name: String,
    pub description: String,
    pub required: bool,
    pub example: Option<String>,
}

/// A catalog entry for a well-known MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerEntry {
    pub name: String,
    pub description: String,
    pub category: String,
    pub command: String,
    pub args: Vec<String>,
    pub env_vars: Vec<EnvVar>,
    pub install_hint: String,
    pub popular: bool,
}

/// Return the default catalog of popular MCP servers.
pub fn default_catalog() -> Vec<McpServerEntry> {
    vec![
        McpServerEntry {
            name: "filesystem".into(),
            description: "Read, write, and manage files and directories".into(),
            category: "filesystem".into(),
            command: "npx".into(),
            args: vec![
                "-y".into(),
                "@modelcontextprotocol/server-filesystem".into(),
                "/".into(),
            ],
            env_vars: vec![],
            install_hint: "npm install -g @modelcontextprotocol/server-filesystem".into(),
            popular: true,
        },
        McpServerEntry {
            name: "github".into(),
            description: "Access GitHub repos, issues, pull requests, and more".into(),
            category: "scm".into(),
            command: "npx".into(),
            args: vec!["-y".into(), "@modelcontextprotocol/server-github".into()],
            env_vars: vec![EnvVar {
                name: "GITHUB_TOKEN".into(),
                description: "GitHub personal access token".into(),
                required: true,
                example: Some("ghp_xxxxxxxxxxxx".into()),
            }],
            install_hint: "npm install -g @modelcontextprotocol/server-github".into(),
            popular: true,
        },
        McpServerEntry {
            name: "brave-search".into(),
            description: "Web search using the Brave Search API".into(),
            category: "search".into(),
            command: "npx".into(),
            args: vec![
                "-y".into(),
                "@modelcontextprotocol/server-brave-search".into(),
            ],
            env_vars: vec![EnvVar {
                name: "BRAVE_API_KEY".into(),
                description: "Brave Search API key".into(),
                required: true,
                example: Some("BSAxxxxxxxxxxxxxxxxxxxxxxx".into()),
            }],
            install_hint: "npm install -g @modelcontextprotocol/server-brave-search".into(),
            popular: true,
        },
        McpServerEntry {
            name: "postgres".into(),
            description: "Query and manage PostgreSQL databases".into(),
            category: "database".into(),
            command: "npx".into(),
            args: vec!["-y".into(), "@modelcontextprotocol/server-postgres".into()],
            env_vars: vec![EnvVar {
                name: "DATABASE_URL".into(),
                description: "PostgreSQL connection string".into(),
                required: true,
                example: Some("postgresql://user:pass@localhost:5432/mydb".into()),
            }],
            install_hint: "npm install -g @modelcontextprotocol/server-postgres".into(),
            popular: true,
        },
        McpServerEntry {
            name: "puppeteer".into(),
            description: "Browser automation with Puppeteer (screenshots, navigation, etc.)".into(),
            category: "devtools".into(),
            command: "npx".into(),
            args: vec!["-y".into(), "@modelcontextprotocol/server-puppeteer".into()],
            env_vars: vec![],
            install_hint: "npm install -g @modelcontextprotocol/server-puppeteer".into(),
            popular: true,
        },
        McpServerEntry {
            name: "memory".into(),
            description: "Persistent knowledge graph memory for the agent".into(),
            category: "devtools".into(),
            command: "npx".into(),
            args: vec!["-y".into(), "@modelcontextprotocol/server-memory".into()],
            env_vars: vec![],
            install_hint: "npm install -g @modelcontextprotocol/server-memory".into(),
            popular: false,
        },
        McpServerEntry {
            name: "fetch".into(),
            description: "Make HTTP requests and fetch web content".into(),
            category: "devtools".into(),
            command: "npx".into(),
            args: vec!["-y".into(), "@modelcontextprotocol/server-fetch".into()],
            env_vars: vec![],
            install_hint: "npm install -g @modelcontextprotocol/server-fetch".into(),
            popular: false,
        },
        McpServerEntry {
            name: "sqlite".into(),
            description: "Query and manage SQLite databases".into(),
            category: "database".into(),
            command: "uvx".into(),
            args: vec![
                "mcp-server-sqlite".into(),
                "--db-path".into(),
                "~/data.db".into(),
            ],
            env_vars: vec![],
            install_hint: "pip install mcp-server-sqlite  (or: uv tool install mcp-server-sqlite)".into(),
            popular: false,
        },
        McpServerEntry {
            name: "sequential-thinking".into(),
            description: "Step-by-step sequential thinking and reasoning".into(),
            category: "devtools".into(),
            command: "npx".into(),
            args: vec!["-y".into(), "@anthropic/sequential-thinking-mcp".into()],
            env_vars: vec![],
            install_hint: "npm install -g @anthropic/sequential-thinking-mcp".into(),
            popular: false,
        },
    ]
}

/// Search the catalog by name, description, or category (case-insensitive).
pub fn search(query: &str) -> Vec<McpServerEntry> {
    let q = query.to_lowercase();
    default_catalog()
        .into_iter()
        .filter(|e| {
            e.name.to_lowercase().contains(&q)
                || e.description.to_lowercase().contains(&q)
                || e.category.to_lowercase().contains(&q)
        })
        .collect()
}

/// Get a catalog entry by exact name.
pub fn get(name: &str) -> Option<McpServerEntry> {
    default_catalog().into_iter().find(|e| e.name == name)
}

/// List all distinct categories in the catalog.
pub fn categories() -> Vec<String> {
    let mut cats: Vec<String> = default_catalog()
        .iter()
        .map(|e| e.category.clone())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    cats.sort();
    cats
}

/// Filter catalog entries by category.
pub fn by_category(cat: &str) -> Vec<McpServerEntry> {
    default_catalog()
        .into_iter()
        .filter(|e| e.category == cat)
        .collect()
}

/// Generate a YAML config snippet for the given catalog entries,
/// suitable for pasting into `~/.hakimi/config.yaml`.
pub fn to_config_yaml(entries: &[McpServerEntry]) -> String {
    let mut out = String::from("mcp_servers:\n");
    for entry in entries {
        out.push_str(&format!("  {}:\n", entry.name));
        out.push_str(&format!("    command: \"{}\"\n", entry.command));
        out.push_str("    args: [");
        for (i, arg) in entry.args.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            out.push_str(&format!("\"{}\"", arg));
        }
        out.push_str("]\n");
        if !entry.env_vars.is_empty() {
            out.push_str("    env:\n");
            for ev in &entry.env_vars {
                let val = ev.example.as_deref().unwrap_or("...");
                out.push_str(&format!("      {}: \"{}\"\n", ev.name, val));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_catalog_not_empty() {
        let catalog = default_catalog();
        assert!(!catalog.is_empty(), "catalog should have entries");
        assert!(catalog.len() >= 9, "expected at least 9 catalog entries");
    }

    #[test]
    fn test_catalog_search() {
        let results = search("github");
        assert!(!results.is_empty());
        assert!(results.iter().any(|e| e.name == "github"));

        let results2 = search("database");
        assert!(results2.iter().any(|e| e.category == "database"));

        let results3 = search("nonexistent_xyz_12345");
        assert!(results3.is_empty());
    }

    #[test]
    fn test_catalog_by_category() {
        let db_entries = by_category("database");
        assert!(!db_entries.is_empty());
        assert!(db_entries.iter().all(|e| e.category == "database"));

        let fs_entries = by_category("filesystem");
        assert!(!fs_entries.is_empty());
        assert!(fs_entries.iter().all(|e| e.category == "filesystem"));
    }

    #[test]
    fn test_catalog_get_existing() {
        let entry = get("github");
        assert!(entry.is_some());
        let e = entry.unwrap();
        assert_eq!(e.name, "github");
        assert_eq!(e.category, "scm");
        assert!(!e.env_vars.is_empty());
    }

    #[test]
    fn test_catalog_get_nonexistent() {
        assert!(get("does-not-exist").is_none());
    }

    #[test]
    fn test_to_config_yaml_generation() {
        let entries = vec![McpServerEntry {
            name: "test-server".into(),
            description: "A test server".into(),
            category: "devtools".into(),
            command: "npx".into(),
            args: vec!["-y".into(), "@test/server".into()],
            env_vars: vec![EnvVar {
                name: "API_KEY".into(),
                description: "An API key".into(),
                required: true,
                example: Some("sk-test123".into()),
            }],
            install_hint: "npm install".into(),
            popular: false,
        }];

        let yaml = to_config_yaml(&entries);
        assert!(yaml.contains("mcp_servers:"));
        assert!(yaml.contains("test-server:"));
        assert!(yaml.contains("command: \"npx\""));
        assert!(yaml.contains("API_KEY"));
        assert!(yaml.contains("sk-test123"));
    }

    #[test]
    fn test_env_var_required() {
        let github = get("github").unwrap();
        assert!(github.env_vars.iter().any(|ev| ev.required));

        let fs = get("filesystem").unwrap();
        assert!(fs.env_vars.is_empty());
    }

    #[test]
    fn test_popular_entries_first() {
        let catalog = default_catalog();
        let popular: Vec<_> = catalog.iter().filter(|e| e.popular).collect();
        assert!(!popular.is_empty(), "should have popular entries");
        // Verify popular entries exist
        assert!(popular.iter().any(|e| e.name == "filesystem"));
        assert!(popular.iter().any(|e| e.name == "github"));
        assert!(popular.iter().any(|e| e.name == "brave-search"));
    }

    #[test]
    fn test_categories_non_empty() {
        let cats = categories();
        assert!(!cats.is_empty());
        assert!(cats.contains(&"database".to_string()));
        assert!(cats.contains(&"filesystem".to_string()));
        assert!(cats.contains(&"search".to_string()));
        assert!(cats.contains(&"scm".to_string()));
        assert!(cats.contains(&"devtools".to_string()));
    }

    #[test]
    fn test_template_files_exist() {
        // Templates live at the workspace root /templates/
        let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let templates_dir = manifest_dir
            .parent()  // crates/
            .and_then(|p| p.parent())  // workspace root
            .map(|p| p.join("templates"))
            .expect("could not resolve templates dir");

        assert!(
            templates_dir.join("plugin-http-api.yaml").exists(),
        );
        assert!(
            templates_dir.join("plugin-weather.yaml").exists(),
        );
        assert!(
            templates_dir.join("mcp-server-custom.yaml").exists(),
        );
        assert!(
            templates_dir.join("gateway-webhook.yaml").exists(),
        );
    }
}
