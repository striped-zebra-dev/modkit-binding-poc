fn main() {
    println!("=== NotificationApi (Provided Interface) ===");
    let api_spec = notification_sdk::api::notification_api_rest_openapi_spec();
    println!("{}", serde_json::to_string_pretty(&api_spec).unwrap());

    println!("\n=== NotificationBackend (Extension Point) ===");
    let backend_spec = notification_sdk::api::notification_backend_rest_openapi_spec();
    println!("{}", serde_json::to_string_pretty(&backend_spec).unwrap());
}
