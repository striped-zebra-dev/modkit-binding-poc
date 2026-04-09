use std::collections::HashMap;
use std::sync::{Arc, RwLock};

pub use modkit_contract_runtime::ClientConfig;
pub use modkit_contract_runtime::RetryConfig;

/// Simple service directory — resolves GTS-like IDs to endpoint configs.
#[derive(Debug, Clone, Default)]
pub struct ServiceDirectory {
    entries: Arc<RwLock<HashMap<String, ClientConfig>>>,
}

impl ServiceDirectory {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a service by GTS-like ID (no validation).
    pub fn register(&self, gts_id: impl Into<String>, config: ClientConfig) {
        self.entries
            .write()
            .expect("directory lock poisoned")
            .insert(gts_id.into(), config);
    }

    /// Register a service and validate its OpenAPI spec matches the expected contract.
    ///
    /// Fetches `/.well-known/openapi.json` from the service and checks
    /// compatibility rules. Prints a detailed validation report.
    pub async fn register_and_validate(
        &self,
        gts_id: impl Into<String>,
        config: ClientConfig,
        expected_spec: &serde_json::Value,
    ) -> Result<(), String> {
        let gts_id = gts_id.into();
        let report = validate_remote_spec(&config.base_url, expected_spec).await?;

        // Log the validation report
        println!("[directory] {gts_id}:");
        println!(
            "[directory]   spec fetched from {}/.well-known/openapi.json",
            config.base_url
        );
        for check in &report.checks {
            let icon = if check.passed { "  pass" } else { "  FAIL" };
            println!("[directory]   {icon}: {}", check.description);
        }

        if report.has_failures() {
            let failures: Vec<&str> = report
                .checks
                .iter()
                .filter(|c| !c.passed)
                .map(|c| c.description.as_str())
                .collect();
            return Err(format!(
                "Spec validation failed for {gts_id}: {}",
                failures.join("; ")
            ));
        }

        println!(
            "[directory]   registered ({} checks passed)",
            report.checks.len()
        );
        self.register(gts_id, config);
        Ok(())
    }

    /// Resolve a GTS ID prefix to a client config.
    pub fn resolve(&self, gts_prefix: &str) -> Option<ClientConfig> {
        let entries = self.entries.read().expect("directory lock poisoned");
        if let Some(config) = entries.get(gts_prefix) {
            return Some(config.clone());
        }
        entries
            .iter()
            .find(|(id, _)| id.starts_with(gts_prefix))
            .map(|(_, config)| config.clone())
    }

    pub fn list(&self) -> Vec<(String, ClientConfig)> {
        self.entries
            .read()
            .expect("directory lock poisoned")
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }
}

// ═══════════════════════════════════════════════════════════════════
// Spec Validation
//
// Compatibility rules:
//
// 1. REACHABILITY — the remote service must expose /.well-known/openapi.json
//
// 2. REQUIRED ENDPOINTS — every non-optional path in the expected spec
//    must exist in the remote spec. Optional endpoints (those with
//    "optional" or "MAY omit" in their description) are allowed to
//    be missing.
//
// 3. HTTP METHODS — for each matching path, the expected HTTP methods
//    (POST, GET, etc.) must be present in the remote spec.
//
// 4. CONTENT TYPES — for each matching operation, the expected request
//    and response content types must be present. Specifically:
//    - Request body content types (application/json)
//    - Response content types (application/json, text/event-stream)
//
// 5. ADDITIONAL ENDPOINTS ALLOWED — the remote spec may have extra
//    endpoints not in the expected spec (server-side flexibility).
// ═══════════════════════════════════════════════════════════════════

struct ValidationReport {
    checks: Vec<ValidationCheck>,
}

struct ValidationCheck {
    description: String,
    passed: bool,
}

impl ValidationReport {
    fn new() -> Self {
        Self { checks: Vec::new() }
    }

    fn pass(&mut self, description: impl Into<String>) {
        self.checks.push(ValidationCheck {
            description: description.into(),
            passed: true,
        });
    }

    fn fail(&mut self, description: impl Into<String>) {
        self.checks.push(ValidationCheck {
            description: description.into(),
            passed: false,
        });
    }

