use std::convert::Infallible;
use std::time::Duration;

use axum::extract::Json;
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use futures_util::stream;
use modkit_contract_runtime::ProblemDetails;
use notification_sdk::types::{DeliverRequest, DeliverResponse, DeliveryEvent};

/// Remote plugin for the NotificationBackend extension point.
/// Simulates a third-party SMS delivery gateway.
///
/// This is a standalone REST service that implements the NotificationBackend
/// contract. The notification module discovers it via the directory
/// and calls it through the generated NotificationBackendRestClient.

#[tokio::main]
async fn main() {
    let port = std::env::var("PORT").unwrap_or_else(|_| "3001".to_string());
    let addr = format!("0.0.0.0:{port}");

    let app = Router::new()
        .route("/v1/deliver", post(deliver_handler))
        .route("/v1/delivery/stream", post(stream_delivery_handler))
        .route("/.well-known/openapi.json", get(openapi_handler));

    println!("notification-plugin-remote (SMS gateway) listening on {addr}");
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn deliver_handler(Json(req): Json<DeliverRequest>) -> Response {
    if req.recipient.is_empty() {
        return problem_response(
            StatusCode::BAD_REQUEST,
            "INVALID_RECIPIENT",
            "Recipient phone number cannot be empty",
            serde_json::json!({ "reason": "Recipient phone number cannot be empty" }),
        );
    }

    // Simulate SMS sending
    tokio::time::sleep(Duration::from_millis(50)).await;

    let resp = DeliverResponse::new(format!("sms-{}", timestamp_id()), true);
    (StatusCode::OK, Json(resp)).into_response()
}

async fn stream_delivery_handler(Json(req): Json<DeliverRequest>) -> Response {
    if req.recipient.is_empty() {
        return problem_response(
            StatusCode::BAD_REQUEST,
            "INVALID_RECIPIENT",
            "Recipient phone number cannot be empty",
            serde_json::json!({ "reason": "Recipient phone number cannot be empty" }),
        );
    }

    let recipient = req.recipient.clone();
    let mut event_id: u64 = 0;

    let event_stream = stream::unfold(SmsState::Start { recipient }, move |state| {
        async move {
            match state {
                SmsState::Start { recipient } => {
                    event_id += 1;
                    let delivery_id = format!("sms-{event_id}");
                    let evt = DeliveryEvent::Started { delivery_id: delivery_id.clone() };
                    let sse = Event::default()
                        .event("delivery_started")
                        .id(event_id.to_string())
                        .json_data(&evt).unwrap();
                    Some((Ok::<_, Infallible>(sse), SmsState::Sending { delivery_id, recipient, event_id }))
                }
                SmsState::Sending { delivery_id, recipient, mut event_id } => {
                    tokio::time::sleep(Duration::from_millis(40)).await;
                    event_id += 1;
                    let evt = DeliveryEvent::Progress {
                        stage: "queued".into(),
                        detail: format!("SMS queued for {recipient}"),
                    };
                    let sse = Event::default()
                        .event("progress")
                        .id(event_id.to_string())
                        .json_data(&evt).unwrap();
                    Some((Ok(sse), SmsState::Delivering { delivery_id, event_id }))
                }
                SmsState::Delivering { delivery_id, mut event_id } => {
                    tokio::time::sleep(Duration::from_millis(40)).await;
                    event_id += 1;
                    let evt = DeliveryEvent::Delivered { delivery_id };
                    let sse = Event::default()
                        .event("delivered")
                        .id(event_id.to_string())
                        .json_data(&evt).unwrap();
                    Some((Ok(sse), SmsState::Done))
                }
                SmsState::Done => None,
            }
        }
    });

    Sse::new(event_stream)
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
        .into_response()
}

enum SmsState {
    Start { recipient: String },
    Sending { delivery_id: String, recipient: String, event_id: u64 },
    Delivering { delivery_id: String, event_id: u64 },
    Done,
}

async fn openapi_handler() -> Json<serde_json::Value> {
    Json(notification_sdk::api::notification_backend_rest_openapi_spec())
}

fn problem_response(
    status: StatusCode,
    error_code: &str,
    detail: &str,
    context: serde_json::Value,
) -> Response {
    let problem_type = match status {
        StatusCode::BAD_REQUEST => "https://errors.cyberfabric.io/invalid-argument",
        StatusCode::NOT_FOUND => "https://errors.cyberfabric.io/not-found",
        StatusCode::SERVICE_UNAVAILABLE => "https://errors.cyberfabric.io/service-unavailable",
        _ => "https://errors.cyberfabric.io/internal",
    };
    let title = match status {
        StatusCode::BAD_REQUEST => "Invalid Argument",
        StatusCode::NOT_FOUND => "Not Found",
        StatusCode::SERVICE_UNAVAILABLE => "Service Unavailable",
        _ => "Internal",
    };
    let pd = ProblemDetails {
        problem_type: problem_type.to_owned(),
        title: title.to_owned(),
        status: status.as_u16(),
        detail: detail.to_owned(),
        error_code: error_code.to_owned(),
        error_domain: "poc.notification".to_owned(),
        context,
        trace_id: None,
    };
    (status, [(axum::http::header::CONTENT_TYPE, "application/problem+json")], Json(pd)).into_response()
}

fn timestamp_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        .to_string()
}
