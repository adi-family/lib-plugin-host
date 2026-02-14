//! Plugin manager for v3 ABI

use crate::LoadedPluginV3;
use lib_plugin_abi_v3::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

// Thread-local storage for current plugin manager
thread_local! {
    static CURRENT_PLUGIN_MANAGER: RefCell<Option<Arc<PluginManagerV3>>> = const { RefCell::new(None) };
}

/// Set the current plugin manager in thread-local storage.
///
/// This should be called by the host before invoking plugin methods
/// to allow plugins to access other plugins' services.
pub fn set_current_plugin_manager(manager: Arc<PluginManagerV3>) {
    CURRENT_PLUGIN_MANAGER.with(|m| {
        *m.borrow_mut() = Some(manager);
    });
}

/// Clear the current plugin manager from thread-local storage.
pub fn clear_current_plugin_manager() {
    CURRENT_PLUGIN_MANAGER.with(|m| {
        *m.borrow_mut() = None;
    });
}

/// Get the current plugin manager from thread-local storage.
///
/// This is available to plugins during execution to access other
/// plugins' services (like language analyzers, embedders, etc.).
///
/// Returns `None` if no plugin manager is set (e.g., outside of plugin context).
pub fn current_plugin_manager() -> Option<Arc<PluginManagerV3>> {
    CURRENT_PLUGIN_MANAGER.with(|m| m.borrow().clone())
}

/// Plugin manager for v3 plugins
///
/// Manages loaded plugins and provides type-safe access to plugin services.
pub struct PluginManagerV3 {
    /// All loaded plugins
    plugins: HashMap<String, Arc<dyn Plugin>>,

    /// Service-specific lookups
    cli_commands: HashMap<String, Arc<dyn cli::CliCommands>>,
    http_routes: HashMap<String, Arc<dyn http::HttpRoutes>>,
    mcp_tools: HashMap<String, Arc<dyn mcp::McpTools>>,
    mcp_resources: HashMap<String, Arc<dyn mcp::McpResources>>,
    mcp_prompts: HashMap<String, Arc<dyn mcp::McpPrompts>>,

    // Language analyzer traits (keyed by language name, e.g., "rust", "python")
    language_analyzers: HashMap<String, Arc<dyn lang::LanguageAnalyzer>>,

    // Embedder trait (keyed by provider name, e.g., "fastembed", "openai")
    embedders: HashMap<String, Arc<dyn embed::Embedder>>,

    // Orchestration traits
    runners: HashMap<String, Arc<dyn runner::Runner>>,
    health_checks: HashMap<String, Arc<dyn health::HealthCheck>>,
    env_providers: HashMap<String, Arc<dyn env::EnvProvider>>,
    proxy_middleware: HashMap<String, Arc<dyn proxy::ProxyMiddleware>>,
    obs_sinks: HashMap<String, Arc<dyn obs::ObservabilitySink>>,
    rollout_strategies: HashMap<String, Arc<dyn rollout::RolloutStrategy>>,

    // Log streaming
    log_providers: HashMap<String, Arc<dyn logs::LogProvider>>,
}

impl PluginManagerV3 {
    /// Create a new plugin manager
    pub fn new() -> Self {
        Self {
            plugins: HashMap::new(),
            cli_commands: HashMap::new(),
            http_routes: HashMap::new(),
            mcp_tools: HashMap::new(),
            mcp_resources: HashMap::new(),
            mcp_prompts: HashMap::new(),
            language_analyzers: HashMap::new(),
            embedders: HashMap::new(),
            runners: HashMap::new(),
            health_checks: HashMap::new(),
            env_providers: HashMap::new(),
            proxy_middleware: HashMap::new(),
            obs_sinks: HashMap::new(),
            rollout_strategies: HashMap::new(),
            log_providers: HashMap::new(),
        }
    }

    /// Register a loaded plugin
    pub fn register(&mut self, loaded: LoadedPluginV3) -> lib_plugin_abi_v3::Result<()> {
        let plugin_id = loaded.metadata().id.clone();
        let plugin = loaded.plugin;

        // Store base plugin
        self.plugins.insert(plugin_id.clone(), plugin.clone());

        // Register CLI commands if available
        if let Some(cli) = loaded.cli_commands {
            self.cli_commands.insert(plugin_id.clone(), cli);
            tracing::debug!("Registered CLI commands for plugin: {}", plugin_id);
        }

        // Register log provider if available
        if let Some(log_provider) = loaded.log_provider {
            self.log_providers.insert(plugin_id.clone(), log_provider);
            tracing::debug!("Registered log provider for plugin: {}", plugin_id);
        }

        Ok(())
    }

