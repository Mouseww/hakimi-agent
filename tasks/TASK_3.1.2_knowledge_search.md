# TASK 3.1.2: Knowledge Base Full-Text Search

**状态**: 🔄 待执行 (0%)  
**优先级**: P1  
**预计工作量**: 4-5 小时  
**依赖**: 无

## 📋 任务目标

为知识库实现高效的全文搜索功能，支持关键词检索、模糊匹配和相关性排序。

## 🎯 成功标准

- [x] 支持关键词全文搜索
- [x] 支持模糊匹配和通配符
- [x] 相关性评分和排序
- [x] 搜索结果高亮显示
- [x] 搜索性能优化（10,000 条目 < 100ms）
- [x] 单元测试覆盖 ≥ 90%

## 🔧 实现步骤

### 1. 实现搜索索引

**文件**: `crates/hakimi-knowledge/src/search_index.rs` (新建)

```rust
use tantivy::{Index, IndexWriter, schema::*};

pub struct KnowledgeSearchIndex {
    index: Index,
    writer: IndexWriter,
    schema: Schema,
}

impl KnowledgeSearchIndex {
    pub fn new(index_path: &str) -> Result<Self> {
        let mut schema_builder = Schema::builder();
        schema_builder.add_text_field("id", STRING | STORED);
        schema_builder.add_text_field("content", TEXT | STORED);
        schema_builder.add_text_field("tags", TEXT);
        schema_builder.add_i64_field("created_at", INDEXED);
        
        let schema = schema_builder.build();
        let index = Index::create_in_dir(index_path, schema.clone())?;
        let writer = index.writer(50_000_000)?;
        
        Ok(Self { index, writer, schema })
    }
    
    pub fn index_entry(&mut self, entry: &KnowledgeEntry) -> Result<()>;
    pub fn delete_entry(&mut self, id: &str) -> Result<()>;
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>>;
    pub fn commit(&mut self) -> Result<()>;
}

pub struct SearchResult {
    pub id: String,
    pub content: String,
    pub score: f32,
    pub highlights: Vec<String>,
}
```

### 2. 集成搜索到知识库

**文件**: `crates/hakimi-knowledge/src/knowledge_base.rs`

```rust
impl KnowledgeBase {
    pub fn search(&self, query: &str, options: SearchOptions) -> Result<Vec<SearchResult>> {
        let results = self.search_index.search(query, options.limit)?;
        
        // 应用过滤器
        let filtered = results.into_iter()
            .filter(|r| self.matches_filters(r, &options))
            .collect();
            
        Ok(filtered)
    }
    
    pub fn search_with_tags(&self, query: &str, tags: &[String]) -> Result<Vec<SearchResult>> {
        // 组合搜索：关键词 + 标签过滤
    }
    
    pub fn search_by_date_range(&self, query: &str, start: i64, end: i64) -> Result<Vec<SearchResult>> {
        // 带时间范围的搜索
    }
}

pub struct SearchOptions {
    pub limit: usize,
    pub tags: Option<Vec<String>>,
    pub date_range: Option<(i64, i64)>,
    pub min_score: Option<f32>,
}
```

### 3. 添加搜索 API 端点

**文件**: `crates/hakimi-server/src/routes/knowledge.rs`

```rust
#[derive(Deserialize)]
pub struct SearchQuery {
    pub q: String,
    pub tags: Option<Vec<String>>,
    pub limit: Option<usize>,
    pub min_score: Option<f32>,
}

// GET /api/knowledge/search?q=keyword&tags=tag1,tag2&limit=10
pub async fn search_knowledge(
    Query(query): Query<SearchQuery>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<SearchResult>>, StatusCode> {
    let options = SearchOptions {
        limit: query.limit.unwrap_or(20),
        tags: query.tags,
        date_range: None,
        min_score: query.min_score,
    };
    
    let kb = state.knowledge_base.lock().await;
    let results = kb.search(&query.q, options)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    Ok(Json(results))
}
```

### 4. 实现高亮显示

**文件**: `crates/hakimi-knowledge/src/highlighter.rs` (新建)

```rust
pub struct Highlighter {
    pre_tag: String,
    post_tag: String,
}

impl Highlighter {
    pub fn new() -> Self {
        Self {
            pre_tag: "<mark>".to_string(),
            post_tag: "</mark>".to_string(),
        }
    }
    
    pub fn highlight(&self, text: &str, query: &str) -> Vec<String> {
        // 查找匹配片段并高亮
        // 返回包含高亮标记的文本片段列表
    }
    
    pub fn extract_snippets(&self, text: &str, query: &str, snippet_size: usize) -> Vec<String> {
        // 提取包含关键词的上下文片段
    }
}
```

### 5. 添加搜索索引维护

**文件**: `crates/hakimi-knowledge/src/index_manager.rs` (新建)

```rust
pub struct IndexManager {
    index: Arc<Mutex<KnowledgeSearchIndex>>,
}

impl IndexManager {
    pub fn rebuild_index(&self, kb: &KnowledgeBase) -> Result<()> {
        // 重建完整索引
        let entries = kb.get_all_entries()?;
        let mut index = self.index.lock().unwrap();
        
        for entry in entries {
            index.index_entry(&entry)?;
        }
        
        index.commit()?;
        Ok(())
    }
    
    pub fn optimize_index(&self) -> Result<()> {
        // 优化索引性能
    }
    
    pub async fn start_auto_commit(&self, interval_secs: u64) {
        // 定期自动提交索引
    }
}
```

### 6. 单元测试

**文件**: `crates/hakimi-knowledge/src/search_test.rs`

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_simple_keyword_search() {
        // 测试基本关键词搜索
    }
    
    #[test]
    fn test_fuzzy_search() {
        // 测试模糊搜索
    }
    
    #[test]
    fn test_tag_filter() {
        // 测试标签过滤
    }
    
    #[test]
    fn test_relevance_scoring() {
        // 测试相关性评分
    }
    
    #[test]
    fn test_highlight_extraction() {
        // 测试高亮提取
    }
    
    #[test]
    fn test_search_performance() {
        // 性能测试：10,000 条目
    }
}
```

## 🔍 验证清单

- [ ] 所有单元测试通过
- [ ] 搜索结果准确且相关
- [ ] 高亮显示正确标记关键词
- [ ] 模糊搜索正确处理拼写变体
- [ ] 标签过滤正确工作
- [ ] 大规模数据集搜索性能达标
- [ ] 索引自动更新机制正常

## 📊 性能指标

- 10,000 条目搜索: < 100ms
- 100,000 条目搜索: < 500ms
- 索引更新延迟: < 50ms
- 内存占用: < 索引大小 × 2

## 🔗 相关文件

- `crates/hakimi-knowledge/src/search_index.rs` (新建)
- `crates/hakimi-knowledge/src/highlighter.rs` (新建)
- `crates/hakimi-knowledge/src/index_manager.rs` (新建)
- `crates/hakimi-knowledge/src/knowledge_base.rs`
- `crates/hakimi-server/src/routes/knowledge.rs`

## 📝 注意事项

1. 考虑使用 tantivy 或 sonic 等 Rust 全文搜索库
2. 索引应增量更新而非每次完全重建
3. 搜索结果缓存可显著提升性能
4. 支持中文分词（jieba-rs）
5. 考虑搜索历史和热门搜索统计
