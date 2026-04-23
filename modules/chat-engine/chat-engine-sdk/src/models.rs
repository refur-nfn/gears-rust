use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub session_id: Uuid,
    pub tenant_id: String,
    pub user_id: String,
    pub client_id: Option<String>,
    pub session_type_id: Option<Uuid>,
    pub enabled_capabilities: Option<serde_json::Value>,
    pub metadata: Option<serde_json::Value>,
    pub lifecycle_state: LifecycleState,
    pub share_token: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

/// Lifecycle state of a session.
///
/// Allowed transitions:
/// - `Active` ↔ `Archived`, `Active` → `SoftDeleted`, `Active` → `HardDeleted`
/// - `Archived` → `SoftDeleted`, `Archived` → `HardDeleted`
/// - `SoftDeleted` → `Active`, `SoftDeleted` → `HardDeleted`
/// - `HardDeleted` is terminal
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleState {
    Active,
    Archived,
    SoftDeleted,
    HardDeleted,
}

impl LifecycleState {
    /// Canonical lowercase string representation (DB storage format).
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Archived => "archived",
            Self::SoftDeleted => "soft_deleted",
            Self::HardDeleted => "hard_deleted",
        }
    }

    /// Parse from lowercase string (returns `None` for unknown values).
    #[must_use]
    pub fn from_str_value(s: &str) -> Option<Self> {
        match s {
            "active" => Some(Self::Active),
            "archived" => Some(Self::Archived),
            "soft_deleted" => Some(Self::SoftDeleted),
            "hard_deleted" => Some(Self::HardDeleted),
            _ => None,
        }
    }

    /// Check whether a transition from `self` to `target` is valid per the
    /// session lifecycle state machine.
    #[must_use]
    pub fn can_transition_to(&self, target: &Self) -> bool {
        matches!(
            (self, target),
            (Self::Active, Self::Archived)
                | (Self::Active, Self::SoftDeleted)
                | (Self::Active, Self::HardDeleted)
                | (Self::Archived, Self::Active)
                | (Self::Archived, Self::SoftDeleted)
                | (Self::Archived, Self::HardDeleted)
                | (Self::SoftDeleted, Self::Active)
                | (Self::SoftDeleted, Self::HardDeleted)
        )
    }
}

impl std::fmt::Display for LifecycleState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionType {
    pub session_type_id: Uuid,
    pub name: String,
    pub plugin_instance_id: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub message_id: Uuid,
    pub session_id: Uuid,
    pub parent_message_id: Option<Uuid>,
    #[serde(default)]
    pub variant_index: i32,
    #[serde(default)]
    pub is_active: bool,
    pub role: MessageRole,
    pub content: serde_json::Value,
    #[serde(default)]
    pub file_ids: Vec<Uuid>,
    pub metadata: Option<serde_json::Value>,
    #[serde(default = "default_true")]
    pub is_complete: bool,
    #[serde(default)]
    pub is_hidden_from_user: bool,
    #[serde(default)]
    pub is_hidden_from_backend: bool,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    User,
    Assistant,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capability {
    pub name: String,
    pub value: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityValue {
    pub name: String,
    pub value: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariantInfo {
    pub message_id: Uuid,
    pub variant_index: i32,
    pub total_variants: i32,
    pub is_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MemoryStrategy {
    Full,
    SlidingWindow { window_size: u32 },
    Summarized { recent_messages_to_keep: u32 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RetentionPolicy {
    None,
    AgeBased { max_age_days: u32 },
    CountBased { max_message_count: u32 },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HealthStatus {
    Healthy,
    Degraded,
    Unhealthy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamingEvent {
    Start(StreamingStartEvent),
    Chunk(StreamingChunkEvent),
    Complete(StreamingCompleteEvent),
    Error(StreamingErrorEvent),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamingStartEvent {
    pub message_id: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamingChunkEvent {
    pub message_id: Uuid,
    pub chunk: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamingCompleteEvent {
    pub message_id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamingErrorEvent {
    pub message_id: Uuid,
    pub error: String,
}
