use std::sync::Arc;

use hakimi_common::{DelegateExecutor, KnowledgeSearcher, ToolContext, ToolProgressCallback};

/// Builder for constructing a [`ToolContext`].
#[derive(Clone, Default)]
pub struct ToolContextBuilder {
    session_id: Option<String>,
    user_id: Option<String>,
    task_id: Option<String>,
    workdir: Option<String>,
    model: Option<String>,
    delegate_executor: Option<Arc<dyn DelegateExecutor>>,
    tts_provider: Option<String>,
    tts_model: Option<String>,
    tts_base_url: Option<String>,
    tts_api_key: Option<String>,
    tts_voice: Option<String>,
    transcription_provider: Option<String>,
    transcription_model: Option<String>,
    transcription_base_url: Option<String>,
    transcription_api_key: Option<String>,
    knowledge_searcher: Option<Arc<dyn KnowledgeSearcher>>,
    progress_callback: Option<ToolProgressCallback>,
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
            .field("progress_callback", &self.progress_callback.is_some())
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

    /// Set the TTS provider.
    pub fn tts_provider(mut self, provider: impl Into<String>) -> Self {
        self.tts_provider = Some(provider.into());
        self
    }

    /// Set the TTS model.
    pub fn tts_model(mut self, model: impl Into<String>) -> Self {
        self.tts_model = Some(model.into());
        self
    }

    /// Set the TTS base URL.
    pub fn tts_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.tts_base_url = Some(base_url.into());
        self
    }

    /// Set the TTS API key.
    pub fn tts_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.tts_api_key = Some(api_key.into());
        self
    }

    /// Set the default TTS voice.
    pub fn tts_voice(mut self, voice: impl Into<String>) -> Self {
        self.tts_voice = Some(voice.into());
        self
    }

    /// Set the transcription provider.
    pub fn transcription_provider(mut self, provider: impl Into<String>) -> Self {
        self.transcription_provider = Some(provider.into());
        self
    }

    /// Set the transcription model.
    pub fn transcription_model(mut self, model: impl Into<String>) -> Self {
        self.transcription_model = Some(model.into());
        self
    }

    /// Set the transcription base URL.
    pub fn transcription_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.transcription_base_url = Some(base_url.into());
        self
    }

    /// Set the transcription API key.
    pub fn transcription_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.transcription_api_key = Some(api_key.into());
        self
    }

    /// Set the knowledge searcher.
    pub fn knowledge_searcher(mut self, searcher: Arc<dyn KnowledgeSearcher>) -> Self {
        self.knowledge_searcher = Some(searcher);
        self
    }

    /// Set the progress callback for long-running tools.
    pub fn progress_callback(mut self, callback: ToolProgressCallback) -> Self {
        self.progress_callback = Some(callback);
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
            progress_callback: self.progress_callback,
            tts_provider: self.tts_provider,
            tts_model: self.tts_model,
            tts_base_url: self.tts_base_url,
            tts_api_key: self.tts_api_key,
            tts_voice: self.tts_voice,
            transcription_provider: self.transcription_provider,
            transcription_model: self.transcription_model,
            transcription_base_url: self.transcription_base_url,
            transcription_api_key: self.transcription_api_key,
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
            progress_callback: self.progress_callback,
            tts_provider: self.tts_provider,
            tts_model: self.tts_model,
            tts_base_url: self.tts_base_url,
            tts_api_key: self.tts_api_key,
            tts_voice: self.tts_voice,
            transcription_provider: self.transcription_provider,
            transcription_model: self.transcription_model,
            transcription_base_url: self.transcription_base_url,
            transcription_api_key: self.transcription_api_key,
        })
    }
}
