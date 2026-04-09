use std::pin::Pin;

use async_trait::async_trait;
use futures_core::Stream;
use notification_sdk::{
    DeliverRequest, DeliverResponse, DeliveryEvent, NotificationError, NotificationBackend,
};

/// Compile-time plugin for the NotificationBackend extension point.
/// Provides email delivery.
///
/// In a real platform, this would connect to an SMTP server.
/// For the PoC, it simulates email delivery with delays.
pub struct EmailDeliveryPlugin;

impl EmailDeliveryPlugin {
    pub fn new() -> Self {
        Self
    }
}

impl Default for EmailDeliveryPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl NotificationBackend for EmailDeliveryPlugin {
    async fn deliver(
        &self,
        req: &DeliverRequest,
    ) -> Result<DeliverResponse, NotificationError> {
        if req.recipient.is_empty() {
            return Err(NotificationError::InvalidRecipient {
                reason: "Recipient email cannot be empty".into(),
            });
        }

        // Simulate email sending
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let delivery_id = format!("email-{}", timestamp_id());
        Ok(DeliverResponse::new(delivery_id, true))
    }

    async fn stream_delivery(
        &self,
        req: &DeliverRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = DeliveryEvent> + Send>>, NotificationError> {
        if req.recipient.is_empty() {
            return Err(NotificationError::InvalidRecipient {
                reason: "Recipient email cannot be empty".into(),
            });
        }

        let recipient = req.recipient.clone();
        let (tx, rx) = tokio::sync::mpsc::channel(16);

        tokio::spawn(async move {
            let delivery_id = format!("email-{}", timestamp_id());

            let _ = tx.send(DeliveryEvent::Started {
                delivery_id: delivery_id.clone(),
            }).await;

            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
            let _ = tx.send(DeliveryEvent::Progress {
                stage: "resolving".into(),
                detail: format!("Looking up MX record for {recipient}"),
            }).await;

            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
            let _ = tx.send(DeliveryEvent::Progress {
                stage: "sending".into(),
                detail: "SMTP handshake complete, sending body".into(),
            }).await;

            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
            let _ = tx.send(DeliveryEvent::Delivered {
                delivery_id,
            }).await;
        });

        Ok(Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx)))
    }
}

fn timestamp_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{ts:x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio_stream::StreamExt;

    #[tokio::test]
    async fn deliver_returns_delivery_id() {
        let plugin = EmailDeliveryPlugin::new();
        let resp = plugin
            .deliver(&DeliverRequest::new("user@example.com", "Hello"))
            .await
            .unwrap();
        assert!(resp.delivery_id.starts_with("email-"));
        assert!(resp.accepted);
    }

    #[tokio::test]
    async fn deliver_rejects_empty_recipient() {
        let plugin = EmailDeliveryPlugin::new();
        let err = plugin
            .deliver(&DeliverRequest::new("", "Hello"))
            .await
            .unwrap_err();
        assert!(matches!(err, NotificationError::InvalidRecipient { .. }));
    }

    #[tokio::test]
    async fn stream_emits_started_progress_delivered() {
        let plugin = EmailDeliveryPlugin::new();
        let stream = plugin
            .stream_delivery(&DeliverRequest::new("user@example.com", "Hello"))
            .await
            .unwrap();

        let events: Vec<_> = stream.collect().await;
        assert!(matches!(&events[0], DeliveryEvent::Started { .. }));
        assert!(events.iter().any(|e| matches!(e, DeliveryEvent::Progress { .. })));
        assert!(matches!(events.last().unwrap(), DeliveryEvent::Delivered { .. }));
    }
}
