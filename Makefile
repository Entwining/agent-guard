GO ?= go
STATICCHECK ?= staticcheck

.PHONY: build check

build:
	@test -n "$(OUT)" || { printf '%s\n' 'Set OUT to a directory outside the checkout.' >&2; exit 1; }
	@case "$(OUT)" in /*) ;; *) printf '%s\n' 'OUT must be absolute.' >&2; exit 1;; esac; \
	ancestor="$(OUT)"; \
	while test ! -e "$$ancestor"; do ancestor=$${ancestor%/*}; test -n "$$ancestor" || ancestor=/; done; \
	ancestor=$$(cd "$$ancestor" && pwd -P) || exit 1; \
	while test "$$ancestor" != /; do \
	  test ! -e "$$ancestor/.git" || { printf '%s\n' 'OUT must stay outside Git checkouts.' >&2; exit 1; }; \
	  ancestor=$${ancestor%/*}; test -n "$$ancestor" || ancestor=/; \
	done
	mkdir -p "$(OUT)/bin"
	$(GO) build -trimpath -ldflags "-X main.version=$$(cat VERSION)" -o "$(OUT)/bin/agent-guard-native" ./cmd/agent-guard
	install -m 755 bin/agent-guard "$(OUT)/bin/agent-guard"
	install -m 644 VERSION LICENSE README.md "$(OUT)/"

check:
	GO="$(GO)" STATICCHECK="$(STATICCHECK)" native/check
