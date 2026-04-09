use modkit_contract_macros::ContractError;

#[derive(Debug, Clone, ContractError)]
#[contract_error(domain = "poc.notification")]
pub enum NotificationError {
    #[error(status = 404, problem_type = "not-found")]
    NotificationNotFound { notification_id: String },

    #[error(status = 400, problem_type = "invalid-argument")]
    InvalidRecipient { reason: String },

    #[error(status = 503, problem_type = "service-unavailable")]
    DeliveryUnavailable { channel: String, retry_after_seconds: Option<u64> },

    #[error(status = 500, problem_type = "internal")]
    Internal { description: String },
}

impl NotificationError {
    #[doc(hidden)]
    pub fn __contract_error_fallback(msg: &str) -> Self {
        Self::Internal {
            description: msg.to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_found_round_trip() {
        let err = NotificationError::NotificationNotFound {
            notification_id: "ntf-123".into(),
        };
        let pd = err.to_problem_details();
        assert_eq!(pd.error_code, "NOTIFICATION_NOT_FOUND");
        assert_eq!(pd.error_domain, "poc.notification");
        assert_eq!(pd.status, 404);

        let json = serde_json::to_string(&pd).unwrap();
        let pd2: modkit_contract_runtime::ProblemDetails =
            serde_json::from_str(&json).unwrap();
        let err2 = NotificationError::from_problem_details(&pd2);
        match err2 {
            NotificationError::NotificationNotFound { notification_id } => {
                assert_eq!(notification_id, "ntf-123");
            }
            other => panic!("Expected NotificationNotFound, got: {other:?}"),
        }
    }

    #[test]
    fn delivery_unavailable_round_trip() {
        let err = NotificationError::DeliveryUnavailable {
            channel: "sms".into(),
            retry_after_seconds: Some(30),
        };
        let pd = err.to_problem_details();
        assert_eq!(pd.error_code, "DELIVERY_UNAVAILABLE");

        let json = serde_json::to_string(&pd).unwrap();
        let pd2: modkit_contract_runtime::ProblemDetails =
            serde_json::from_str(&json).unwrap();
        let err2 = NotificationError::from_problem_details(&pd2);
        match err2 {
            NotificationError::DeliveryUnavailable { channel, retry_after_seconds } => {
                assert_eq!(channel, "sms");
                assert_eq!(retry_after_seconds, Some(30));
            }
            other => panic!("Expected DeliveryUnavailable, got: {other:?}"),
        }
    }
}