    /// Register a CLI commands plugin
    pub fn register_cli_commands(&mut self, plugin_id: impl Into<String>, plugin: Arc<dyn cli::CliCommands>) {
        self.cli_commands.insert(plugin_id.into(), plugin);
    }

    /// Register an HTTP routes plugin
    pub fn register_http_routes(&mut self, plugin_id: impl Into<String>, plugin: Arc<dyn http::HttpRoutes>) {
        self.http_routes.insert(plugin_id.into(), plugin);
    }

    /// Register a runner plugin
    pub fn register_runner(&mut self, runner_type: impl Into<String>, plugin: Arc<dyn runner::Runner>) {
        self.runners.insert(runner_type.into(), plugin);
    }

    /// Register a health check plugin
    pub fn register_health_check(&mut self, check_type: impl Into<String>, plugin: Arc<dyn health::HealthCheck>) {
        self.health_checks.insert(check_type.into(), plugin);
    }

    /// Get a CLI commands plugin
    pub fn get_cli_commands(&self, plugin_id: &str) -> Option<Arc<dyn cli::CliCommands>> {
        self.cli_commands.get(plugin_id).cloned()
    }

    /// Get all CLI commands plugins
    pub fn all_cli_commands(&self) -> Vec<(String, Arc<dyn cli::CliCommands>)> {
        self.cli_commands
            .iter()
            .map(|(id, plugin)| (id.clone(), plugin.clone()))
            .collect()
    }

    /// Get an HTTP routes plugin
    pub fn get_http_routes(&self, plugin_id: &str) -> Option<Arc<dyn http::HttpRoutes>> {
        self.http_routes.get(plugin_id).cloned()
    }

    /// Get all HTTP routes plugins
    pub fn all_http_routes(&self) -> Vec<(String, Arc<dyn http::HttpRoutes>)> {
        self.http_routes
            .iter()
            .map(|(id, plugin)| (id.clone(), plugin.clone()))
            .collect()
    }

    /// Get an MCP tools plugin
    pub fn get_mcp_tools(&self, plugin_id: &str) -> Option<Arc<dyn mcp::McpTools>> {
        self.mcp_tools.get(plugin_id).cloned()
    }

    /// Get all MCP tools plugins
    pub fn all_mcp_tools(&self) -> Vec<(String, Arc<dyn mcp::McpTools>)> {
        self.mcp_tools
            .iter()
            .map(|(id, plugin)| (id.clone(), plugin.clone()))
            .collect()
    }

    /// Get a runner plugin
    pub fn get_runner(&self, runner_type: &str) -> Option<Arc<dyn runner::Runner>> {
        self.runners.get(runner_type).cloned()
    }

    /// Get all runners
    pub fn all_runners(&self) -> Vec<(String, Arc<dyn runner::Runner>)> {
        self.runners
            .iter()
            .map(|(id, plugin)| (id.clone(), plugin.clone()))
            .collect()
    }

    /// Get a health check plugin
    pub fn get_health_check(&self, check_type: &str) -> Option<Arc<dyn health::HealthCheck>> {
        self.health_checks.get(check_type).cloned()
    }

    /// Get all health checks
    pub fn all_health_checks(&self) -> Vec<(String, Arc<dyn health::HealthCheck>)> {
        self.health_checks
            .iter()
            .map(|(id, plugin)| (id.clone(), plugin.clone()))
            .collect()
    }

    /// Get an environment provider plugin
    pub fn get_env_provider(&self, provider_type: &str) -> Option<Arc<dyn env::EnvProvider>> {
        self.env_providers.get(provider_type).cloned()
    }

    /// Get a proxy middleware plugin
    pub fn get_proxy_middleware(&self, middleware_type: &str) -> Option<Arc<dyn proxy::ProxyMiddleware>> {
        self.proxy_middleware.get(middleware_type).cloned()
    }

