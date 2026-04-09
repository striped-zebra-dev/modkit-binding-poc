pub mod api;
pub mod error;
pub mod types;

pub use api::{NotificationApi, NotificationBackend, NotificationFormatter};
pub use error::NotificationError;
pub use types::*;

#[cfg(feature = "rest-client")]
pub use api::{NotificationApiRestClient, NotificationBackendRestClient};
