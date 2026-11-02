//! The `ail` command-line tool.
//!
//! This is the single entry point end users interact with; it wires
//! together every compiler phase (lexer -> parser -> semantic analyzer ->
//! type checker -> interpreter) plus the formatter, package manager
//! foundation, and REPL, behind the subcommands described in the language
//! spec:
//!
//! ```text
//! ail run <file>
//! ail build <file>
//! ail check <file>
//! ail fmt [file]
//! ail repl
//! ail test [path]
//! ail version
//! ail init
//! ail new <ProjectName>
//! ```

use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use ailang_ast::Program;
use ailang_diagnostics::DiagnosticBag;
use clap::{Parser, Subcommand};

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Parser)]
#[command(
    name = "ail",
    version = VERSION,
    about = "AILang: an agentic-systems programming language (v0.1 foundation)",
    long_about = "AILang v0.1 is the foundation release of an AI-native programming \
language. This release implements the core language (lexer, parser, semantic \
analysis, type checking, and a tree-walk interpreter) without any AI-specific \
constructs; those arrive in v0.5+. See the roadmap in docs/roadmap.md."
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run an .ail source file
    Run {
        file: PathBuf,
        /// Apply constant-folding / dead-code-elimination before running
        /// (see ailang-optimizer's module doc)
        #[arg(long)]
        optimize: bool,
        /// Print an execution timeline (workflow stages, team members,
        /// tool invocations) after the program finishes
        #[arg(long)]
        trace: bool,
        /// Step through top-level statements interactively (n/next,
        /// c/continue, p/print <var>, q/quit)
        #[arg(long)]
        debug: bool,
    },
    /// Type-check and validate a source file, then run it (alias used by
    /// early-stage tooling before a bytecode backend exists)
    Build {
        file: PathBuf,
        #[arg(long)]
        optimize: bool,
        #[arg(long)]
        trace: bool,
    },
    /// Check a file for errors without running it
    Check { file: PathBuf },
    /// Format an .ail source file (prints to stdout unless --write is given)
    Fmt {
        file: PathBuf,
        #[arg(long)]
        write: bool,
    },
    /// Start the interactive REPL
    Repl,
    /// Run all `.ail` test files under a directory (default: ./tests)
    Test { path: Option<PathBuf> },
    /// Print the AILang version
    Version,
    /// Initialize an ai.toml manifest in the current directory
    Init,
    /// Create a new AILang project in a new directory
    New { name: String },
    /// Package the current project (ai.toml + every .ail file) into
    /// dist/<target>/ with a deploy.json manifest. See
    /// ailang-pm::bundle_for_deploy's doc for exactly what this does and
    /// does not do (no real cloud/edge provisioning).
    Deploy { target: String },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Commands::Run {
            file,
            optimize,
            trace,
            debug,
        } => cmd_run(&file, optimize, trace, debug),
        Commands::Build {
            file,
            optimize,
            trace,
        } => cmd_build(&file, optimize, trace),
        Commands::Check { file } => cmd_check(&file),
        Commands::Fmt { file, write } => cmd_fmt(&file, write),
        Commands::Repl => cmd_repl(),
        Commands::Test { path } => cmd_test(path.unwrap_or_else(|| PathBuf::from("tests"))),
        Commands::Version => {
            println!("ail {VERSION}");
            Ok(())
        }
        Commands::Init => cmd_init(),
        Commands::New { name } => cmd_new(&name),
        Commands::Deploy { target } => cmd_deploy(&target),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Shared front-end pipeline: read source, lex+parse, run semantic analysis
/// and the type checker, and print any diagnostics. Returns the parsed
/// program only if there were no *errors* (warnings still allow the
/// program through, matching `rustc`'s behavior).
fn compile(path: &Path) -> anyhow::Result<Option<(String, Program)>> {
    let source = fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("could not read {}: {e}", path.display()))?;
    let filename = path.display().to_string();

    let (program, mut diags) = ailang_parser::parse(&source);
    let program = match program {
        Some(p) => p,
        None => {
            print_diags(&diags, &filename, &source);
            return Ok(None);
        }
    };

    if !diags.has_errors() {
        let sem_diags = ailang_semantic::SemanticAnalyzer::new().analyze(&program);
        diags.extend(sem_diags);
    }
    if !diags.has_errors() {
        let type_diags = ailang_types::TypeChecker::new().check_program(&program);
        diags.extend(type_diags);
    }

    print_diags(&diags, &filename, &source);

    if diags.has_errors() {
        Ok(None)
    } else {
        Ok(Some((source, program)))
    }
}