    /// Get an observability sink plugin
    pub fn get_obs_sink(&self, sink_type: &str) -> Option<Arc<dyn obs::ObservabilitySink>> {
        self.obs_sinks.get(sink_type).cloned()
    }

    /// Get a rollout strategy plugin
    pub fn get_rollout_strategy(&self, strategy_type: &str) -> Option<Arc<dyn rollout::RolloutStrategy>> {
        self.rollout_strategies.get(strategy_type).cloned()
    }

    /// Register a log provider plugin
    pub fn register_log_provider(&mut self, plugin_id: impl Into<String>, plugin: Arc<dyn logs::LogProvider>) {
        self.log_providers.insert(plugin_id.into(), plugin);
    }

    /// Get a log provider plugin
    pub fn get_log_provider(&self, plugin_id: &str) -> Option<Arc<dyn logs::LogProvider>> {
        self.log_providers.get(plugin_id).cloned()
    }

    /// Register a language analyzer plugin
    pub fn register_language_analyzer(&mut self, language: impl Into<String>, plugin: Arc<dyn lang::LanguageAnalyzer>) {
        self.language_analyzers.insert(language.into(), plugin);
    }

    /// Get a language analyzer plugin by language name (e.g., "rust", "python")
    pub fn get_language_analyzer(&self, language: &str) -> Option<Arc<dyn lang::LanguageAnalyzer>> {
        self.language_analyzers.get(language).cloned()
    }

    /// Get all language analyzer plugins
    pub fn all_language_analyzers(&self) -> Vec<(String, Arc<dyn lang::LanguageAnalyzer>)> {
        self.language_analyzers
            .iter()
            .map(|(lang, plugin)| (lang.clone(), plugin.clone()))
            .collect()
    }

    /// Check if a language analyzer is available for a language
    pub fn has_language_analyzer(&self, language: &str) -> bool {
        self.language_analyzers.contains_key(language)
    }

    /// Register an embedder plugin
    pub fn register_embedder(&mut self, provider: impl Into<String>, plugin: Arc<dyn embed::Embedder>) {
        self.embedders.insert(provider.into(), plugin);
    }

    /// Get an embedder plugin by provider name (e.g., "fastembed", "openai")
    pub fn get_embedder(&self, provider: &str) -> Option<Arc<dyn embed::Embedder>> {
        self.embedders.get(provider).cloned()
    }

    /// Get the default embedder (first available)
    pub fn get_default_embedder(&self) -> Option<Arc<dyn embed::Embedder>> {
        self.embedders.values().next().cloned()
    }

    /// Get all embedder plugins
    pub fn all_embedders(&self) -> Vec<(String, Arc<dyn embed::Embedder>)> {
        self.embedders
            .iter()
            .map(|(provider, plugin)| (provider.clone(), plugin.clone()))
            .collect()
    }

    /// Check if any embedder is available
    pub fn has_embedder(&self) -> bool {
        !self.embedders.is_empty()
    }

    /// Get a plugin by ID
    pub fn get_plugin(&self, plugin_id: &str) -> Option<Arc<dyn Plugin>> {
        self.plugins.get(plugin_id).cloned()
    }

    /// List all loaded plugins
    pub fn list_plugins(&self) -> Vec<PluginMetadata> {
        self.plugins
            .values()
            .map(|p| p.metadata())
            .collect()
    }

    /// Unload all plugins
    pub async fn shutdown_all(&mut self) -> lib_plugin_abi_v3::Result<()> {
        for (_id, plugin) in self.plugins.drain() {
            if let Err(e) = plugin.shutdown().await {
                eprintln!("Error shutting down plugin: {}", e);
            }
        }

        // Clear all service registries
        self.cli_commands.clear();
        self.http_routes.clear();
        self.mcp_tools.clear();
        self.mcp_resources.clear();
        self.mcp_prompts.clear();
        self.language_analyzers.clear();
        self.embedders.clear();
        self.runners.clear();
        self.health_checks.clear();
        self.env_providers.clear();
        self.proxy_middleware.clear();
        self.obs_sinks.clear();
        self.rollout_strategies.clear();
        self.log_providers.clear();

        Ok(())
    }
}

impl Default for PluginManagerV3 {
    fn default() -> Self {
        Self::new()
    }
}
