//! # ailang-benches
//!
//! Lightweight, dependency-free benchmarks (`std::time::Instant`, run each
//! workload N times, report mean) for the lexer, parser, and interpreter.
//!
//! **Why not `criterion`?** `criterion` is the intended long-term tool (see
//! `docs/roadmap.md`) — it has proper statistical rigor, HTML reports, and
//! regression detection that this binary doesn't attempt. It's deliberately
//! left out of v0.1's default dependency graph because its transitive tree
//! (via `plotters`, `tinytemplate`, etc.) requires a newer Rust toolchain
//! than the one used to build and validate this v0.1 release, and adding it
//! is a drop-in upgrade once that constraint no longer applies — this
//! binary exists so "benchmarks where appropriate" (the spec's requirement)
//! is satisfied today, not deferred to "someday we'll add criterion."
//!
//! Run with `cargo run -p ailang-benches --release`.

use std::time::Instant;

const ITERATIONS: u32 = 200;

fn fib_source(n: u32) -> String {
    format!(
        "fn fib(n: int) -> int {{ if n < 2 {{ return n; }} return fib(n - 1) + fib(n - 2); }} \
         println(fib({n}));"
    )
}

fn bench(name: &str, iterations: u32, mut f: impl FnMut()) {
    // Warm up (page faults, allocator warm-up) before timing.
    for _ in 0..3 {
        f();
    }
    let start = Instant::now();
    for _ in 0..iterations {
        f();
    }
    let elapsed = start.elapsed();
    let per_iter = elapsed / iterations.max(1);
    println!(
        "{name:<28} total {elapsed:>10.2?}   mean/iter {per_iter:>10.2?}   ({iterations} iters)"
    );
}

fn main() {
    println!("AILang v0.1 benchmarks ({ITERATIONS} iterations each)\n");

    let hello = "println(\"Hello, world!\");".to_string();
    bench("lex: hello world", ITERATIONS, || {
        let _ = ailang_lexer::tokenize(&hello);
    });

    let fib15 = fib_source(15);
    bench("lex: fib(15) source", ITERATIONS, || {
        let _ = ailang_lexer::tokenize(&fib15);
    });

    bench("parse: fib(15) source", ITERATIONS, || {
        let _ = ailang_parser::parse(&fib15);
    });

    bench("parse+semantic: fib(15)", ITERATIONS, || {
        let (program, diags) = ailang_parser::parse(&fib15);
        if !diags.has_errors() {
            let _ = ailang_semantic::SemanticAnalyzer::new().analyze(&program.unwrap());
        }
    });

    let fib20 = fib_source(20);
    let (program, _) = ailang_parser::parse(&fib20);
    let program = program.unwrap();
    bench("interpret: fib(20) (recursive)", 20, || {
        let mut sink: Vec<u8> = Vec::new();
        let mut interp = ailang_interpreter::Interpreter::new(&mut sink);
        interp.run(&program).unwrap();
    });

    let loop_source = "let sum = 0; for i in 0..100000 { sum += i; } println(sum);".to_string();
    let (loop_program, _) = ailang_parser::parse(&loop_source);
    let loop_program = loop_program.unwrap();
    bench("interpret: 100k-iteration loop", 20, || {
        let mut sink: Vec<u8> = Vec::new();
        let mut interp = ailang_interpreter::Interpreter::new(&mut sink);
        interp.run(&loop_program).unwrap();
    });
}
