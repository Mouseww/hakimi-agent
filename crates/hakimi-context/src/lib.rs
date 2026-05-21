mod compressor;
mod engine;
pub mod intent;
mod memory;
mod prompt_builder;
pub mod role_adapter;
mod scrubber;
pub mod simple_engine;
pub mod smart_engine;

pub use compressor::{ContextCompressor, LlmCompressor};
pub use engine::{CompressionStats, ContextEngine};
pub use intent::{Intent, IntentClassifier, IntentPrediction};
pub use memory::{FileMemoryProvider, MemoryProvider, UserMemoryProvider};
pub use prompt_builder::{
    build_context_files_prompt, build_environment_hints, build_skills_prompt, build_system_prompt,
};
pub use role_adapter::{Role, RoleAdapter, RoleProfile};
pub use scrubber::{StreamingContextScrubber, sanitize_context};
pub use simple_engine::SimpleContextEngine;
pub use smart_engine::SmartContextEngine;
