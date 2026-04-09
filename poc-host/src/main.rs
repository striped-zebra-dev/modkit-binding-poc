use std::sync::Arc;
use std::time::Duration;

use modkit_directory::{ClientConfig, RetryConfig, ServiceDirectory};
use notification::NotificationModule;
use notification_plugin_email::EmailDeliveryPlugin;
use notification_sdk::{
    Channel, GetStatusRequest, NotificationApi, NotificationApiRestClient,
    NotificationBackend, NotificationBackendRestClient, SendNotificationRequest,
};

#[tokio::main]
async fn main() {
    let mode = std::env::var("BINDING_MODE").unwrap_or_else(|_| "compile".into());

    // ── Service Directory ──────────────────────────────────────
    let directory = ServiceDirectory::new();

    if let Ok(url) = std::env::var("NOTIFICATION_SPI_URL") {
        let config = ClientConfig::new(&url)
            .with_timeout(Duration::from_secs(10))
            .with_retry(RetryConfig {
                max_retries: 3,
                base_delay: Duration::from_millis(100),
                max_delay: Duration::from_secs(2),
            });
        match directory
            .register_and_validate(
                "gts.poc.notification.backend.v1~remote.v1~",
                config,
                &notification_sdk::api::notification_backend_rest_openapi_spec(),
            )
            .await
        {
            Ok(()) => println!("[directory] NotificationBackend: registered + spec validated"),
            Err(e) => println!("[directory] NotificationBackend: validation failed — {e}"),
        }
    }

    if let Ok(url) = std::env::var("NOTIFICATION_API_URL") {
        let config = ClientConfig::new(&url)
            .with_timeout(Duration::from_secs(5))
            .with_retry(RetryConfig {
                max_retries: 2,
                base_delay: Duration::from_millis(200),
                max_delay: Duration::from_secs(1),
            });
        match directory
            .register_and_validate(
                "gts.poc.notification.api.v1~remote.v1~",
                config,
                &notification_sdk::api::notification_api_rest_openapi_spec(),
            )
            .await
        {
            Ok(()) => println!("[directory] NotificationApi: registered + spec validated"),
            Err(e) => println!("[directory] NotificationApi: validation failed — {e}"),
        }
    }

    println!();

    // ═══════════════════════════════════════════════════════════
    // Extension Point (Port / Provider Interface)
    //
    // The notification module NEEDS a delivery plugin.
    // ═══════════════════════════════════════════════════════════

    println!("╔══════════════════════════════════════════════════════════╗");
    println!("║  Extension Point (Port / Provider Interface)            ║");
    println!("║  Notification module needs a delivery plugin            ║");
    println!("╚══════════════════════════════════════════════════════════╝\n");

    let backend: Arc<dyn NotificationBackend> = match mode.as_str() {
        "compile" => {
            println!("[backend] Binding: compile-time email plugin");
            Arc::new(EmailDeliveryPlugin::new())
        }
        "rest" => {
            let config = directory
                .resolve("gts.poc.notification.backend.v1~")
                .expect("No NotificationBackend in directory. Set NOTIFICATION_SPI_URL.");
            println!("[backend] Binding: REST → {}", config.base_url);
            Arc::new(NotificationBackendRestClient::from_config(config))
        }
        other => {
            eprintln!("Unknown BINDING_MODE: {other}");
            std::process::exit(1);
        }
    };

    let module = NotificationModule::new(backend);
    println!();
    module.demo().await;

    // ═══════════════════════════════════════════════════════════
    // API: Provider / Provided Interface
    //
    // Show BOTH binding modes for the API:
    //   1. In-process: module used directly as Arc<dyn NotificationApi>
    //   2. REST: module called via generated NotificationApiRestClient
    //
    // The consumer function is IDENTICAL in both cases.
    // ═══════════════════════════════════════════════════════════

    // ── API: In-process (compile-time) ─────────────────────────
    println!("\n╔══════════════════════════════════════════════════════════╗");
    println!("║  API: In-process binding (zero-cost, same binary)      ║");
    println!("║  Consumer uses Arc<dyn NotificationApi> directly       ║");
    println!("║  Chain: Consumer → Module → SPI → Plugin               ║");
    println!("╚══════════════════════════════════════════════════════════╝\n");

    let api_local: Arc<dyn NotificationApi> = Arc::new(module.clone());
    println!("[api] Binding: in-process (Arc<dyn NotificationApi>)\n");
    consume_notification_api(&api_local).await;

    // ── API: REST (out-of-process) ─────────────────────────────
    if let Some(config) = directory.resolve("gts.poc.notification.api.v1~") {
        println!("\n╔══════════════════════════════════════════════════════════╗");
        println!("║  API: REST binding (cross-process)                     ║");
        println!("║  Consumer uses generated NotificationApiRestClient     ║");
        println!("║  Chain: Consumer → REST → Module → SPI → Plugin       ║");
        println!("╚══════════════════════════════════════════════════════════╝\n");

        println!("[api] Binding: REST → {}\n", config.base_url);
        let api_remote: Arc<dyn NotificationApi> =
            Arc::new(NotificationApiRestClient::from_config(config));
        consume_notification_api(&api_remote).await;
    }

    println!("\n═══ Demo Complete ═══");
}

/// Consumer function — uses NotificationApi without knowing
/// whether it's in-process or behind a REST proxy.
/// **This function is identical for both binding modes.**
async fn consume_notification_api(api: &Arc<dyn NotificationApi>) {
    println!("  ┌─ API calls (consumer → module → plugin) ──────┐");

    print!("  │ api.send(user1, \"Hello!\", Email)  → ");
    match api
        .send(&SendNotificationRequest::new("user1", "Hello!", Channel::Email))
        .await
    {
        Ok(resp) => {
            println!("id={}", resp.notification_id);
            print!("  │ api.get_status({})    → ", &resp.notification_id);
            match api
                .get_status(&GetStatusRequest::new(&resp.notification_id))
                .await
            {
                Ok(status) => println!("{:?}", status.state),
                Err(e) => println!("Error: {e}"),
            }
        }
        Err(e) => println!("Error: {e}"),
    }

    print!("  │ api.send(user2, \"Hey!\", Sms)     → ");
    match api
        .send(&SendNotificationRequest::new("user2", "Hey!", Channel::Sms))
        .await
    {
        Ok(resp) => println!("id={}", resp.notification_id),
        Err(e) => println!("Error: {e}"),
    }

    print!("  │ api.get_status(\"ntf-999\")         → ");
    match api.get_status(&GetStatusRequest::new("ntf-999")).await {
        Ok(status) => println!("{:?}", status.state),
        Err(e) => println!("Error: {e}"),
    }

    println!("  └─────────────────────────────────────────────────┘");
}
