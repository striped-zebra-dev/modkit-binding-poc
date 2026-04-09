//! Integration test — mocking NotificationBackend with mockall.
//!
//! `#[automock]` on the trait generates a MockNotificationBackend struct
//! with `expect_*` methods for setting up behaviors. This shows that
//! `async_trait` + trait-object-compatible base traits work with
//! mockall's derive-based mock generation.

use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use futures_core::Stream;
use mockall::mock;
use notification::NotificationModule;
use notification_sdk::types::*;
use notification_sdk::{NotificationApi, NotificationBackend, NotificationError};

// mockall's automock has limitations with Pin<Box<dyn Stream>> returns.
// Use the `mock!` macro instead for full control.
mock! {
    pub Backend {}

    #[async_trait]
    impl NotificationBackend for Backend {
        async fn deliver(
            &self,
            req: &DeliverRequest,
        ) -> Result<DeliverResponse, NotificationError>;

        async fn stream_delivery(
            &self,
            req: &DeliverRequest,
        ) -> Result<Pin<Box<dyn Stream<Item = DeliveryEvent> + Send>>, NotificationError>;
    }
}

#[tokio::test]
async fn send_via_mockall_mock() {
    let mut mock = MockBackend::new();

    mock.expect_deliver()
        .times(1)
        .withf(|req: &DeliverRequest| {
            req.recipient == "user-7@email.com" && req.message == "Ping"
        })
        .returning(|_| Ok(DeliverResponse::new("dlv-mockall", true)));

    let module = NotificationModule::new(Arc::new(mock));

    let resp = module
        .send(&SendNotificationRequest::new(
            "user-7",
            "Ping",
            Channel::Email,
        ))
        .await
        .expect("send should succeed");

    assert!(resp.notification_id.contains("dlv-mockall"));

    // Calling get_status on the stored notification
    let status = module
        .get_status(&GetStatusRequest::new(&resp.notification_id))
        .await
        .expect("status should be tracked");
    assert!(matches!(status.state, DeliveryState::Pending));
}

#[tokio::test]
async fn get_status_not_found() {
    let mock = MockBackend::new();
    let module = NotificationModule::new(Arc::new(mock));

    let err = module
        .get_status(&GetStatusRequest::new("ntf-does-not-exist"))
        .await
        .expect_err("should return not found");

    assert!(matches!(err, NotificationError::NotificationNotFound { .. }));
}

#[tokio::test]
async fn send_failure_is_tracked_as_failed() {
    let mut mock = MockBackend::new();

    mock.expect_deliver()
        .times(1)
        .returning(|_| Ok(DeliverResponse::new("dlv-rejected", false)));

    let module = NotificationModule::new(Arc::new(mock));

    let resp = module
        .send(&SendNotificationRequest::new(
            "user-8",
            "Hey",
            Channel::Push,
        ))
        .await
        .expect("send should return ok even if backend rejects");

    let status = module
        .get_status(&GetStatusRequest::new(&resp.notification_id))
        .await
        .expect("status should be tracked");

    // accepted=false maps to Failed state per NotificationModule::send logic
    assert!(matches!(status.state, DeliveryState::Failed));
}
