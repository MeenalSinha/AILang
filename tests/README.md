# `tests/`

This directory holds tests that exercise AILang **as a whole system**
(source file in, behavior out) rather than a single crate's internals —
those live next to their crate as `#[cfg(test)] mod tests` (see
`compiler/*/src/lib.rs`, `formatter/src/lib.rs`, `package_manager/src/lib.rs`).

| Directory      | Purpose                                                                 |
|-----------------|--------------------------------------------------------------------------|
| `integration/` | `.ail` programs run via `ail test` (or `cargo test -p ail`, see below). Each asserts its own correctness with `assert(...)` — a program that runs to completion without a failed `assert` is a passing test. This is the "integration tests" and "golden file tests" bucket from the spec: the `.ail` source *is* the fixture. |
| `lexer/`       | Placeholder for future token-stream golden files once the lexer needs cross-version regression fixtures beyond its inline unit tests. |
| `parser/`      | Placeholder for future AST-shape snapshot tests (see `golden/` for the mechanism already wired up). |
| `interpreter/` | Placeholder for future execution-trace golden files. |
| `golden/`      | `insta`-style golden/snapshot tests for full source -> output pipelines, driven by `cli/tests/golden.rs`. Subdirectories `v05/`, `v10/`, and `v20/` hold fixtures for the AI-native example programs under `examples/v05/`, `examples/v10/`, and `examples/v20/`. |

## Running everything

```sh
cargo test --workspace     # every crate's unit tests + this directory's golden tests
ail test tests/integration # just the .ail integration suite, via the real CLI
```
