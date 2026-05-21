use hakimi_common::ToolContext;

/// Builder for constructing a [`ToolContext`].
#[derive(Debug, Clone, Default)]
pub struct ToolContextBuilder {
    session_id: Option<String>,
    user_id: Option<String>,
    task_id: Option<String>,
    workdir: Option<String>,
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
            workdir: self
                .workdir
                .expect("workdir is required for ToolContext"),
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
        })
    }
}
