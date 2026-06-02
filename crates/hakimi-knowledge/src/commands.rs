use crate::{EdgeType, KnowledgeStore, NodeType};
use std::path::{Path, PathBuf};

pub fn knowledge_path(home: &Path) -> PathBuf {
    home.join("knowledge.json")
}

pub fn knowledge_response_from_raw(raw: Option<&str>, home: &Path) -> String {
    let args = raw
        .unwrap_or_default()
        .split_whitespace()
        .map(String::from)
        .collect::<Vec<_>>();
    knowledge_response(&args, home)
}

pub fn knowledge_response(args: &[String], home: &Path) -> String {
    let action = args
        .first()
        .map(|value| value.to_ascii_lowercase())
        .unwrap_or_else(|| "stats".to_string());
    let path = knowledge_path(home);

    match action.as_str() {
        "stats" | "status" => with_store(&path, |store| {
            let stats = store.graph().stats();
            format!(
                "Knowledge graph stats:\n- Path: `{}`\n- Nodes: {}\n- Edges: {}\n- Connected components: {}\n- Avg degree: {:.2}",
                path.display(),
                stats.node_count,
                stats.edge_count,
                stats.connected_components,
                stats.avg_degree
            )
        }),
        "list" | "ls" => with_store(&path, |store| {
            let mut nodes = store
                .graph()
                .all_nodes()
                .into_iter()
                .map(format_node)
                .collect::<Vec<_>>();
            nodes.sort();
            if nodes.is_empty() {
                "Knowledge graph is empty.".to_string()
            } else {
                format!("Knowledge graph entities:\n{}", nodes.join("\n"))
            }
        }),
        "search" | "find" => {
            let query = args[1..].join(" ");
            if query.trim().is_empty() {
                return "Usage: `hakimi knowledge search <query>`".to_string();
            }
            with_store(&path, |store| {
                let mut nodes = store
                    .graph()
                    .search(&query)
                    .into_iter()
                    .map(format_node)
                    .collect::<Vec<_>>();
                nodes.sort();
                if nodes.is_empty() {
                    format!("No matching knowledge entities found for `{query}`.")
                } else {
                    format!("Knowledge search `{query}`:\n{}", nodes.join("\n"))
                }
            })
        }
        "context" | "ctx" | "show" => {
            let Some(key) = args.get(1) else {
                return "Usage: `hakimi knowledge context <key> [depth]`".to_string();
            };
            let depth = args
                .get(2)
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(2);
            with_store(&path, |store| {
                if !store.graph().has_node(key) {
                    return format!("Knowledge entity `{key}` was not found.");
                }
                let mut neighbors = store
                    .graph()
                    .query_neighbors(key, depth)
                    .into_iter()
                    .map(format_node)
                    .collect::<Vec<_>>();
                neighbors.sort();
                if neighbors.is_empty() {
                    format!("No neighbors found for `{key}`.")
                } else {
                    format!("Knowledge context for `{key}`:\n{}", neighbors.join("\n"))
                }
            })
        }
        "add" => {
            let Some(kind) = args.get(1) else {
                return "Usage: `hakimi knowledge add <kind> <key>`".to_string();
            };
            let Some(key) = args.get(2) else {
                return "Usage: `hakimi knowledge add <kind> <key>`".to_string();
            };
            if key.trim().is_empty() {
                return "Knowledge entity key must not be empty.".to_string();
            }
            let Some(node) = NodeType::from_kind_and_key(kind, key.to_string()) else {
                return format!(
                    "Unknown knowledge kind `{kind}`. Use entity, person, location, skill, tool, event, note, concept, fact, or preference."
                );
            };
            with_store_mut(&path, |store| {
                store.add_node(node)?;
                Ok(format!("Added {} `{key}` to knowledge graph.", kind.trim()))
            })
        }
        "relate" | "relation" | "link" => {
            let Some(from) = args.get(1) else {
                return "Usage: `hakimi knowledge relate <from> <relation> <to>`".to_string();
            };
            let Some(relation) = args.get(2) else {
                return "Usage: `hakimi knowledge relate <from> <relation> <to>`".to_string();
            };
            let Some(to) = args.get(3) else {
                return "Usage: `hakimi knowledge relate <from> <relation> <to>`".to_string();
            };
            let edge = EdgeType::from_relation(relation);
            with_store_mut(&path, |store| {
                store.add_edge(from, to, edge)?;
                Ok(format!(
                    "Added knowledge relation `{}` from `{from}` to `{to}`.",
                    relation.trim()
                ))
            })
        }
        "path" => format!("Knowledge graph path: `{}`", path.display()),
        "help" | "-h" | "--help" => knowledge_help_response(),
        other => format!(
            "Unknown knowledge command `{other}`.\n\n{}",
            knowledge_help_response()
        ),
    }
}

