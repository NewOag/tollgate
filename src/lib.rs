//! A local LLM gateway: sits in front of OpenAI/Anthropic-compatible client
//! SDKs, forwards requests to the real upstream unmodified, and logs
//! usage/latency/cost/full request-response content to a local SQLite file.
//!
//! Exposed as a library so both the `tollgate` CLI binary (`main.rs`) and
//! the Tauri app (`app/src-tauri`) can share the same gateway/store/config
//! implementation, calling straight into it in-process rather than through
//! HTTP.

pub mod config;
pub mod format;
pub mod gateway;
pub mod mdns;
pub mod pricing;
pub mod shutdown;
pub mod store;
pub mod tls;
