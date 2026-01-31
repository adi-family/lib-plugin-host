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

    /// Plugin already enabled
    #[error("Plugin already enabled: {0}")]
    AlreadyEnabled(String),

    /// Plugin not enabled
    #[error("Plugin not enabled: {0}")]
    NotEnabled(String),

    /// Failed to load plugin library
    #[error("Failed to load plugin: {0}")]
    LoadFailed(String),

    /// Plugin entry symbol not found
    #[error("Plugin entry symbol not found: {0}")]
    SymbolNotFound(String),

    /// Invalid plugin vtable
    #[error("Invalid plugin vtable")]
    InvalidVTable,

    /// Incompatible API version
    #[error("Incompatible API version: expected {expected}, got {actual}")]
    IncompatibleApiVersion { expected: u32, actual: u32 },

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

    /// Signature verification failed
    #[error("Signature verification failed: {0}")]
    SignatureInvalid(String),

    /// Circular dependency detected
    #[error("Circular dependency detected: {0}")]
    CircularDependency(String),

    /// Dependency not found
    #[error("Dependency not found: {0}")]
    DependencyNotFound(String),

    /// Required service not available
    #[error("Required service not available: {service} - {error}")]
    ServiceNotAvailable { service: String, error: String },

    /// Dependency failed to load
    #[error("Dependency '{dependency}' failed to load for plugin '{plugin}': {error}")]
    DependencyLoadFailed {
        plugin: String,
        dependency: String,
        error: String,
    },

    /// Service registration failed
    #[error("Service registration failed: {0}")]
    ServiceRegistrationFailed(String),
}

/// Alias for PluginError - used internally for v3 plugin loading
pub type PluginError = HostError;

/// Result type for plugin host operations
pub type Result<T> = std::result::Result<T, HostError>;
