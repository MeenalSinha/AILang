# Contributing to AILang

## Project layout

See [`docs/architecture.md`](./docs/architecture.md) for the full crate map
and why the pipeline is split the way it is. In short: `compiler/*` is the
language implementation (lexer through interpreter), `cli/` /`repl/`
/`formatter/` /`package_manager/` /`language_server/` are the tools built
on top of it, and each has its own `Cargo.toml` so dependencies stay
scoped to where they're actually needed.

## Building and testing

```sh
cargo build --workspace
cargo test --workspace          # unit tests (every compiler/* crate + ai_runtime/) +
                                 # golden tests (cli/tests/golden.rs)
ail test tests/integration      # .ail-level integration tests via the real CLI
```

A change should not reduce the passing count of any of the above. If you
add a language feature, add:

1. A lexer/parser unit test (in the relevant crate's `#[cfg(test)]` module)
   exercising the new syntax.
2. A semantic-analysis test if the feature introduces new scoping/name
   rules.
3. A type-checker test if it introduces new type rules.
4. An interpreter test showing it executes correctly.
5. A `.ail` example under `examples/` if it's a user-facing feature, and/or
   an integration test under `tests/integration/`.

## Coding conventions

* Every public item gets a doc comment. Module-level doc comments
  (`//! ...`) at the top of each `lib.rs`/`main.rs` should explain *why*
  the module is structured the way it is, not just *what* it contains —
  see any existing crate for the expected depth.
* No `unwrap()`/`expect()` on user-controlled input outside of tests —
  return a `Diagnostic`/`RuntimeError` instead.
* Keep `match` statements over `Stmt`/`Expr` exhaustive (no wildcard `_ =>`
  arms) so adding a new AST variant in v0.5+ produces compiler errors at
  every call site that needs updating, rather than silently no-op'ing.
* Run `cargo fmt` (standard `rustfmt`, for the Rust implementation itself —
  not to be confused with `ail fmt`, which formats `.ail` source) before
  submitting.

## Adding a v2.1+ construct

`docs/roadmap.md`'s "v2.1+ — Future Work" section lists what's next. The
mechanical pattern for adding a new AI-native construct is the same one
v0.5 and v1.0 used for `agent`/`workflow`/`knowledge`/etc.: a new
`Stmt`/`Expr` variant in `ailang-ast`, a new parse function dispatched
from `declaration()`, new (exhaustive — no wildcard `_ =>`) match arms in
`ailang-semantic` and `ailang-types`, and execution logic in
`ailang-interpreter` that, if it needs real behavior beyond simple value
binding, belongs in `ailang-ai-runtime` (not the interpreter crate
itself) — see that crate's module doc for the dependency-direction rule
(`ailang-ai-runtime` depends on `ailang-runtime`, never the reverse).
Because every `match` is exhaustive, the compiler will tell you every
file that needs a new arm once you add the variant — follow the compiler
errors, the way the v0.5, v1.0, and v2.0 upgrades did at every step.

## Reporting issues

Please include:

* The `.ail` source that triggers the issue (minimal repro if possible)
* The output of `ail check <file>` or `ail run <file>`
* Your `ail version` output

## License

By contributing, you agree your contributions are licensed under the MIT
License (see [`LICENSE`](./LICENSE)).
