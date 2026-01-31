//! Plugin host for loading and managing v3 plugins.
//!
//! This is the main integration crate that applications use to load,
//! manage, and interact with plugins using the v3 plugin ABI.
//!
//! # Example
//!
//! ```rust,ignore
//! use lib_plugin_host::{PluginManagerV3, LoadedPluginV3, PluginConfig};
//! use std::path::PathBuf;
//! use std::sync::Arc;
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
//!     config.ensure_dirs()?;
//!
//!     let mut manager = PluginManagerV3::new();
//!
//!     // Load a plugin
//!     let manifest = lib_plugin_manifest::PluginManifest::from_file("plugin.toml")?;
//!     let loaded = LoadedPluginV3::load(manifest, &config.plugins_dir).await?;
//!     manager.register(loaded)?;
//!
//!     // Set as current for plugin-to-plugin access
//!     let manager = Arc::new(manager);
//!     lib_plugin_host::set_current_plugin_manager(manager.clone());
//!
//!     Ok(())
//! }
//! ```

mod config;
mod error;
mod installed;

// V3 plugin support
mod loader_v3;
mod manager_v3;

pub use config::*;
pub use error::*;
pub use installed::*;

// V3 exports
pub use loader_v3::*;
pub use manager_v3::*;

// Re-export dependencies for convenience
pub use lib_plugin_abi_v3;
pub use lib_plugin_manifest;
pub use lib_plugin_registry;
pub use lib_plugin_verify;