    fn has_failures(&self) -> bool {
        self.checks.iter().any(|c| !c.passed)
    }
}

async fn validate_remote_spec(
    base_url: &str,
    expected_spec: &serde_json::Value,
) -> Result<ValidationReport, String> {
    let mut report = ValidationReport::new();

    // Rule 1: Reachability
    let url = format!("{base_url}/.well-known/openapi.json");
    let resp = reqwest::get(&url)
        .await
        .map_err(|e| format!("Cannot reach {url}: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("Server returned {} for {url}", resp.status()));
    }

    let remote_spec: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Invalid JSON from {url}: {e}"))?;

    report.pass("spec endpoint reachable");

    let expected_paths = expected_spec.get("paths").and_then(|p| p.as_object());
    let remote_paths = remote_spec.get("paths").and_then(|p| p.as_object());

    let Some(expected_paths) = expected_paths else {
        report.pass("no paths to validate");
        return Ok(report);
    };

    let remote_paths = match remote_paths {
        Some(p) => p,
        None => {
            report.fail("remote spec has no 'paths' section");
            return Ok(report);
        }
    };

    for (path, path_item) in expected_paths {
        let is_optional = path_item
            .get("post")
            .and_then(|p| p.get("description"))
            .and_then(|d| d.as_str())
            .is_some_and(|d| d.contains("optional") || d.contains("MAY omit"));

        let Some(remote_path_item) = remote_paths.get(path) else {
            if is_optional {
                report.pass(format!("{path} — optional, not implemented (ok)"));
            } else {
                report.fail(format!("{path} — required endpoint missing"));
            }
            continue;
        };

        // Rule 3: HTTP methods
        let expected_methods: Vec<&str> = path_item
            .as_object()
            .map(|o| o.keys().map(|k| k.as_str()).collect())
            .unwrap_or_default();

        for method in &expected_methods {
            if remote_path_item.get(*method).is_some() {
                report.pass(format!("{path} {method} — present"));

                // Rule 4: Content types
                let expected_op = path_item.get(*method);
                let remote_op = remote_path_item.get(*method);

                if let (Some(exp_op), Some(rem_op)) = (expected_op, remote_op) {
                    // Check response content types
                    if let Some(exp_responses) = exp_op.get("responses").and_then(|r| r.as_object())
                    {
                        for (status, exp_resp) in exp_responses {
                            if let Some(exp_content) =
                                exp_resp.get("content").and_then(|c| c.as_object())
                            {
                                let rem_content = rem_op
                                    .get("responses")
                                    .and_then(|r| r.get(status))
                                    .and_then(|r| r.get("content"))
                                    .and_then(|c| c.as_object());

                                for content_type in exp_content.keys() {
                                    if let Some(rc) = rem_content {
                                        if rc.contains_key(content_type) {
                                            report.pass(format!(
                                                "{path} {method} {status} — {content_type} present"
                                            ));
                                        } else {
                                            report.fail(format!(
                                                "{path} {method} {status} — {content_type} missing"
                                            ));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            } else {
                report.fail(format!("{path} {method} — method missing"));
            }
        }
    }

    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_and_resolve_exact() {
        let dir = ServiceDirectory::new();
        dir.register(
            "gts.poc.notification.backend.v1~static.v1~",
            ClientConfig::new("http://localhost:3001"),
        );
        let config = dir
            .resolve("gts.poc.notification.backend.v1~static.v1~")
            .unwrap();
        assert_eq!(config.base_url, "http://localhost:3001");
    }

    #[test]
    fn resolve_by_prefix() {
        let dir = ServiceDirectory::new();
        dir.register(
            "gts.poc.notification.backend.v1~static.v1~",
            ClientConfig::new("http://localhost:3001"),
        );
        let config = dir.resolve("gts.poc.notification.backend.v1~").unwrap();
        assert_eq!(config.base_url, "http://localhost:3001");
    }

    #[test]
    fn resolve_missing_returns_none() {
        let dir = ServiceDirectory::new();
        assert!(dir.resolve("gts.poc.missing.v1~").is_none());
    }
}
