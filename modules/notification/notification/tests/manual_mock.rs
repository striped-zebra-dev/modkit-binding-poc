//! Integration test — mocking NotificationBackend by hand.
//!
//! Demonstrates that the module can be tested without booting the real
//! delivery plugin or the REST server. The mock point is the BASE trait
//! (NotificationBackend), not the transport projection — proving that
//! consumer code is identical whether backed by real impl, mock, or
//! generated REST client.

use std::pin::Pin;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use futures_core::Stream;
use notification::NotificationModule;
use notification_sdk::types::*;
use notification_sdk::{NotificationApi, NotificationBackend, NotificationError};

/// Hand-rolled mock that records all calls and returns queued responses.
#[derive(Default)]
struct ManualMockBackend {
    calls: Mutex<Vec<DeliverRequest>>,
    responses: Mutex<Vec<Result<DeliverResponse, NotificationError>>>,
}

impl ManualMockBackend {
    fn new() -> Self {
        Self::default()
    }

    fn queue_response(&self, resp: Result<DeliverResponse, NotificationError>) {
        self.responses.lock().unwrap().push(resp);
    }

    fn recorded_calls(&self) -> Vec<DeliverRequest> {
        self.calls.lock().unwrap().clone()
    }
}

#[async_trait]
impl NotificationBackend for ManualMockBackend {
    async fn deliver(
        &self,
        req: &DeliverRequest,
    ) -> Result<DeliverResponse, NotificationError> {
        self.calls.lock().unwrap().push(req.clone());
        let mut queue = self.responses.lock().unwrap();
        if queue.is_empty() {
            return Err(NotificationError::Internal {
                description: "no response queued".into(),
            });
        }
        queue.remove(0)
    }

    async fn stream_delivery(
        &self,
        _req: &DeliverRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = DeliveryEvent> + Send>>, NotificationError> {
        Err(NotificationError::Internal {
            description: "streaming not mocked in this test".into(),
        })
    }
}

#[tokio::test]
async fn send_delegates_to_backend_and_records_status() {
    let mock = Arc::new(ManualMockBackend::new());
    mock.queue_response(Ok(DeliverResponse::new("dlv-42", true)));

    let module = NotificationModule::new(mock.clone());

    let resp = module
        .send(&SendNotificationRequest::new(
            "user-1",
            "Hello",
            Channel::Email,
        ))
        .await
        .expect("send should succeed");

    // Notification ID is derived from delivery ID
    assert!(resp.notification_id.contains("dlv-42"));

    // The mock received exactly one call with the expected recipient
    let calls = mock.recorded_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].recipient, "user-1@email.com");
    assert_eq!(calls[0].message, "Hello");

    // Status is tracked in the module
    let status = module
        .get_status(&GetStatusRequest::new(&resp.notification_id))
        .await
        .expect("get_status should succeed");
    assert!(matches!(status.state, DeliveryState::Pending));
}

#[tokio::test]
async fn send_propagates_backend_error() {
    let mock = Arc::new(ManualMockBackend::new());
    mock.queue_response(Err(NotificationError::InvalidRecipient {
        reason: "empty".into(),
    }));

    let module = NotificationModule::new(mock);

    let err = module
        .send(&SendNotificationRequest::new(
            "user-1",
            "Hi",
            Channel::Sms,
        ))
        .await
        .expect_err("send should propagate backend error");

    assert!(matches!(err, NotificationError::InvalidRecipient { .. }));
}
