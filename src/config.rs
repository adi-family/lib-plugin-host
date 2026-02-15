//! Plugin host configuration.

use std::path::PathBuf;

/// Configuration for the plugin host.
#[derive(Debug, Clone)]
pub struct PluginConfig {
    /// Directory where plugins are installed
    pub plugins_dir: PathBuf,

    /// Cache directory for downloads
    pub cache_dir: PathBuf,

    /// Registry URL (None = no registry, local only)
    pub registry_url: Option<String>,

    /// Require signature verification
    pub require_signatures: bool,

    /// Trusted public keys (base64 encoded)
    pub trusted_keys: Vec<String>,

    /// Host application version (for compatibility checks)
    pub host_version: String,
}

impl PluginConfig {
    /// Create a new configuration with required paths.
    pub fn new(plugins_dir: PathBuf, cache_dir: PathBuf) -> Self {
        Self {
            plugins_dir,
            cache_dir,
            registry_url: None,
            require_signatures: false,
            trusted_keys: Vec::new(),
            host_version: String::new(),
        }
    }

    /// Set the registry URL.
    pub fn with_registry(mut self, url: impl Into<String>) -> Self {
        self.registry_url = Some(url.into());
        self
    }

    /// Enable signature verification.
    pub fn require_signatures(mut self, require: bool) -> Self {
        self.require_signatures = require;
        self
    }

    /// Add a trusted key.
    pub fn with_trusted_key(mut self, key: impl Into<String>) -> Self {
        self.trusted_keys.push(key.into());
        self
    }

    /// Add multiple trusted keys.
    pub fn with_trusted_keys(mut self, keys: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.trusted_keys.extend(keys.into_iter().map(Into::into));
        self
    }

    /// Set the host version.
    pub fn with_host_version(mut self, version: impl Into<String>) -> Self {
        self.host_version = version.into();
        self
    }

    /// Ensure directories exist.
    pub fn ensure_dirs(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.plugins_dir)?;
        std::fs::create_dir_all(&self.cache_dir)?;
        Ok(())
    }
}

impl PluginConfig {
    /// Default plugins installation directory (`<data_local_dir>/adi/plugins`).
    pub fn default_plugins_dir() -> PathBuf {
        dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("adi")
            .join("plugins")
    }

    /// Default registry cache directory (`<cache_dir>/adi/registry-cache`).
    pub fn default_cache_dir() -> PathBuf {
        dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("adi")
            .join("registry-cache")
    }
}

impl Default for PluginConfig {
    fn default() -> Self {
        Self {
            plugins_dir: Self::default_plugins_dir(),
            cache_dir: Self::default_cache_dir(),
            registry_url: None,
            require_signatures: false,
            trusted_keys: Vec::new(),
            host_version: String::new(),
        }
    }
}