fn print_diags(diags: &DiagnosticBag, filename: &str, source: &str) {
    for d in &diags.diagnostics {
        eprintln!("{}", d.render(filename, source));
    }
    if diags.error_count() > 0 || diags.warning_count() > 0 {
        eprintln!(
            "{} error(s), {} warning(s)",
            diags.error_count(),
            diags.warning_count()
        );
    }
}

fn cmd_check(file: &Path) -> anyhow::Result<()> {
    match compile(file)? {
        Some(_) => {
            println!("checked {} — no errors", file.display());
            Ok(())
        }
        None => anyhow::bail!("compilation failed"),
    }
}

fn cmd_build(file: &Path, optimize: bool, trace: bool) -> anyhow::Result<()> {
    // v0.1 has no separate codegen/bytecode backend yet (see roadmap), so
    // `build` performs full front-end validation and then executes the
    // program, the same as `run`, but reports itself as a "build" step so
    // scripts and CI depending on `ail build` keep working once a real
    // backend lands.
    //
    // It also resolves and locks dependencies (writing `ai.lock`) if an
    // `ai.toml` manifest is present in the current directory — this is the
    // one point in the CLI where "building a project" (vs. just running a
    // loose script file) is meaningful.
    if let Ok(cwd) = std::env::current_dir() {
        let manifest_path = cwd.join("ai.toml");
        if manifest_path.exists() {
            match ailang_pm::Manifest::load(&manifest_path) {
                Ok(manifest) => match ailang_pm::write_lockfile(&cwd, &manifest) {
                    Ok(lockfile) => {
                        if !lockfile.dependencies.is_empty() {
                            println!(
                                "resolved {} dependencies -> ai.lock",
                                lockfile.dependencies.len()
                            );
                        }
                    }
                    Err(e) => eprintln!("warning: could not write ai.lock: {e}"),
                },
                Err(e) => eprintln!("warning: could not read ai.toml: {e}"),
            }
        }
    }

    match compile(file)? {
        Some((_, program)) => {
            println!("build succeeded: {}", file.display());
            execute(&program, optimize, trace, false)
        }
        None => anyhow::bail!("build failed"),
    }
}

fn cmd_run(file: &Path, optimize: bool, trace: bool, debug: bool) -> anyhow::Result<()> {
    match compile(file)? {
        Some((_, program)) => execute(&program, optimize, trace, debug),
        None => anyhow::bail!("could not run: compilation failed"),
    }
}

fn execute(program: &Program, optimize: bool, trace: bool, debug: bool) -> anyhow::Result<()> {
    let optimized;
    let program = if optimize {
        optimized = ailang_optimizer::optimize_program(program);
        &optimized
    } else {
        program
    };
    let stdout = io::stdout();
    let mut interp = ailang_interpreter::Interpreter::new(stdout.lock());
    let result = if debug {
        run_debug_session(&mut interp, program)
    } else {
        interp.run(program)
    };
    if trace {
        let timeline = interp.trace_timeline();
        if timeline.is_empty() {
            eprintln!("\n--- execution trace: (nothing traceable ran) ---");
        } else {
            eprintln!("\n--- execution trace ---\n{timeline}");
        }
    }
    if let Err(e) = result {
        anyhow::bail!("runtime error: {e}");
    }
    Ok(())
}

