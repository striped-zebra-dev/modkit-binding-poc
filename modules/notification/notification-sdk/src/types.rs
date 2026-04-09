use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// ═══════════════════════════════════════════════════════════════════
// API types — used by consumers calling the Notification module
// ═══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[non_exhaustive]
pub struct SendNotificationRequest {
    pub user_id: String,
    pub message: String,
    pub channel: Channel,
}

impl SendNotificationRequest {
    pub fn new(
        user_id: impl Into<String>,
        message: impl Into<String>,
        channel: Channel,
    ) -> Self {
        Self {
            user_id: user_id.into(),
            message: message.into(),
            channel,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub enum Channel {
    Email,
    Sms,
    Push,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[non_exhaustive]
pub struct SendNotificationResponse {
    pub notification_id: String,
}

impl SendNotificationResponse {
    pub fn new(notification_id: impl Into<String>) -> Self {
        Self {
            notification_id: notification_id.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[non_exhaustive]
pub struct GetStatusRequest {
    pub notification_id: String,
}

impl GetStatusRequest {
    pub fn new(notification_id: impl Into<String>) -> Self {
        Self {
            notification_id: notification_id.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[non_exhaustive]
pub struct NotificationStatus {
    pub notification_id: String,
    pub state: DeliveryState,
}

impl NotificationStatus {
    pub fn new(notification_id: impl Into<String>, state: DeliveryState) -> Self {
        Self {
            notification_id: notification_id.into(),
            state,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub enum DeliveryState {
    Pending,
    Delivered,
    Failed,
}

// ═══════════════════════════════════════════════════════════════════
// SPI types — used by delivery plugins implementing the backend
// ═══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[non_exhaustive]
pub struct DeliverRequest {
    pub recipient: String,
    pub message: String,
}

impl DeliverRequest {
    pub fn new(recipient: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            recipient: recipient.into(),
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[non_exhaustive]
pub struct DeliverResponse {
    pub delivery_id: String,
    pub accepted: bool,
}

impl DeliverResponse {
    pub fn new(delivery_id: impl Into<String>, accepted: bool) -> Self {
        Self {
            delivery_id: delivery_id.into(),
            accepted,
        }
    }
}

/// Events emitted during delivery streaming (SSE).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type")]
pub enum DeliveryEvent {
    #[serde(rename = "delivery_started")]
    Started { delivery_id: String },

    #[serde(rename = "progress")]
    Progress { stage: String, detail: String },

    #[serde(rename = "delivered")]
    Delivered { delivery_id: String },

    #[serde(rename = "failed")]
    Failed { reason: String },
}
