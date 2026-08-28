.PHONY: render-diagrams verify-docs verify-formal verify-rust verify

render-diagrams:
	./scripts/render-diagrams.sh

verify-docs:
	./scripts/verify-docs.sh

verify-formal:
	./scripts/verify-formal.sh all

verify-rust:
	./scripts/verify-rust.sh

verify: verify-formal verify-rust verify-docs