/// A minimal, real, interactive step debugger: pauses before every
/// top-level statement, printing its source line, and waits for a
/// command on stdin. Deliberately scoped to top-level-statement
/// granularity (not per-expression, and not inside function bodies/loop
/// iterations) — real breakpoints, step-into, and call-stack inspection
/// need the interpreter to yield control mid-execution, which the current
/// tree-walk `exec_stmt`/`eval_expr` recursion doesn't do (it's plain
/// recursive Rust function calls, not a resumable state machine). What
/// ships here is genuinely interactive and useful for a linear script,
/// via [`ailang_interpreter::Interpreter::exec_top_level`]; deeper
/// stepping is future work (see docs/roadmap.md), not faked here.
///
/// Commands: `n`/`next` (default — run this statement, stop before the
/// next), `c`/`continue` (run every remaining statement without
/// stopping), `p <name>`/`print <name>` (show a global variable's current
/// value without advancing), `q`/`quit` (stop early, successfully).
fn run_debug_session<W: std::io::Write>(
    interp: &mut ailang_interpreter::Interpreter<W>,
    program: &Program,
) -> Result<(), ailang_interpreter::RuntimeError> {
    interp.prepare(program);
    let mut running_freely = false;
    let stdin = io::stdin();

    for (i, stmt) in program.statements.iter().enumerate() {
        if !running_freely {
            loop {
                eprint!(
                    "[{}/{}] (n/c/p <var>/q) > ",
                    i + 1,
                    program.statements.len()
                );
                io::stderr().flush().ok();
                let mut line = String::new();
                if stdin.lock().read_line(&mut line).unwrap_or(0) == 0 {
                    running_freely = true; // stdin closed (e.g. piped/non-interactive): just run
                    break;
                }
                match line.trim() {
                    "n" | "next" | "" => break,
                    "c" | "continue" => {
                        running_freely = true;
                        break;
                    }
                    "q" | "quit" => return Ok(()),
                    other if other.starts_with('p') => {
                        let var = other
                            .trim_start_matches("print")
                            .trim_start_matches('p')
                            .trim();
                        match interp.global_value(var) {
                            Some(v) => eprintln!("{var} = {v}"),
                            None => eprintln!("(undefined: {var})"),
                        }
                    }
                    other => eprintln!("unknown command `{other}` (try n, c, p <var>, or q)"),
                }
            }
        }
        interp.exec_top_level(stmt)?;
    }
    Ok(())
}

fn cmd_fmt(file: &Path, write: bool) -> anyhow::Result<()> {
    let source = fs::read_to_string(file)
        .map_err(|e| anyhow::anyhow!("could not read {}: {e}", file.display()))?;
    let (program, diags) = ailang_parser::parse(&source);
    if diags.has_errors() {
        print_diags(&diags, &file.display().to_string(), &source);
        anyhow::bail!("cannot format a file with syntax errors");
    }
    let formatted = ailang_fmt::format_program(&program.unwrap());
    if write {
        fs::write(file, &formatted)?;
        println!("formatted {}", file.display());
    } else {
        print!("{formatted}");
    }
    Ok(())
}

fn cmd_repl() -> anyhow::Result<()> {
    ailang_repl::run()
}

fn cmd_test(path: PathBuf) -> anyhow::Result<()> {
    if !path.exists() {
        anyhow::bail!("test path {} does not exist", path.display());
    }
    let mut files = Vec::new();
    collect_ail_files(&path, &mut files)?;
    files.sort();

    if files.is_empty() {
        println!("no .ail test files found under {}", path.display());
        return Ok(());
    }

    let mut passed = 0;
    let mut failed = 0;
    for f in &files {
        print!("test {} ... ", f.display());
        io::stdout().flush().ok();
        match compile(f)? {
            Some((_, program)) => {
                let mut sink: Vec<u8> = Vec::new();
                let mut interp = ailang_interpreter::Interpreter::new(&mut sink);
                match interp.run(&program) {
                    Ok(()) => {
                        println!("ok");
                        passed += 1;
                    }
                    Err(e) => {
                        println!("FAILED\n    {e}");
                        failed += 1;
                    }
                }
            }
            None => {
                println!("FAILED (compile error)");
                failed += 1;
            }
        }
    }

    println!("\ntest result: {} passed; {} failed", passed, failed);
    if failed > 0 {
        anyhow::bail!("{failed} test(s) failed");
    }
    Ok(())
}

fn collect_ail_files(dir: &Path, out: &mut Vec<PathBuf>) -> anyhow::Result<()> {
    if dir.is_file() {
        if dir.extension().and_then(|e| e.to_str()) == Some("ail") {
            out.push(dir.to_path_buf());
        }
        return Ok(());
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let p = entry.path();
        if p.is_dir() {
            collect_ail_files(&p, out)?;
        } else if p.extension().and_then(|e| e.to_str()) == Some("ail") {
            out.push(p);
        }
    }
    Ok(())
}

fn cmd_init() -> anyhow::Result<()> {
    ailang_pm::init_manifest(&std::env::current_dir()?)
}

fn cmd_new(name: &str) -> anyhow::Result<()> {
    ailang_pm::new_project(name)
}

fn cmd_deploy(target: &str) -> anyhow::Result<()> {
    let cwd = std::env::current_dir()?;
    let manifest = ailang_pm::bundle_for_deploy(&cwd, target)?;
    println!(
        "packaged `{}` v{} for target `{}` -> dist/{}/",
        manifest.package, manifest.version, manifest.target, manifest.target
    );
    for f in &manifest.files {
        println!("  {}", f.relative_path);
    }
    Ok(())
}
