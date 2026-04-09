.PHONY: build test clean demo demo-compile demo-rest stop openapi expand help

SPI_PORT  ?= 3001
API_PORT  ?= 3010

## Build & Test ───────────────────────────────────────────────

build:
	cargo build

test:
	cargo test

check:
	cargo check

clean:
	cargo clean

## Demos ──────────────────────────────────────────────────────

# Full demo: compile-time SPI + REST SPI + REST API
demo: build stop
	@echo ""
	@echo "╔══════════════════════════════════════════════════════════╗"
	@echo "║  PART 1: Compile-time SPI (email plugin, zero-cost)    ║"
	@echo "╚══════════════════════════════════════════════════════════╝"
	@echo ""
	@./target/debug/poc-host
	@echo ""
	@echo "╔══════════════════════════════════════════════════════════╗"
	@echo "║  PART 2: REST SPI (SMS gateway) + REST API             ║"
	@echo "║  Directory validates OpenAPI specs on registration      ║"
	@echo "╚══════════════════════════════════════════════════════════╝"
	@echo ""
	@PORT=$(SPI_PORT) ./target/debug/notification-plugin-remote & \
	 PORT=$(API_PORT) ./target/debug/notification & \
	 sleep 2; \
	 BINDING_MODE=rest \
	   NOTIFICATION_SPI_URL=http://localhost:$(SPI_PORT) \
	   NOTIFICATION_API_URL=http://localhost:$(API_PORT) \
	 ./target/debug/poc-host; \
	 $(MAKE) --no-print-directory stop

# Compile-time only
demo-compile:
	cargo run --bin poc-host

# REST only
demo-rest: build stop
	@PORT=$(SPI_PORT) ./target/debug/notification-plugin-remote & \
	 PORT=$(API_PORT) ./target/debug/notification & \
	 sleep 2; \
	 BINDING_MODE=rest \
	   NOTIFICATION_SPI_URL=http://localhost:$(SPI_PORT) \
	   NOTIFICATION_API_URL=http://localhost:$(API_PORT) \
	 ./target/debug/poc-host; \
	 $(MAKE) --no-print-directory stop

stop:
	@-pkill -f "notification-plugin-remote" 2>/dev/null; \
	 pkill -f "target/debug/notification[^-]" 2>/dev/null; \
	 true

## OpenAPI ────────────────────────────────────────────────────

openapi:
	@cargo run --quiet --bin openapi-gen

openapi-gen: build
	@mkdir -p target/openapi
	@cargo run --quiet --bin openapi-gen > target/openapi/notification.json
	@echo "Generated target/openapi/notification.json"

## Inspect ────────────────────────────────────────────────────

expand:
	cargo expand --package notification-sdk

## Help ──────────────────────────────────────────────────────

help:
	@echo "modkit-binding-poc (Notification module)"
	@echo ""
	@echo "  make demo           Full demo — compile SPI + REST SPI + REST API"
	@echo "  make demo-compile   Compile-time email plugin only"
	@echo "  make demo-rest      REST mode: remote SMS gateway + API client"
	@echo "  make test           Run all tests"
	@echo "  make openapi        Print API + SPI specs"
	@echo "  make expand         Show macro-expanded notification-sdk"
	@echo "  make stop           Stop background services"
	@echo "  make clean          Clean build artifacts"
