# ModKit Binding PoC

## Problem

ModKit modules define Rust traits as their extension points. Today, binding an implementation to a trait requires compiling it into the same binary. This works for in-process plugins but breaks when the implementation lives in a separate process, is written in another language, or is developed by a third party with an independent release cycle.

We need a binding mechanism that works across compilation boundaries while preserving the zero-cost in-process path.

## Approach

We propose a **two-layer contract-binding system** where module contracts (plain Rust traits) are separated from their transport projections (REST traits with HTTP annotations). The consumer always depends on the base trait and is completely unaware of the binding mode.

## Naming Convention

Module contracts follow a naming convention that encodes what the trait is and how it can be bound:

```
                 Always local              Can be remote
                 ────────────              ──────────────

Provided         {Module}Api               {Module}ApiRest
(module serves)  NotificationApi           NotificationApiRest

Required         {Module}Extension          {Module}Backend
(plugin serves)  NotificationFormatter      NotificationBackend
                 (compile-only,             NotificationBackendRest
                  no REST option)
```

**Api** — the module IS the service. Consumers call it. The module implements the base trait and optionally exposes an `ApiRest` projection for remote consumers.

**Extension** — the module NEEDS a compile-time plugin. Always local, no REST projection, no macro — just a plain Rust trait. For performance-critical hooks where remote calls are unacceptable (e.g., OAGW transforms, credential resolution, message formatting).

**Backend** — the module NEEDS a plugin that MAY be remote. The base trait (`Backend`) is clean. A `BackendRest` projection can be added for remote binding. The macro generates the REST client and OpenAPI spec from the REST trait.

The base trait (Api, Extension, Backend) is always a plain Rust trait with zero annotations. Only the `*Rest` traits carry transport annotations (`#[post(...)]`, `#[get(...)]`, `#[streaming]`, `#[retryable]`). Consumers always depend on the base trait.

A single module can have all three. For example, the notification module:
- **Provides** `NotificationApi` (+ `NotificationApiRest` for remote consumers)
- **Requires** `NotificationBackend` (+ `NotificationBackendRest` for remote plugins)
- **Requires** `NotificationFormatter` (extension, always local)

## What This PoC Shows

**Compile-time and out-of-process binding.** This PoC shows that the same trait can be bound in-process (direct function call, zero serialization) or via REST (HTTP + JSON, cross-process). The consumer code is identical in both cases — it receives `Arc<dyn Trait>` and calls methods on it.

**OpenAPI-spec-driven validation of remote plugins.** This PoC shows that remote services can expose their generated OpenAPI spec at `/.well-known/openapi.json`. The directory service fetches this spec on registration and validates that all required endpoints exist before accepting the service. This ensures contract compatibility at wiring time, not at first call.

**Macro-driven code generation from traits.** This PoC shows that a `#[modkit_contract]` proc macro on a trait can generate the REST client proxy, the OpenAPI spec, and the SSE streaming support. The `#[derive(ContractError)]` macro on an error enum generates the RFC 9457 Problem Details conversion — including `error_code` and `error_domain` fields for machine-readable error reconstruction across process boundaries. No hand-written boilerplate.

**SSE streaming across the REST boundary.** This PoC shows that methods marked `#[modkit_contract(streaming)]` generate SSE-aware REST clients that negotiate `text/event-stream` and parse server-sent events into a native Rust `Stream`. The remote plugin emits events with `id:` fields for reconnection support.

**Structured error mapping across process boundaries.** This PoC shows that module-specific error enums carrying semantic meaning (e.g., `NotificationNotFound { notification_id }`) can survive the REST round-trip. The `(error_domain, error_code)` pair on the wire maps 1:1 to the Rust enum variant. The REST client proxy reconstructs the exact typed error, including structured context fields.

**Directory service with GTS-like resolution.** This PoC shows that services can register by GTS-like IDs with client configuration (timeout, retry policy). The directory resolves by exact match or prefix and provides the `ClientConfig` to the generated REST client.

