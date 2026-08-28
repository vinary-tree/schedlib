.PHONY: render-diagrams verify-docs verify-formal verify

render-diagrams:
	./scripts/render-diagrams.sh

verify-docs:
	./scripts/verify-docs.sh

verify-formal:
	./scripts/verify-formal.sh all

verify: verify-formal verify-docs
