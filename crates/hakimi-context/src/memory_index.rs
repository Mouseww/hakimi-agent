//! Pure-Rust BM25 index over memory entries.
//!
//! `FileMemoryProvider::prefetch` used to scan memory line by line and return
//! the first lines whose text contained a query word. That has two problems:
//!
//! 1. It returns *file order*, not relevance order — a passing mention near the
//!    top of `memory.md` outranks the entry that actually answers the question.
//! 2. Multi-term queries get no partial credit and no length normalisation, and
//!    CJK text (no whitespace) cannot be split into keywords at all.
//!
//! This module replaces that with a small, dependency-free BM25 ranking that
//! works for both Latin text and CJK.

use std::collections::HashMap;

/// BM25 term-frequency saturation parameter.
const K1: f64 = 1.2;
/// BM25 length-normalisation parameter.
const B: f64 = 0.75;

/// One scored memory line.
#[derive(Debug, Clone, PartialEq)]
pub struct MemoryHit {
    /// 0-based line number inside the indexed document.
    pub line_no: usize,
    /// The raw memory line.
    pub text: String,
    /// BM25 relevance score.
    pub score: f64,
}

#[derive(Debug, Clone)]
struct Doc {
    line_no: usize,
    text: String,
    len: usize,
    tf: HashMap<String, u32>,
}

/// A BM25 index over the lines of a memory document.
#[derive(Debug, Clone, Default)]
pub struct MemoryIndex {
    docs: Vec<Doc>,
    df: HashMap<String, u32>,
    avg_len: f64,
}

/// Whether a character belongs to a CJK block that has no word delimiters.
fn is_cjk(ch: char) -> bool {
    matches!(ch as u32,
        0x3040..=0x30FF      // kana
        | 0x3400..=0x4DBF    // CJK extension A
        | 0x4E00..=0x9FFF    // CJK unified ideographs
        | 0xF900..=0xFAFF    // compatibility ideographs
        | 0x20000..=0x2FA1F  // extensions B-F
    )
}

/// Tokenize text into index terms.
///
/// Latin runs become lowercased word tokens. CJK runs become unigrams *and*
/// bigrams, so a two-character Chinese query matches without a segmenter.
pub fn tokenize(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut latin = String::new();
    let mut cjk_run: Vec<char> = Vec::new();

    fn flush_latin(buf: &mut String, out: &mut Vec<String>) {
        if !buf.is_empty() {
            out.push(std::mem::take(buf));
        }
    }

    fn flush_cjk(run: &mut Vec<char>, out: &mut Vec<String>) {
        if run.is_empty() {
            return;
        }
        for ch in run.iter() {
            out.push(ch.to_string());
        }
        for pair in run.windows(2) {
            out.push(pair.iter().collect());
        }
        run.clear();
    }

    for ch in text.chars() {
        if is_cjk(ch) {
            flush_latin(&mut latin, &mut tokens);
            cjk_run.push(ch);
        } else if ch.is_alphanumeric() || ch == '_' || ch == '-' {
            flush_cjk(&mut cjk_run, &mut tokens);
            latin.extend(ch.to_lowercase());
        } else {
            flush_latin(&mut latin, &mut tokens);
            flush_cjk(&mut cjk_run, &mut tokens);
        }
    }
    flush_latin(&mut latin, &mut tokens);
    flush_cjk(&mut cjk_run, &mut tokens);

    tokens
}

impl MemoryIndex {
    /// Build an index from a memory document (one entry per non-empty line).
    ///
    /// Markdown headings and blockquote lines are treated as structure, not as
    /// retrievable memories, and are skipped.
    pub fn build(content: &str) -> Self {
        let mut docs = Vec::new();
        let mut df: HashMap<String, u32> = HashMap::new();

        for (line_no, raw) in content.lines().enumerate() {
            let text = raw.trim();
            if text.is_empty() || text.starts_with('#') || text.starts_with('>') {
                continue;
            }
            let tokens = tokenize(text);
            if tokens.is_empty() {
                continue;
            }
            let mut tf: HashMap<String, u32> = HashMap::new();
            for token in tokens {
                *tf.entry(token).or_insert(0) += 1;
            }
            for term in tf.keys() {
                *df.entry(term.clone()).or_insert(0) += 1;
            }
            docs.push(Doc {
                line_no,
                text: text.to_string(),
                len: tf.values().map(|count| *count as usize).sum(),
                tf,
            });
        }

        let avg_len = if docs.is_empty() {
            0.0
        } else {
            docs.iter().map(|doc| doc.len as f64).sum::<f64>() / docs.len() as f64
        };

        Self { docs, df, avg_len }
    }

