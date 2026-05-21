use async_trait::async_trait;
use hakimi_common::{HakimiError, Result, ToolContext};
use regex::Regex;
use serde_json::{json, Value as JsonValue};
use tokio::fs;
use tokio::process::Command;
use tracing::debug;

use crate::Tool;

/// Built-in tool that searches for patterns in files using ripgrep or grep.
pub struct SearchFilesTool;

#[async_trait]
impl Tool for SearchFilesTool {
    fn name(&self) -> &str {
        "search_files"
    }

    fn toolset(&self) -> &str {
        "file"
    }

    fn description(&self) -> &str {
        "Search for a regex pattern in files. Uses ripgrep (rg) if available, falls back to grep. Supports content search, file search by glob, and match counting."
    }

    fn emoji(&self) -> &str {
        "\u{1f50d}"
    }

    fn schema(&self) -> JsonValue {
        json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Regex pattern to search for."
                },
                "path": {
                    "type": "string",
                    "description": "Directory or file to search in. Defaults to current workdir."
                },
                "file_glob": {
                    "type": "string",
                    "description": "Filter files by glob pattern (e.g. '*.rs', '*.py')."
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of results to return. Defaults to 50.",
                    "minimum": 1,
                    "maximum": 500
                },
                "offset": {
                    "type": "integer",
                    "description": "Skip first N results for pagination. Defaults to 0.",
                    "minimum": 0
                },
                "context": {
                    "type": "integer",
                    "description": "Number of context lines before and after each match. Defaults to 0.",
                    "minimum": 0,
                    "maximum": 20
                },
                "target": {
                    "type": "string",
                    "description": "'content' searches inside file contents, 'files' searches for files by name. Defaults to 'content'.",
                    "enum": ["content", "files"]
                },
                "output_mode": {
                    "type": "string",
                    "description": "Output format: 'content' shows matching lines with line numbers, 'files_only' lists file paths, 'count' shows match counts per file. Defaults to 'content'.",
                    "enum": ["content", "files_only", "count"]
                }
            },
            "required": ["pattern"]
        })
    }

    fn max_result_size(&self) -> Option<usize> {
        Some(256 * 1024)
    }

    async fn execute(&self, args: &JsonValue, ctx: &ToolContext) -> Result<String> {
        let pattern = args
            .get("pattern")
            .and_then(|v| v.as_str())
            .ok_or_else(|| HakimiError::Tool("missing required parameter: pattern".into()))?;

        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or(&ctx.workdir);

        let file_glob = args.get("file_glob").and_then(|v| v.as_str());

        let limit = args
            .get("limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(50)
            .min(500) as usize;

        let offset = args
            .get("offset")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as usize;

        let context = args
            .get("context")
            .and_then(|v| v.as_u64())
            .unwrap_or(0)
            .min(20) as usize;

        let target = args
            .get("target")
            .and_then(|v| v.as_str())
            .unwrap_or("content");

        let output_mode = args
            .get("output_mode")
            .and_then(|v| v.as_str())
            .unwrap_or("content");

        // Resolve path
        let full_path = if path.starts_with('/') {
            std::path::PathBuf::from(path)
        } else {
            std::path::PathBuf::from(&ctx.workdir).join(path)
        };

        debug!(
            pattern = %pattern,
            path = %full_path.display(),
            target = %target,
            output_mode = %output_mode,
            "searching files"
        );

        match target {
            "files" => search_files_by_name(&full_path, pattern, file_glob, limit, offset).await,
            _ => search_file_contents(&full_path, pattern, file_glob, limit, offset, context, output_mode).await,
        }
    }
}

/// Search file contents using ripgrep or grep.
async fn search_file_contents(
    path: &std::path::Path,
    pattern: &str,
    file_glob: Option<&str>,
    limit: usize,
    offset: usize,
    context: usize,
    output_mode: &str,
) -> Result<String> {
    // Try ripgrep first, fall back to grep
    let rg_available = Command::new("rg")
        .arg("--version")
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false);

    let result = if rg_available {
        run_ripgrep(path, pattern, file_glob, limit, offset, context, output_mode).await?
    } else {
        run_grep(path, pattern, file_glob, limit, offset, context, output_mode).await?
    };

    Ok(result)
}

