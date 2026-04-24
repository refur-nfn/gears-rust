use async_trait::async_trait;
use futures::stream::{self, BoxStream, StreamExt};
use uuid::Uuid;

use crate::error::PluginError;
use crate::models::{
    Capability, CapabilityValue, HealthStatus, Message, StreamingEvent, TenantId, UserId,
};

/// A boxed async stream of streaming events from a plugin.
///
/// Each item is a `Result`, so individual events can fail (e.g., mid-stream
/// network error) without aborting the stream. The outer `Result<PluginStream, _>`
/// returned by the trait methods represents errors that occur *before* the stream
/// starts (e.g., invalid config, plugin unavailable).
pub type PluginStream = BoxStream<'static, Result<StreamingEvent, PluginError>>;

/// Helper to build an empty plugin stream (default no-op responses).
#[must_use]
pub fn empty_stream() -> PluginStream {
    stream::empty().boxed()
}

/// Helper to build a plugin stream from a pre-collected vector of events.
///
/// Useful for non-streaming plugins or stub implementations that produce all
/// events up-front.
#[must_use]
pub fn stream_from_events(events: Vec<StreamingEvent>) -> PluginStream {
    stream::iter(events.into_iter().map(Ok)).boxed()
}

#[allow(clippy::module_name_repetitions)]
#[derive(Debug, Clone)]
pub struct SessionPluginCtx {
    pub session_type_id: Uuid,
    pub session_id: Option<Uuid>,
    pub call_ctx: PluginCallContext,
}

#[allow(clippy::module_name_repetitions)]
#[derive(Debug, Clone)]
pub struct MessagePluginCtx {
    pub session_id: Uuid,
    pub message_id: Uuid,
    pub messages: Vec<Message>,
    pub call_ctx: PluginCallContext,
}

/// Shared context attached to every plugin invocation.
#[allow(clippy::module_name_repetitions)]
#[derive(Debug, Clone)]
pub struct PluginCallContext {
    /// Correlation ID for this plugin invocation. Used for log correlation and
    /// distributed tracing; Chat Engine generates a fresh UUIDv4 per call (or
    /// may propagate an upstream correlation ID). Plugins should include this
    /// in every log line emitted while handling the call.
    pub request_id: Uuid,
    /// Tenant that owns the session issuing the call.
    pub tenant_id: TenantId,
    /// End-user behind the call (opaque string from the auth token).
    pub user_id: UserId,
    /// GTS plugin instance ID that is handling the call (matches the bound
    /// `SessionType.plugin_instance_id`).
    pub plugin_instance_id: String,
    /// Session type the call is scoped to.
    pub session_type_id: Uuid,
    /// Opaque plugin-specific configuration loaded from `plugin_configs` for
    /// this `(plugin_instance_id, session_type_id)` pair.
    pub plugin_config: Option<serde_json::Value>,
    /// Capability values selected for this call (subset of those declared by
    /// the plugin via `Capability`).
    pub enabled_capabilities: Option<Vec<CapabilityValue>>,
}

#[async_trait]
pub trait ChatEngineBackendPlugin: Send + Sync {
    async fn on_session_type_configured(
        &self,
        _ctx: SessionPluginCtx,
    ) -> Result<Vec<Capability>, PluginError> {
        Ok(vec![])
    }

    async fn on_session_created(
        &self,
        _ctx: SessionPluginCtx,
    ) -> Result<Vec<Capability>, PluginError> {
        Ok(vec![])
    }

    async fn on_session_updated(
        &self,
        _ctx: SessionPluginCtx,
    ) -> Result<Vec<Capability>, PluginError> {
        Ok(vec![])
    }

    /// Process a new user message and stream response events back.
    ///
    /// The outer `Result` reports failures *before* streaming starts (e.g., auth
    /// failure). Once a stream is returned, individual items may be `Err` to
    /// signal mid-stream failures (e.g., upstream disconnect).
    async fn on_message(
        &self,
        _ctx: MessagePluginCtx,
    ) -> Result<PluginStream, PluginError> {
        Ok(empty_stream())
    }

    /// Regenerate a response for an existing user message (new variant).
    ///
    /// Same streaming semantics as `on_message`.
    async fn on_message_recreate(
        &self,
        _ctx: MessagePluginCtx,
    ) -> Result<PluginStream, PluginError> {
        Ok(empty_stream())
    }

    /// Generate a session summary and stream the result back.
    ///
    /// Summary plugins typically emit one or more `Chunk` events followed by a
    /// `Complete` event carrying metadata.
    async fn on_session_summary(
        &self,
        _ctx: SessionPluginCtx,
    ) -> Result<PluginStream, PluginError> {
        Ok(empty_stream())
    }

    async fn health_check(&self) -> Result<HealthStatus, PluginError> {
        Ok(HealthStatus::Healthy)
    }

    fn plugin_instance_id(&self) -> &str;
}
