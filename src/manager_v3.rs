//! Plugin manager for v3 ABI

use crate::{LoadedPluginV3, PluginError, Result};
use lib_plugin_abi_v3::*;
use std::collections::HashMap;
use std::sync::Arc;

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

    // Orchestration traits
    runners: HashMap<String, Arc<dyn runner::Runner>>,
    health_checks: HashMap<String, Arc<dyn health::HealthCheck>>,
    env_providers: HashMap<String, Arc<dyn env::EnvProvider>>,
    proxy_middleware: HashMap<String, Arc<dyn proxy::ProxyMiddleware>>,
    obs_sinks: HashMap<String, Arc<dyn obs::ObservabilitySink>>,
    rollout_strategies: HashMap<String, Arc<dyn rollout::RolloutStrategy>>,
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
            runners: HashMap::new(),
            health_checks: HashMap::new(),
            env_providers: HashMap::new(),
            proxy_middleware: HashMap::new(),
            obs_sinks: HashMap::new(),
            rollout_strategies: HashMap::new(),
        }
    }

    /// Register a loaded plugin
    pub fn register(&mut self, loaded: LoadedPluginV3) -> Result<()> {
        let plugin_id = loaded.metadata().id.clone();
        let plugin = loaded.plugin;

        // Store base plugin
        self.plugins.insert(plugin_id.clone(), plugin.clone());

        // Try to downcast to service traits and register
        // Note: This is a workaround. In real implementation, plugins would
        // declare their provided services in the manifest.

        // For now, we'll need plugins to implement a method that tells us
        // what services they provide, or we check the manifest.

        // TODO: Implement service registration based on manifest `provides` field

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
    pub async fn shutdown_all(&mut self) -> Result<()> {
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
        self.runners.clear();
        self.health_checks.clear();
        self.env_providers.clear();
        self.proxy_middleware.clear();
        self.obs_sinks.clear();
        self.rollout_strategies.clear();

        Ok(())
    }
}

impl Default for PluginManagerV3 {
    fn default() -> Self {
        Self::new()
    }
}