async fn run_ripgrep(
    path: &std::path::Path,
    pattern: &str,
    file_glob: Option<&str>,
    limit: usize,
    offset: usize,
    context: usize,
    output_mode: &str,
) -> Result<String> {
    let mut cmd = Command::new("rg");

    // Add context lines
    if context > 0 {
        cmd.arg("-C").arg(context.to_string());
    }

    // Output mode flags
    match output_mode {
        "files_only" => {
            cmd.arg("-l");
        }
        "count" => {
            cmd.arg("-c");
        }
        _ => {
            cmd.arg("-n"); // line numbers
        }
    }

    // File glob filter
    if let Some(glob) = file_glob {
        cmd.arg("-g").arg(glob);
    }

    // Limit results
    cmd.arg("-m").arg((limit + offset).to_string());

    // Pattern and path
    cmd.arg(pattern).arg(path);

    let output = cmd.output().await.map_err(|e| {
        HakimiError::Tool(format!("failed to run ripgrep: {e}"))
    })?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().collect();

    // Apply offset
    let result_lines: Vec<&str> = lines
        .into_iter()
        .skip(offset)
        .take(limit)
        .collect();

    if result_lines.is_empty() {
        return Ok("No matches found.".to_string());
    }

    Ok(result_lines.join("\n"))
}

async fn run_grep(
    path: &std::path::Path,
    pattern: &str,
    file_glob: Option<&str>,
    limit: usize,
    offset: usize,
    context: usize,
    output_mode: &str,
) -> Result<String> {
    let mut cmd = Command::new("grep");

    cmd.arg("-r"); // recursive

    if context > 0 {
        cmd.arg("-C").arg(context.to_string());
    }

    match output_mode {
        "files_only" => {
            cmd.arg("-l");
        }
        "count" => {
            cmd.arg("-c");
        }
        _ => {
            cmd.arg("-n"); // line numbers
        }
    }

    // File glob filter (grep uses --include)
    if let Some(glob) = file_glob {
        cmd.arg("--include").arg(glob);
    }

    cmd.arg("-E"); // extended regex
    cmd.arg(pattern).arg(path);

    let output = cmd.output().await.map_err(|e| {
        HakimiError::Tool(format!("failed to run grep: {e}"))
    })?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().collect();

    let result_lines: Vec<&str> = lines
        .into_iter()
        .skip(offset)
        .take(limit)
        .collect();

    if result_lines.is_empty() {
        return Ok("No matches found.".to_string());
    }

    Ok(result_lines.join("\n"))
}

/// Search for files by name using pattern matching.
async fn search_files_by_name(
    path: &std::path::Path,
    pattern: &str,
    file_glob: Option<&str>,
    limit: usize,
    offset: usize,
) -> Result<String> {
    let re = Regex::new(pattern).map_err(|e| {
        HakimiError::Tool(format!("invalid regex pattern: {e}"))
    })?;

    let mut results = Vec::new();
    collect_files(path, &re, file_glob, &mut results, limit + offset).await?;

    let result_files: Vec<&std::path::PathBuf> = results
        .iter()
        .skip(offset)
        .take(limit)
        .collect();

    if result_files.is_empty() {
        return Ok("No files found matching pattern.".to_string());
    }

    let output: Vec<String> = result_files
        .iter()
        .map(|p| p.display().to_string())
        .collect();

    Ok(output.join("\n"))
}

/// Recursively collect files matching the regex pattern.
fn collect_files<'a>(
    dir: &'a std::path::Path,
    re: &'a Regex,
    file_glob: Option<&'a str>,
    results: &'a mut Vec<std::path::PathBuf>,
    max: usize,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + 'a>> {
    Box::pin(async move {
        if results.len() >= max {
            return Ok(());
        }

        let mut entries = fs::read_dir(dir).await.map_err(|e| {
            HakimiError::Tool(format!("failed to read directory '{}': {e}", dir.display()))
        })?;

        while let Some(entry) = entries.next_entry().await.map_err(|e| {
            HakimiError::Tool(format!("failed to read directory entry: {e}"))
        })? {
            if results.len() >= max {
                break;
            }

            let path = entry.path();
            let file_name = entry.file_name();
            let file_name_str = file_name.to_string_lossy();

            // Check glob filter
            if let Some(glob) = file_glob {
                if !matches_glob(&file_name_str, glob) {
                    if path.is_dir() {
                        collect_files(&path, re, file_glob, results, max).await?;
                    }
                    continue;
                }
            }

            if path.is_dir() {
                collect_files(&path, re, file_glob, results, max).await?;
            } else if re.is_match(&file_name_str) {
                results.push(path);
            }
        }

        Ok(())
    })
}