**Retryable methods with exponential backoff.** This PoC shows that methods marked `#[modkit_contract(retryable)]` can be automatically retried on transient failures. Retry policy (max retries, base delay, max delay) is carried in the `ClientConfig` from the directory.

**Non-breaking evolution.** This PoC shows that `#[non_exhaustive]` types enable additive field changes without breaking existing plugins or consumers. Trait methods can be added with default implementations without breaking existing SPI plugins.

## Out of Scope

**Integration with the real ModKit platform.** This PoC is standalone. Integration with ClientHub, GTS plugin system, `inventory` discovery, and the module lifecycle is a separate effort. The PoC proves the patterns; the platform integration applies them.

**Cross-language implementations.** The PoC generates OpenAPI specs that a Go or Java team could implement against, but we don't build actual cross-language clients here.

**Production-quality macros.** The proc macros handle the patterns demonstrated but don't cover all Rust edge cases (generic traits, lifetime parameters, multiple error types per trait).

**Versioning (v1/v2 coexistence).** The proposal describes Kubernetes-style internal type + per-version conversion functions. The PoC only demonstrates v1.

**SSE reconnection.** The remote plugin sends `id:` fields on events, but the REST client doesn't implement auto-reconnect with `Last-Event-ID` yet.

**Security and service-level authentication.** The REST proxy doesn't inject service-level credentials. The proposal says the service is responsible for its own security context.

**Contract test harness.** The directory validates endpoint existence but doesn't run full contract tests (request/response shape, error code coverage).

**Performance benchmarking.** No latency or throughput comparison between in-process and REST binding.

## Running the PoC

```bash
# Full demo — compile-time + REST, all binding combinations
bash run-demo.sh

# Tests
cargo test

# Generated OpenAPI specs (both API and SPI)
cargo run --bin openapi-gen

# Macro-expanded code
cargo expand --package notification-sdk
```

## Testing

Consumer code holds `Arc<dyn NotificationBackend>` — the **base trait** is the mock point, not the transport projection. This means the module can be tested without booting the real delivery plugin or the REST server: swap the backend for a mock and the consumer code is identical.

Two styles are demonstrated in `modules/notification/notification/tests/`:

- **Manual mock** (`manual_mock.rs`) — a plain struct implementing `NotificationBackend` with queued responses and recorded calls. No extra dependencies.
- **mockall mock** (`mockall_mock.rs`) — uses the `mockall` crate's `mock!` macro to generate `MockBackend` with `expect_*` methods. Works with `#[async_trait]` and `Pin<Box<dyn Stream>>` return types.

```bash
cargo test --package notification
# Runs:
# - 2 manual mock tests
# - 3 mockall tests
```

**Why the base trait is the right mock point:**

- Consumers depend on `Arc<dyn NotificationBackend>` regardless of binding mode.
- Real implementations (compile-time plugin, generated REST client, hand-written client) and mocks are structurally indistinguishable.
- Tests exercise the module's orchestration logic (channel selection, status tracking, error mapping) without touching the network or the real plugin.

## Structure

```
modules/
  notification/
    notification-sdk/         API + SPI traits, types, errors
                              macro-generated REST clients and OpenAPI specs
    notification/             module: impl NotificationApi, uses NotificationBackend
                              REST server binary
  notification-plugin/
    notification-plugin-email/    compile-time SPI plugin (email delivery)
    notification-plugin-remote/   REST SPI plugin (SMS gateway, standalone binary)

modkit-contract-macros/       #[modkit_contract], #[derive(ContractError)]
modkit-contract-runtime/      ProblemDetails, SSE parser, ClientConfig, retry
modkit-directory/             service directory, GTS resolution, spec validation
poc-host/                     demo: wires everything, exercises all binding modes
```

## Related

- Proposal: `openspec/changes/oop-extensions-binding/proposal.md`
- Design: `openspec/changes/oop-extensions-binding/design.md`
- ADR-0004 (PR #1380): Module/plugin declaration and resolution — complementary, no conflicts
