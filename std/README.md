# `std/`

AILang v0.1's standard library is a small, flat set of always-in-scope
functions (`print`, `println`, `len`, `type`, `assert`, `clock`,
`to_string`, `parse_int`, `parse_float`, `input`, `exit`), specified in
`docs/language-reference.md#standard-library` and implemented natively in
Rust at `compiler/interpreter/src/stdlib.rs` — there is no `.ail`-source
standard library to place in this directory yet.

This directory is reserved for the future `.ail`-authored standard library
(collections helpers, string utilities, `std.io`/`std.math`/etc. modules
referenced by `import std.io;` in the language spec) once:

1. `import` resolves real module files rather than only checking usage
   (see `docs/roadmap.md`), and
2. `module`-qualified access (`std.io.print(...)`) works end-to-end (also
   tracked in `docs/roadmap.md` — currently `module` hoists into the
   global scope rather than creating a real namespace).

Until then, treat the table in `docs/language-reference.md` as the
authoritative stdlib surface for v0.1.
