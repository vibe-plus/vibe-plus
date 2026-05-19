//! Plugin API for Vibe Plus.
//!
//! The gateway core emits typed events into a registry. Built-in or future
//! external plugins subscribe to those events and own side effects such as
//! observability persistence.

use std::sync::Arc;

use axum::Router;
use vibe_protocol::{AppLogEvent, RequestLog, UpstreamAttemptLog};

#[derive(Debug, Clone)]
pub enum GatewayEvent {
    RequestFinished(RequestLog),
    UpstreamAttemptFinished(UpstreamAttemptLog),
    AppLog(AppLogEvent),
}

pub trait EventSink: Send + Sync + 'static {
    fn emit(&self, event: GatewayEvent);
}

pub trait Plugin<State>: EventSink {
    fn name(&self) -> &'static str;

    fn router(&self) -> Option<Router<State>> {
        None
    }
}

#[derive(Clone, Default)]
pub struct PluginRegistry<State = ()> {
    plugins: Arc<Vec<Arc<dyn Plugin<State>>>>,
}

impl<State> PluginRegistry<State>
where
    State: Clone + Send + Sync + 'static,
{
    pub fn new() -> Self {
        Self {
            plugins: Arc::new(Vec::new()),
        }
    }

    pub fn with_plugin<P>(mut self, plugin: P) -> Self
    where
        P: Plugin<State> + 'static,
    {
        Arc::make_mut(&mut self.plugins).push(Arc::new(plugin));
        self
    }

    pub fn emit(&self, event: GatewayEvent) {
        for plugin in self.plugins.iter() {
            plugin.emit(event.clone());
        }
    }

    pub fn routers(&self) -> Vec<(&'static str, Router<State>)> {
        self.plugins
            .iter()
            .filter_map(|p| p.router().map(|r| (p.name(), r)))
            .collect()
    }

    pub fn names(&self) -> Vec<&'static str> {
        self.plugins.iter().map(|p| p.name()).collect()
    }
}

impl<State: 'static> std::fmt::Debug for PluginRegistry<State> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let names: Vec<&'static str> = self.plugins.iter().map(|p| p.name()).collect();
        f.debug_struct("PluginRegistry")
            .field("plugins", &names)
            .finish()
    }
}