/// Simple glob matching supporting * and ? wildcards.
fn matches_glob(name: &str, glob: &str) -> bool {
    let regex_pattern: String = {
        let mut pat = String::from("^");
        for c in glob.chars() {
            match c {
                '*' => pat.push_str(".*"),
                '?' => pat.push('.'),
                '.' => pat.push_str("\\."),
                other => pat.push(other),
            }
        }
        pat.push('$');
        pat
    };

    Regex::new(&regex_pattern)
        .map(|re| re.is_match(name))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hakimi_common::ToolContext;

    fn test_ctx(workdir: &str) -> ToolContext {
ToolContext {
            session_id: "test".to_string(),
            user_id: None,
            task_id: None,
            workdir: workdir.to_string(),
            model: None,
            delegate_executor: None,
        }
    }

    #[test]
    fn test_schema_is_valid() {
        let tool = SearchFilesTool;
        let schema = tool.schema();
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"].is_object());
        assert!(schema["properties"]["pattern"].is_object());
        assert!(schema["properties"]["path"].is_object());
        assert!(schema["properties"]["target"].is_object());
        let required = schema["required"].as_array().unwrap();
        assert!(required.iter().any(|v| v == "pattern"));
    }

    #[test]
    fn test_tool_properties() {
        let tool = SearchFilesTool;
        assert_eq!(tool.name(), "search_files");
        assert_eq!(tool.toolset(), "file");
        assert!(tool.check_available());
        assert_eq!(tool.max_result_size(), Some(256 * 1024));
    }

    #[test]
    fn test_matches_glob() {
        assert!(matches_glob("test.rs", "*.rs"));
        assert!(!matches_glob("test.py", "*.rs"));
        assert!(matches_glob("test.rs", "test.?s"));
        assert!(!matches_glob("test.rs", "prod.*"));
        assert!(matches_glob("hello.txt", "hello.txt"));
    }

    #[tokio::test]
    async fn test_search_content_basic() {
        // Create temp dir with some files
        let dir = std::env::temp_dir().join("hakimi_test_search_content");
        let _ = fs::create_dir_all(&dir).await;
        fs::write(dir.join("a.txt"), "hello world\nfoo bar\nhello again")
            .await
            .unwrap();
        fs::write(dir.join("b.txt"), "nothing here")
            .await
            .unwrap();

        let ctx = test_ctx(&dir.to_string_lossy());
        let args = json!({
            "pattern": "hello",
            "path": dir.to_string_lossy()
        });

        let result = SearchFilesTool.execute(&args, &ctx).await.unwrap();
        assert!(result.contains("hello"));
        assert!(result.contains("a.txt"));

        let _ = fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn test_search_content_no_matches() {
        let dir = std::env::temp_dir().join("hakimi_test_search_nomatch");
        let _ = fs::create_dir_all(&dir).await;
        fs::write(dir.join("a.txt"), "hello world")
            .await
            .unwrap();

        let ctx = test_ctx(&dir.to_string_lossy());
        let args = json!({
            "pattern": "zzzzz_not_found",
            "path": dir.to_string_lossy()
        });

        let result = SearchFilesTool.execute(&args, &ctx).await.unwrap();
        assert!(result.contains("No matches"));

        let _ = fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn test_search_files_by_name() {
        let dir = std::env::temp_dir().join("hakimi_test_search_files");
        let _ = fs::create_dir_all(&dir).await;
        fs::write(dir.join("readme.md"), "# Hello")
            .await
            .unwrap();
        fs::write(dir.join("code.rs"), "fn main() {}")
            .await
            .unwrap();
        fs::write(dir.join("test.rs"), "#[test] fn t() {}")
            .await
            .unwrap();

        let ctx = test_ctx(&dir.to_string_lossy());
        let args = json!({
            "pattern": ".*\\.rs$",
            "path": dir.to_string_lossy(),
            "target": "files"
        });

        let result = SearchFilesTool.execute(&args, &ctx).await.unwrap();
        assert!(result.contains("code.rs"));
        assert!(result.contains("test.rs"));
        assert!(!result.contains("readme.md"));

        let _ = fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn test_search_with_glob_filter() {
        let dir = std::env::temp_dir().join("hakimi_test_search_glob");
        let _ = fs::create_dir_all(&dir).await;
        fs::write(dir.join("a.rs"), "hello")
            .await
            .unwrap();
        fs::write(dir.join("a.py"), "hello")
            .await
            .unwrap();

        let ctx = test_ctx(&dir.to_string_lossy());
        let args = json!({
            "pattern": "hello",
            "path": dir.to_string_lossy(),
            "file_glob": "*.rs"
        });

        let result = SearchFilesTool.execute(&args, &ctx).await.unwrap();
        // Should only match .rs files
        assert!(result.contains("a.rs") || result.contains("No matches"));

        let _ = fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn test_search_missing_pattern_error() {
        let ctx = test_ctx("/tmp");
        let args = json!({});
        let err = SearchFilesTool.execute(&args, &ctx).await.unwrap_err();
        assert!(format!("{err}").contains("pattern"));
    }
}