fn with_store(path: &Path, render: impl FnOnce(&KnowledgeStore) -> String) -> String {
    let mut store = KnowledgeStore::new(path.to_path_buf());
    if let Err(err) = store.load() {
        return format!("Failed to load knowledge graph `{}`: {err}", path.display());
    }
    render(&store)
}

fn with_store_mut(
    path: &Path,
    mutate: impl FnOnce(&mut KnowledgeStore) -> anyhow::Result<String>,
) -> String {
    let mut store = KnowledgeStore::new(path.to_path_buf());
    if let Err(err) = store.load() {
        return format!("Failed to load knowledge graph `{}`: {err}", path.display());
    }
    match mutate(&mut store) {
        Ok(message) => message,
        Err(err) => format!("Failed to update knowledge graph: {err}"),
    }
}

fn format_node(node: &NodeType) -> String {
    format!("- [{}] {}", node.kind(), node.key())
}

fn knowledge_help_response() -> String {
    [
        "Usage: `hakimi knowledge <command>`",
        "",
        "Commands:",
        "- `stats` - show graph counts and storage path",
        "- `list` - list stored entities",
        "- `search <query>` - search entity keys",
        "- `context <key> [depth]` - show nearby graph entities",
        "- `add <kind> <key>` - add or update an entity",
        "- `relate <from> <relation> <to>` - add a directed relation",
        "- `path` - show the knowledge JSON file path",
    ]
    .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_adds_lists_searches_and_contexts_graph() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path();

        let add_person = knowledge_response(&["add".into(), "person".into(), "alice".into()], home);
        assert!(add_person.contains("Added person `alice`"));

        let add_tool = knowledge_response(&["add".into(), "tool".into(), "read_file".into()], home);
        assert!(add_tool.contains("Added tool `read_file`"));

        let relate = knowledge_response(
            &[
                "relate".into(),
                "alice".into(),
                "used_with".into(),
                "read_file".into(),
            ],
            home,
        );
        assert!(relate.contains("used_with"));

        let listed = knowledge_response(&["list".into()], home);
        assert!(listed.contains("[person] alice"));
        assert!(listed.contains("[tool] read_file"));

        let searched = knowledge_response(&["search".into(), "read".into()], home);
        assert!(searched.contains("[tool] read_file"));

        let context = knowledge_response(&["context".into(), "alice".into()], home);
        assert!(context.contains("[tool] read_file"));

        let stats = knowledge_response(&["stats".into()], home);
        assert!(stats.contains("- Nodes: 2"));
        assert!(stats.contains("- Edges: 1"));
        assert!(knowledge_path(home).exists());
    }

    #[test]
    fn response_rejects_invalid_add_kind() {
        let temp = tempfile::tempdir().unwrap();
        let response = knowledge_response(
            &["add".into(), "unknown".into(), "alice".into()],
            temp.path(),
        );
        assert!(response.contains("Unknown knowledge kind"));
    }

    #[test]
    fn response_reports_path() {
        let temp = tempfile::tempdir().unwrap();
        let response = knowledge_response(&["path".into()], temp.path());
        assert!(response.contains("Knowledge graph path"));
        assert!(response.contains("knowledge.json"));
    }

    #[test]
    fn response_reports_missing_relation_args() {
        let temp = tempfile::tempdir().unwrap();
        let response = knowledge_response(&["relate".into(), "alice".into()], temp.path());
        assert_eq!(
            response,
            "Usage: `hakimi knowledge relate <from> <relation> <to>`"
        );
    }
}
