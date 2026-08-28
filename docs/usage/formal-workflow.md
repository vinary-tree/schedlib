# Formal workflow

## Prerequisites

The formal gate requires `tla2sany`, `tlc`, and `tlapm`. Documentation requires
PlantUML and `vinary-doc-lint`. The scripts report a missing tool explicitly.

## Run one proof family

Use a focused command while editing a model:

```sh
./scripts/verify-formal.sh tla
./scripts/verify-formal.sh tlaps
```

`tla` parses both modules and runs all four TLC configurations. `tlaps` proves
the shared nonrecursive kernel theorem.

## Run the acceptance suite

```sh
make verify
```

The formal wrapper enters its own bounded systemd scope. Documentation diagrams
are rendered headlessly and then checked online by `vinary-doc-lint`.

## Inspect evidence

Review these files before cleanup:

| Evidence | Path |
| --- | --- |
| TLA+ syntax | `target/verification/tla-syntax.log` |
| Empty model | `target/verification/tlc-Empty.log` |
| Dependency model | `target/verification/tlc-Dependencies.log` |
| Effect model | `target/verification/tlc-Effects.log` |
| Resource model | `target/verification/tlc-Resources.log` |
| Outcome model | `target/verification/tlc-Outcomes.log` |
| TLAPS proof | `target/verification/tlaps.log` |
| Diagram rendering | `target/documentation/render-diagrams.log` |
| Documentation lint | `target/documentation/vinary-doc-lint.log` |

A TLC pass must include all three of: the explicit no-error verdict, a complete
graph depth, and zero states left on the queue. A process exit without those
lines is incomplete.

## Change protocol

1. Update the formal model.
2. Run the relevant formal family and inspect any counterexample.
3. Update every affected row in `formal/refinement-map.tsv`.
4. Update explanatory documentation and diagrams.
5. Run `make verify`.
6. Record semantic decisions, failures, corrections, state counts, and commit
   evidence in pgmcp.

Production changes follow a stricter sequence: formal change, complete
invariant extraction, tests added and observed failing for the missing
behavior, implementation, then full acceptance.

## Cleanup

Generated evidence lives only beneath the repository's ignored `target/`
directory. After recording reviewed evidence, remove exactly the generated
subdirectories for the completed run. Do not use a broad path, memory-backed
temporary storage, or an unresolved variable as a cleanup target.
