CARGO ?= cargo

.PHONY: build check rust-check

package_target = $(or $(CARGO_TARGET_DIR),$(OUT)/../cargo-target)

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
	$(call validate_external_directory,$(package_target),CARGO_TARGET_DIR)
	$(CARGO) build --locked --release --bin agent-guard-native --target-dir "$(package_target)"
	mkdir -p "$(OUT)/bin"
	install -m 755 "$(package_target)/release/agent-guard-native" "$(OUT)/bin/agent-guard-native"
	install -m 755 bin/agent-guard "$(OUT)/bin/agent-guard"
	install -m 644 VERSION LICENSE README.md "$(OUT)/"
# cargo metadata needs every target's crates, which no build fetches; cargo-about only logs a dropped notice, such as a clarification whose checksum no longer matches, so any warning or error fails.
	$(CARGO) fetch --locked
	$(CARGO) about generate --frozen --fail --output-file "$(OUT)/LICENSE-THIRD-PARTY.md" about.hbs 2> "$(package_target)/cargo-about.log" || { cat "$(package_target)/cargo-about.log" >&2; exit 1; }
	@cat "$(package_target)/cargo-about.log" >&2; ! grep -Eq 'WARN|ERROR' "$(package_target)/cargo-about.log"

check: rust-check

rust-check:
	$(call validate_external_directory,$(CARGO_TARGET_DIR),CARGO_TARGET_DIR)
	$(CARGO) fmt -- --check
	$(CARGO) run --locked --bin agent-guard-structure -- "$(CURDIR)"
	CARGO_BUILD_WARNINGS=deny $(CARGO) clippy --locked --all-targets
	$(CARGO) test --locked
	$(CARGO) deny --locked check
