//! Error types for plugin host operations.

use thiserror::Error;

/// Errors that can occur during plugin host operations.
#[derive(Debug, Error)]
pub enum HostError {
    /// Plugin not found
    #[error("Plugin not found: {0}")]
    PluginNotFound(String),

    /// Package not found
    #[error("Package not found: {0}")]
    PackageNotFound(String),

    /// Plugin already installed
    #[error("Plugin already installed: {0}")]
    AlreadyInstalled(String),

    /// Plugin not installed
    #[error("Plugin not installed: {0}")]
    NotInstalled(String),

    /// Failed to load plugin library
    #[error("Failed to load plugin: {0}")]
    LoadFailed(String),

    /// Plugin initialization failed
    #[error("Plugin initialization failed: {0}")]
    InitFailed(String),

    /// Manifest error
    #[error("Manifest error: {0}")]
    Manifest(#[from] lib_plugin_manifest::ManifestError),

    /// Registry error
    #[error("Registry error: {0}")]
    Registry(#[from] lib_plugin_registry::RegistryError),

    /// Verification error
    #[error("Verification error: {0}")]
    Verify(#[from] lib_plugin_verify::VerifyError),

    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Platform not supported
    #[error("Platform not supported: {0}")]
    PlatformNotSupported(String),

    /// Plugin error from v3 ABI
    #[error("Plugin error: {0}")]
    Plugin(#[from] lib_plugin_abi_v3::PluginError),
}

/// Alias for PluginError - used internally for v3 plugin loading
pub type PluginError = HostError;

/// Result type for plugin host operations
pub type Result<T> = std::result::Result<T, HostError>;
