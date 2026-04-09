use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use axum::extract::{Json, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use futures_util::StreamExt;
use notification_sdk::types::*;
use notification_sdk::{NotificationApi, NotificationError, NotificationBackend};

// ═══════════════════════════════════════════════════════════════════
// NotificationModule
//
// Implements NotificationApi (Provided Interface) by delegating
// the actual delivery to a NotificationBackend (Extension Point / Port).
//
//   Consumer ──API──▶ NotificationModule ──Extension Point──▶ Plugin
//   (poc-host)        (this module)                          (email or remote)
// ═══════════════════════════════════════════════════════════════════

#[derive(Clone)]
pub struct NotificationModule {
    backend: Arc<dyn NotificationBackend>,
    statuses: Arc<RwLock<HashMap<String, NotificationStatus>>>,
}

impl NotificationModule {
    pub fn new(backend: Arc<dyn NotificationBackend>) -> Self {
        Self {
            backend,
            statuses: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Build an axum router for the API endpoints.
    pub fn router(self) -> Router {
        Router::new()
            .route("/v1/send", post(api_send))
            .route("/v1/get_status", post(api_get_status))
            .route("/.well-known/openapi.json", get(api_openapi))
            .with_state(self)
    }

    /// Interactive demo exercising the SPI directly.
    pub async fn demo(&self) {
        println!("  ┌─ Backend calls (module → plugin) ──────────────┐");

        print!("  │ backend.deliver(\"user@co.com\", \"Hi\") → ");
        match self.backend.deliver(&DeliverRequest::new("user@co.com", "Hi")).await {
            Ok(resp) => println!("id={}, accepted={}", resp.delivery_id, resp.accepted),
            Err(e) => println!("Error: {e}"),
        }

        print!("  │ backend.stream_delivery(...)         → ");
        match self.backend.stream_delivery(&DeliverRequest::new("user@co.com", "Hi")).await {
            Ok(stream) => {
                let events: Vec<_> = stream.collect().await;
                let stages: Vec<&str> = events.iter().map(|e| match e {
                    DeliveryEvent::Started { .. } => "started",
                    DeliveryEvent::Progress { stage, .. } => stage.as_str(),
                    DeliveryEvent::Delivered { .. } => "delivered",
                    DeliveryEvent::Failed { .. } => "failed",
                }).collect();
                println!("[{}] ({} events)", stages.join(" → "), events.len());
            }
            Err(e) => println!("Error: {e}"),
        }

        print!("  │ backend.deliver(\"\", \"Hi\")             → ");
        match self.backend.deliver(&DeliverRequest::new("", "Hi")).await {
            Ok(resp) => println!("id={}", resp.delivery_id),
            Err(e) => println!("Error: {e}"),
        }

        println!("  └─────────────────────────────────────────────────┘");
    }
}

// ── NotificationApi implementation ─────────────────────────────

#[async_trait]
impl NotificationApi for NotificationModule {
    async fn send(
        &self,
        req: &SendNotificationRequest,
    ) -> Result<SendNotificationResponse, NotificationError> {
        let recipient = format!("{}@{}", req.user_id, match req.channel {
            Channel::Email => "email.com",
            Channel::Sms => "sms.gateway",
            Channel::Push => "push.service",
        });

        let delivery = self.backend.deliver(&DeliverRequest::new(&recipient, &req.message)).await?;
        let notification_id = format!("ntf-{}", &delivery.delivery_id);

        let status = NotificationStatus::new(
            &notification_id,
            if delivery.accepted { DeliveryState::Pending } else { DeliveryState::Failed },
        );
        self.statuses.write().unwrap().insert(notification_id.clone(), status);

        Ok(SendNotificationResponse::new(notification_id))
    }

    async fn get_status(
        &self,
        req: &GetStatusRequest,
    ) -> Result<NotificationStatus, NotificationError> {
        self.statuses
            .read()
            .unwrap()
            .get(&req.notification_id)
            .cloned()
            .ok_or(NotificationError::NotificationNotFound {
                notification_id: req.notification_id.clone(),
            })
    }
}

// ── REST API handlers ──────────────────────────────────────────

async fn api_send(State(module): State<NotificationModule>, Json(req): Json<SendNotificationRequest>) -> Response {
    match module.send(&req).await {
        Ok(resp) => (StatusCode::OK, Json(resp)).into_response(),
        Err(e) => error_response(&e),
    }
}

async fn api_get_status(State(module): State<NotificationModule>, Json(req): Json<GetStatusRequest>) -> Response {
    match module.get_status(&req).await {
        Ok(resp) => (StatusCode::OK, Json(resp)).into_response(),
        Err(e) => error_response(&e),
    }
}

async fn api_openapi() -> Json<serde_json::Value> {
    Json(notification_sdk::api::notification_api_rest_openapi_spec())
}

fn error_response(e: &NotificationError) -> Response {
    let pd = e.to_problem_details();
    let status = StatusCode::from_u16(pd.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (status, [(axum::http::header::CONTENT_TYPE, "application/problem+json")], Json(pd)).into_response()
}
