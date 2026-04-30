.PHONY: sqld sqld-down test test-sync test-vector check velesdb-sidecar sim sim-stop sim-clean dev deploy backup restore

SIM_DIR := /tmp/bamako-sim

# ── sqld ─────────────────────────────────────────────────────────────────────

sqld:
	docker compose up -d
	@echo "sqld running at http://127.0.0.1:8093 — wait a moment for it to be ready"

sqld-down:
	docker compose down

sqld-logs:
	docker compose logs -f sqld

# ── tests ─────────────────────────────────────────────────────────────────────

test: sqld
	@sleep 2
	cd src-tauri && cargo test -- --nocapture

test-sync: sqld
	@sleep 2
	cd src-tauri && cargo test db_sync_test -- --nocapture

test-vector: sqld
	@sleep 2
	cd src-tauri && cargo test vector_test -- --nocapture

# ── VelesDB sidecar ───────────────────────────────────────────────────────────
# Builds VelesDB from source and places the binary where Tauri expects it.
# Run once before `npm run tauri dev` or `npm run tauri build`.

TARGET := $(shell rustc -vV 2>/dev/null | grep "host:" | awk '{print $$2}')
VELES_BIN := src-tauri/binaries/velesdb-server-$(TARGET)

velesdb-sidecar:
	@echo "Building VelesDB for $(TARGET)…"
	@if [ ! -d /tmp/VelesDB ]; then git clone https://github.com/cyberlife-coder/VelesDB.git /tmp/VelesDB; fi
	cd /tmp/VelesDB && git pull && cargo build --release --bin velesdb-server
	mkdir -p src-tauri/binaries
	cp /tmp/VelesDB/target/release/velesdb-server $(VELES_BIN)
	@echo "Sidecar ready: $(VELES_BIN)"

# ── simulation ────────────────────────────────────────────────────────────────

## Start a local simulation: sqld + seeded namespaces
## Then prints instructions for starting two Bamako instances
sim: sim-stop
	@mkdir -p $(SIM_DIR)/alice $(SIM_DIR)/bob
	@echo "==> Starting two sqld instances: shared(:8093) alice-private(:8095)..."
	docker compose -f docker-compose.sim.yml up -d
	@echo "==> Waiting for sqld instances to be ready..."
	@for i in 1 2 3 4 5 6 7 8 9 10; do \
	  curl -sf -X POST http://127.0.0.1:8093/v2/pipeline \
	    -H "Content-Type: application/json" \
	    -d '{"requests":[{"type":"execute","stmt":{"sql":"SELECT 1"}}]}' \
	    >/dev/null 2>&1 && break || sleep 1; \
	done
	@echo "==> Seeding namespaces..."
	cd src-tauri && cargo run --bin seed

sim-stop:
	docker compose -f docker-compose.sim.yml down 2>/dev/null || true

sim-clean: sim-stop
	docker compose -f docker-compose.sim.yml down -v 2>/dev/null || true
	@rm -rf $(SIM_DIR) /tmp/bam-alice /tmp/bam-bob
	@echo "Simulation data cleaned."

## Start normal dev server
dev:
	npm run tauri dev

# ── server deploy ─────────────────────────────────────────────────────────────

## Deploy sqld binary update to DEPLOY_SERVER
deploy:
	@[ -n "$(DEPLOY_SERVER)" ] || (echo "Set DEPLOY_SERVER=<ip>"; exit 1)
	DEPLOY_SERVER=$(DEPLOY_SERVER) ./deploy.sh

## First-time server setup (requires DEPLOY_SERVER and DOMAIN)
deploy-setup:
	@[ -n "$(DEPLOY_SERVER)" ] || (echo "Set DEPLOY_SERVER=<ip>"; exit 1)
	@[ -n "$(DOMAIN)" ] || (echo "Set DOMAIN=docs.example.com"; exit 1)
	DEPLOY_SERVER=$(DEPLOY_SERVER) ./deploy.sh --setup --domain=$(DOMAIN)

## Backup data from server (pulls to ~/Documents/bamako-backups/)
backup:
	@[ -n "$(DEPLOY_SERVER)" ] || (echo "Set DEPLOY_SERVER=<ip>"; exit 1)
	DEPLOY_SERVER=$(DEPLOY_SERVER) ./backup.sh

## Restore data to server (most recent local backup; or BACKUP=name for specific)
restore:
	@[ -n "$(DEPLOY_SERVER)" ] || (echo "Set DEPLOY_SERVER=<ip>"; exit 1)
	DEPLOY_SERVER=$(DEPLOY_SERVER) ./restore.sh $(if $(BACKUP),$(BACKUP),)

# ── dev ───────────────────────────────────────────────────────────────────────

check:
	cd src-tauri && cargo check
