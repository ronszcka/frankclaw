#![forbid(unsafe_code)]

pub mod catalog;
mod failover;
mod openai;
mod openai_compat;
mod anthropic;
mod ollama;
mod sse;

pub use failover::{FailoverChain, ProviderHealth};
pub use openai::OpenAiProvider;
pub use anthropic::AnthropicProvider;
pub use ollama::OllamaProvider;
