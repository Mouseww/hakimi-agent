mod advanced_compressor;
pub mod archive;
mod compressor;
mod engine;
pub mod error;
mod factory;
pub mod intent;
mod memory;
mod memory_cache;
mod prompt_builder;
pub mod role_adapter;
mod scrubber;
pub mod simple_engine;
pub mod smart_engine;

pub use advanced_compressor::{AdvancedCompressor, CompressionConfig};
pub use compressor::{ContextCompressor, LlmCompressor};
pub use engine::{CompressionStats, ContextEngine};
pub use factory::build_context_engine;
pub use intent::{Intent, IntentClassifier, IntentPrediction};
pub use memory::{FileMemoryProvider, MemoryProvider, UserMemoryProvider};
pub use memory_cache::{CacheStats, MemoryCache};
pub use prompt_builder::{
    build_context_files_prompt, build_environment_hints, build_skills_prompt, build_system_prompt,
};
pub use role_adapter::{Role, RoleAdapter, RoleProfile};
pub use scrubber::{StreamingContextScrubber, sanitize_context};
pub use simple_engine::SimpleContextEngine;
pub use smart_engine::SmartContextEngine;
// Export archive module
pub use archive::{ArchiveInfo, ArchiveStats, MemoryArchive, MemoryEntry};
