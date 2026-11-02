# Getting Started with AILang

## Prerequisites

* A Rust toolchain (`rustc` + `cargo`). AILang v0.1's Cargo.toml pins
  dependency versions that build under Rust 1.75+; newer stable toolchains
  work too.

## Building

```sh
git clone https://github.com/ailang-dev/ailang
cd ailang
cargo build --release
```

This produces two binaries under `target/release/`:

* `ail` — the main CLI (run/build/check/fmt/repl/test/init/new/version)
* `ail-lsp` — the Language Server Protocol foundation

Optionally also build the benchmark binary:

```sh
cargo build --release -p ailang-benches
```

## Your first program

Create `hello.ail`:

```ail
println("Hello, world!");
```

Run it:

```sh
ail run hello.ail
```

For an AI-native "hello world", see
[`examples/v05/01_hello_agent.ail`](../examples/v05/01_hello_agent.ail) —
or browse all ten `examples/v05/` programs for agents, tools, memory,
workflows, permissions, and structured output.

## Starting a project

```sh
ail new my_project
cd my_project
ail run src/main.ail
```

This scaffolds:

```text
my_project/
├── ai.toml
└── src/
    └── main.ail
```

`ai.toml` is the package manifest (see [`ailang-pm`](../package_manager/)):

```toml
name = "my_project"
version = "0.1.0"
edition = "2026"
authors = ["AILang Team"]

[dependencies]
```

## The CLI

```sh
ail run <file>          # compile + execute
ail build <file>        # validate + execute (see note below)
ail check <file>        # validate only, no execution
ail fmt <file>           # print formatted source to stdout
ail fmt <file> --write   # format in place
ail repl                 # interactive REPL
ail test [path]          # run every .ail file under `path` (default: ./tests),
                          #  each file's assert() calls determine pass/fail
ail init                 # write ai.toml in the current directory
ail new <name>           # scaffold a new project directory
ail version              # print the ail version
```

> **Note on `ail build`:** v0.1 has no separate bytecode/codegen backend
> yet (see [`roadmap.md`](./roadmap.md)), so `build` currently performs the
> same full front-end validation as `check`, then executes the program like
> `run`. The subcommand exists now so scripts/CI depending on `ail build`
> keep working once a real backend lands — see
> `cli/src/main.rs::cmd_build` for the exact behavior and rationale.

## Writing tests

AILang tests are just `.ail` files that call `assert(...)`. A file that
runs to completion without a failed assertion passes:

```ail
// tests/math_test.ail
assert(1 + 1 == 2);
assert(len("hello") == 5);
```

```sh
ail test tests/
```

## The REPL

```sh
$ ail repl
AILang v0.1 REPL — type an expression, a statement, or :quit to exit.
> let x = 5;
> x + 2
7
> fn square(n: int) -> int { return n * n; }
> square(x)
25
> :history
  1  let x = 5;
  2  x + 2
  3  fn square(n: int) -> int { return n * n; }
  4  square(x)
> :quit
bye!
```

State (variables, functions) persists across lines for the whole session.

## Editor integration (LSP foundation)

Build `ail-lsp` and point an editor extension at it over stdio. It
currently provides live diagnostics (parse/semantic/type errors as you
type) and basic hover; see [`architecture.md`](./architecture.md) for what's
implemented vs. stubbed.

## Where to go next

* [`language-reference.md`](./language-reference.md) — full syntax and semantics
* [`architecture.md`](./architecture.md) — how the compiler is structured
* [`roadmap.md`](./roadmap.md) — what's coming in v0.5 and v1.0
* [`../examples/`](../examples/) — ten worked example programs
