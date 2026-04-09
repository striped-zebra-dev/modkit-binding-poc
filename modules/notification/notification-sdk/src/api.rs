use std::pin::Pin;

use async_trait::async_trait;
use futures_core::Stream;
use modkit_contract_macros::{modkit_rest_contract, post, retryable, streaming};

use crate::error::NotificationError;
use crate::types::*;

// ═══════════════════════════════════════════════════════════════════
//
//                        NAMING CONVENTION
//
// Module contracts follow a naming convention that encodes
// what the trait is and how it can be bound:
//
//   ┌──────────────┬────────────────────┬──────────────────────┐
//   │              │ Always local       │ Can be remote        │
//   ├──────────────┼────────────────────┼──────────────────────┤
//   │ Provided     │ {Module}Api        │ {Module}ApiRest      │
//   │ (module      │ NotificationApi    │ NotificationApiRest  │
//   │  serves)     │                    │                      │
//   ├──────────────┼────────────────────┼──────────────────────┤
//   │ Required     │ {Module}Extension  │ {Module}Backend      │
//   │ (plugin      │ NotificationFmt    │ NotificationBackend  │
//   │  serves)     │ (compile-only,     │ NotificationBackend- │
//   │              │  no REST option)   │   Rest               │
//   └──────────────┴────────────────────┴──────────────────────┘
//
// Api       — the module IS the service. Consumers call it.
// ApiRest   — REST projection of the API. Macro generates a client.
//
// Extension — the module NEEDS a compile-time plugin. Always local.
//             No REST projection, no macro, just a plain trait.
//             For performance-critical hooks (transforms, formatting).
//
// Backend     — the module NEEDS a plugin that MAY be remote.
//              Base trait is clean, no annotations.
// BackendRest — REST projection. Macro generates client + OpenAPI spec.
//
// The base trait (Api, Extension, Backend) is always a plain Rust
// trait. Only the *Rest traits carry transport annotations.
// Consumers always depend on the base trait.
//
// ═══════════════════════════════════════════════════════════════════

// ─── API ─────────────────────────────────────────────────────────
// Provided Interface — the module IS the service.
// Direction: Module → Consumer ("I serve, you consume")

/// Base API trait — clean, no annotations.
#[async_trait]
pub trait NotificationApi: Send + Sync {
    /// Send a notification to a user on a given channel.
    /// Returns a notification ID for status tracking.
    async fn send(
        &self,
        req: &SendNotificationRequest,
    ) -> Result<SendNotificationResponse, NotificationError>;

    /// Get the delivery status of a previously sent notification.
    async fn get_status(
        &self,
        req: &GetStatusRequest,
    ) -> Result<NotificationStatus, NotificationError>;
}

/// REST projection of NotificationApi.
#[modkit_rest_contract]
#[async_trait]
pub trait NotificationApiRest: NotificationApi {
    #[retryable]
    #[post("/v1/send")]
    async fn send(
        &self,
        req: &SendNotificationRequest,
    ) -> Result<SendNotificationResponse, NotificationError>;

    #[retryable]
    #[post("/v1/get_status")]
    async fn get_status(
        &self,
        req: &GetStatusRequest,
    ) -> Result<NotificationStatus, NotificationError>;
}

// ─── BACKEND ─────────────────────────────────────────────────────
// Required Interface that MAY be remote.
// Plugins provide the implementation — compile-time or REST.
// Direction: Plugin → Module ("You implement, I call you")

/// Base Backend trait — clean, no annotations.
#[async_trait]
pub trait NotificationBackend: Send + Sync {
    /// Deliver a message to a recipient. This is the raw delivery —
    /// the module's API layer handles channel selection, validation,
    /// and status tracking on top.
    async fn deliver(
        &self,
        req: &DeliverRequest,
    ) -> Result<DeliverResponse, NotificationError>;

    /// Stream delivery progress events (SSE).
    /// Plugins emit events as the delivery progresses through stages
    /// (e.g., queued → sending → delivered or failed).
    async fn stream_delivery(
        &self,
        req: &DeliverRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = DeliveryEvent> + Send>>, NotificationError>;
}

/// REST projection of NotificationBackend.
#[modkit_rest_contract]
#[async_trait]
pub trait NotificationBackendRest: NotificationBackend {
    #[retryable]
    #[post("/v1/deliver")]
    async fn deliver(
        &self,
        req: &DeliverRequest,
    ) -> Result<DeliverResponse, NotificationError>;

    #[streaming]
    #[post("/v1/delivery/stream")]
    async fn stream_delivery(
        &self,
        req: &DeliverRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = DeliveryEvent> + Send>>, NotificationError>;
}

// ─── EXTENSION ───────────────────────────────────────────────────
// Required Interface that is ALWAYS local (compile-time only).
// No REST projection, no macro — just a plain Rust trait.
// For performance-critical hooks where remote calls are unacceptable.
// Direction: Plugin → Module ("You implement, I call you")

/// Notification formatter — an Extension (compile-time only).
///
/// The module calls this hook to format messages before delivery.
/// Always in-process — no REST projection, no network overhead.
///
/// Example of a performance-critical extension point where
/// remote binding is not appropriate.
#[async_trait]
pub trait NotificationFormatter: Send + Sync {
    /// Format a message for a specific channel.
    /// Called synchronously in the send() path — must be fast.
    async fn format(
        &self,
        message: &str,
        channel: &Channel,
    ) -> Result<String, NotificationError>;
}
