//! Plugin host for loading and managing plugins.
//!
//! This is the main integration crate that applications use to load,
//! manage, and interact with plugins.
//!
//! # Example
//!
//! ```rust,ignore
//! use lib_plugin_host::{PluginHost, PluginConfig};
//! use std::path::PathBuf;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let config = PluginConfig {
//!         plugins_dir: PathBuf::from("~/.myapp/plugins"),
//!         cache_dir: PathBuf::from("~/.cache/myapp/plugins"),
//!         registry_url: Some("https://plugins.example.com".into()),
//!         require_signatures: false,
//!         trusted_keys: vec![],
//!         host_version: "1.0.0".into(),
//!     };
//!
//!     let mut host = PluginHost::new(config)?;
//!     host.scan_installed()?;
//!
//!     // Install a plugin
//!     host.install_plugin("vendor.plugin", "1.0.0").await?;
//!
//!     // Enable it
//!     host.enable("vendor.plugin")?;
//!
//!     Ok(())
//! }
//! ```

mod callbacks;
mod config;
mod error;
mod host;
mod installed;
mod loader;
mod service_registry;

pub use callbacks::*;
pub use config::*;
pub use error::*;
pub use host::*;
pub use installed::*;
pub use loader::*;
pub use service_registry::*;

// Re-export dependencies for convenience
pub use lib_plugin_abi;
pub use lib_plugin_manifest;
pub use lib_plugin_registry;
pub use lib_plugin_verify;
