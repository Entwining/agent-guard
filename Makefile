GO ?= go
CARGO ?= cargo

.PHONY: build check rust-build rust-check

define validate_external_directory
@test -n "$(1)" || { printf '%s\n' 'Set $(2) to a directory outside the checkout.' >&2; exit 1; }
@case "$(1)" in /*) ;; *) printf '%s\n' '$(2) must be absolute.' >&2; exit 1;; esac; \
	ancestor="$(1)"; \
	while test ! -e "$$ancestor"; do ancestor=$${ancestor%/*}; test -n "$$ancestor" || ancestor=/; done; \
	ancestor=$$(cd "$$ancestor" && pwd -P) || exit 1; \
	while test "$$ancestor" != /; do \
	  test ! -e "$$ancestor/.git" || { printf '%s\n' '$(2) must stay outside Git checkouts.' >&2; exit 1; }; \
	  ancestor=$${ancestor%/*}; test -n "$$ancestor" || ancestor=/; \
	done
endef

build:
	$(call validate_external_directory,$(OUT),OUT)
	mkdir -p "$(OUT)/bin"
	$(GO) build -trimpath -ldflags "-X main.version=$$(cat VERSION)" -o "$(OUT)/bin/agent-guard-native" ./cmd/agent-guard
	install -m 755 bin/agent-guard "$(OUT)/bin/agent-guard"
	install -m 644 VERSION LICENSE README.md "$(OUT)/"

check:
	GO="$(GO)" native/check

rust-build:
	$(call validate_external_directory,$(OUT),OUT)
	$(call validate_external_directory,$(CARGO_TARGET_DIR),CARGO_TARGET_DIR)
	$(CARGO) build --locked --release --bin agent-guard-rust-slice --target-dir "$(CARGO_TARGET_DIR)"
	mkdir -p "$(OUT)/bin"
	install -m 755 "$(CARGO_TARGET_DIR)/release/agent-guard-rust-slice" "$(OUT)/bin/agent-guard-rust-slice"

rust-check:
	$(call validate_external_directory,$(CARGO_TARGET_DIR),CARGO_TARGET_DIR)
	$(CARGO) fmt -- --check
	CARGO_BUILD_WARNINGS=deny $(CARGO) clippy --locked --all-targets
	$(CARGO) test --locked
	$(CARGO) deny --locked check