    /// Number of indexed entries.
    pub fn len(&self) -> usize {
        self.docs.len()
    }

    /// Whether the index holds no entries.
    pub fn is_empty(&self) -> bool {
        self.docs.is_empty()
    }

    /// Search for the `top_k` highest-scoring entries.
    pub fn search(&self, query: &str, top_k: usize) -> Vec<MemoryHit> {
        if self.docs.is_empty() || top_k == 0 {
            return Vec::new();
        }
        let terms = tokenize(query);
        if terms.is_empty() {
            return Vec::new();
        }

        let total = self.docs.len() as f64;
        let avg_len = self.avg_len.max(1.0);
        let mut scored: Vec<MemoryHit> = Vec::new();

        for doc in &self.docs {
            let mut score = 0.0;
            for term in &terms {
                let Some(&freq) = doc.tf.get(term) else {
                    continue;
                };
                let doc_freq = f64::from(self.df.get(term).copied().unwrap_or(0));
                let idf = ((total - doc_freq + 0.5) / (doc_freq + 0.5) + 1.0).ln();
                let tf = f64::from(freq);
                let denom = tf + K1 * (1.0 - B + B * doc.len as f64 / avg_len);
                score += idf * (tf * (K1 + 1.0)) / denom;
            }
            if score > 0.0 {
                scored.push(MemoryHit {
                    line_no: doc.line_no,
                    text: doc.text.clone(),
                    score,
                });
            }
        }

        scored.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.line_no.cmp(&b.line_no)));
        scored.truncate(top_k);
        scored
    }

    /// Render the top hits as a prompt-ready bullet block, or an empty string.
    pub fn render(&self, query: &str, top_k: usize) -> String {
        let hits = self.search(query, top_k);
        if hits.is_empty() {
            return String::new();
        }
        hits.iter()
            .map(|hit| format!("- {}", hit.text))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MEMORY: &str = "\
# Memory

- User prefers concise replies with no filler.
- Deploy uses docker compose on port 3005.
- The gateway must never bind 3005 twice.
- 部署完成后需要重启 systemd 服务。
";

    #[test]
    fn tokenize_splits_latin_and_lowercases() {
        assert_eq!(
            tokenize("Deploy on PORT-3005"),
            vec!["deploy", "on", "port-3005"]
        );
    }

    #[test]
    fn tokenize_emits_cjk_bigrams() {
        let tokens = tokenize("部署重启");
        assert!(tokens.contains(&"部署".to_string()));
        assert!(tokens.contains(&"重启".to_string()));
    }

    #[test]
    fn headings_are_not_indexed() {
        let index = MemoryIndex::build(MEMORY);
        assert_eq!(index.len(), 4);
    }

    #[test]
    fn search_prefers_the_matching_entry_over_file_order() {
        let index = MemoryIndex::build(MEMORY);
        let hits = index.search("docker compose port", 1);
        assert_eq!(hits.len(), 1);
        assert!(hits[0].text.contains("docker compose"));
    }

    #[test]
    fn search_handles_cjk_query() {
        let index = MemoryIndex::build(MEMORY);
        let hits = index.search("部署", 1);
        assert_eq!(hits.len(), 1);
        assert!(hits[0].text.contains("部署"));
    }

    #[test]
    fn search_returns_empty_for_unmatched_query() {
        let index = MemoryIndex::build(MEMORY);
        assert!(index.search("kubernetes helm chart", 3).is_empty());
    }

    #[test]
    fn render_produces_bullets() {
        let index = MemoryIndex::build(MEMORY);
        let rendered = index.render("gateway 3005", 2);
        assert!(rendered.starts_with("- "));
    }

    #[test]
    fn empty_document_yields_empty_index() {
        let index = MemoryIndex::build("");
        assert!(index.is_empty());
        assert!(index.search("anything", 5).is_empty());
    }
}
