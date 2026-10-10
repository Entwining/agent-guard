CARGO ?= cargo

.PHONY: build check rust-check

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

# Everything the package ships is staged here, so that a packaging change ships with its release once the tap's formula installs this package.
build:
	$(call validate_external_directory,$(OUT),OUT)
	$(call validate_external_directory,$(CARGO_TARGET_DIR),CARGO_TARGET_DIR)
	$(CARGO) build --locked --release --bin agent-guard-native --target-dir "$(CARGO_TARGET_DIR)"
	mkdir -p "$(OUT)/bin"
	install -m 755 "$(CARGO_TARGET_DIR)/release/agent-guard-native" "$(OUT)/bin/agent-guard-native"
	install -m 755 bin/agent-guard "$(OUT)/bin/agent-guard"
	install -m 644 VERSION LICENSE README.md "$(OUT)/"
# cargo metadata needs every target's crates, which no build fetches; cargo-about only logs a dropped notice, such as a clarification whose checksum no longer matches, so any warning or error fails, and so does an unreadable log, because grep reports a read error as status 2.
	$(CARGO) fetch --locked
	$(CARGO) about generate --frozen --fail --output-file "$(OUT)/LICENSE-THIRD-PARTY.md" about.hbs 2> "$(CARGO_TARGET_DIR)/cargo-about.log" || { cat "$(CARGO_TARGET_DIR)/cargo-about.log" >&2; exit 1; }
	@cat "$(CARGO_TARGET_DIR)/cargo-about.log" >&2 && { grep -Eq 'WARN|ERROR' "$(CARGO_TARGET_DIR)/cargo-about.log"; test $$? -eq 1; }

check: rust-check

rust-check:
	$(call validate_external_directory,$(CARGO_TARGET_DIR),CARGO_TARGET_DIR)
	$(CARGO) fmt -- --check
	$(CARGO) run --locked --bin agent-guard-structure -- "$(CURDIR)"
	CARGO_BUILD_WARNINGS=deny $(CARGO) clippy --locked --all-targets
	$(CARGO) test --locked
	$(CARGO) deny --locked check
