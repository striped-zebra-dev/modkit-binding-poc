use std::sync::Arc;

use notification::NotificationModule;
use notification_plugin_email::EmailDeliveryPlugin;

/// Run the notification module as a standalone REST API server.
///
/// Uses the compile-time email plugin internally.
/// Exposes NotificationApi endpoints for external consumers.
#[tokio::main]
async fn main() {
    let port = std::env::var("PORT").unwrap_or_else(|_| "3010".to_string());
    let addr = format!("0.0.0.0:{port}");

    let backend = Arc::new(EmailDeliveryPlugin::new());
    let module = NotificationModule::new(backend);
    let app = module.router();

    println!("notification API listening on {addr}");
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
