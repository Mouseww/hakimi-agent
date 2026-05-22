use std::sync::Arc;

use hakimi_common::{DelegateExecutor, KnowledgeSearcher, ToolContext};

/// Builder for constructing a [`ToolContext`].
#[derive(Clone, Default)]
pub struct ToolContextBuilder {
    session_id: Option<String>,
    user_id: Option<String>,
    task_id: Option<String>,
    workdir: Option<String>,
    model: Option<String>,
    delegate_executor: Option<Arc<dyn DelegateExecutor>>,
    knowledge_searcher: Option<Arc<dyn KnowledgeSearcher>>,
}

impl std::fmt::Debug for ToolContextBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolContextBuilder")
            .field("session_id", &self.session_id)
            .field("user_id", &self.user_id)
            .field("task_id", &self.task_id)
            .field("workdir", &self.workdir)
            .field("model", &self.model)
            .field("delegate_executor", &self.delegate_executor.is_some())
            .field("knowledge_searcher", &self.knowledge_searcher.is_some())
            .finish()
    }
}

impl ToolContextBuilder {
    /// Create a new builder with all fields unset.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the session ID.
    pub fn session_id(mut self, id: impl Into<String>) -> Self {
        self.session_id = Some(id.into());
        self
    }

    /// Set the user ID.
    pub fn user_id(mut self, id: impl Into<String>) -> Self {
        self.user_id = Some(id.into());
        self
    }

    /// Set the task ID.
    pub fn task_id(mut self, id: impl Into<String>) -> Self {
        self.task_id = Some(id.into());
        self
    }

    /// Set the working directory.
    pub fn workdir(mut self, dir: impl Into<String>) -> Self {
        self.workdir = Some(dir.into());
        self
    }

    /// Set the model identifier for child agent spawning.
    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Set the delegate executor for child agent spawning.
    pub fn delegate_executor(mut self, executor: Arc<dyn DelegateExecutor>) -> Self {
        self.delegate_executor = Some(executor);
        self
    }

    /// Set the knowledge searcher.
    pub fn knowledge_searcher(mut self, searcher: Arc<dyn KnowledgeSearcher>) -> Self {
        self.knowledge_searcher = Some(searcher);
        self
    }

    /// Build the [`ToolContext`].
    ///
    /// # Panics
    /// Panics if `session_id` or `workdir` have not been set.
    pub fn build(self) -> ToolContext {
        ToolContext {
            session_id: self
                .session_id
                .expect("session_id is required for ToolContext"),
            user_id: self.user_id,
            task_id: self.task_id,
            workdir: self.workdir.expect("workdir is required for ToolContext"),
            model: self.model,
            delegate_executor: self.delegate_executor,
            knowledge_searcher: self.knowledge_searcher,
        }
    }

    /// Try to build the [`ToolContext`], returning an error if required fields
    /// are missing.
    pub fn try_build(self) -> Result<ToolContext, String> {
        let session_id = self
            .session_id
            .ok_or_else(|| "session_id is required for ToolContext".to_string())?;
        let workdir = self
            .workdir
            .ok_or_else(|| "workdir is required for ToolContext".to_string())?;
        Ok(ToolContext {
            session_id,
            user_id: self.user_id,
            task_id: self.task_id,
            workdir,
            model: self.model,
            delegate_executor: self.delegate_executor,
            knowledge_searcher: self.knowledge_searcher,
        })
    }
}
