//! Golden/snapshot tests: run every deterministic example under
//! `examples/` through the real `ailang-parser` + `ailang-interpreter`
//! pipeline (the same path `ail run` takes) and compare captured stdout
//! byte-for-byte against a checked-in fixture in `tests/golden/<name>.out`.
//!
//! This intentionally avoids `insta` (see `docs/roadmap.md` for why: this
//! v0.1 build targets a stable, widely-available Rust toolchain, and
//! `insta`'s transitive dependency tree includes crates that require a
//! newer MSRV than that toolchain provides). The mechanism — fixture file
//! in, diff on mismatch, one `UPDATE_GOLDEN=1` env var to regenerate — is
//! the same idea `insta` provides; swapping in the real crate later is a
//! drop-in change to this one file.

use std::fs;
use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("cli/ has a parent directory")
        .to_path_buf()
}

fn run_example(dir: &str, name: &str) -> String {
    let root = workspace_root();
    let path = root.join(dir).join(format!("{name}.ail"));
    let source = fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {path:?}: {e}"));

    let (program, diags) = ailang_parser::parse(&source);
    assert!(
        !diags.has_errors(),
        "{name}: parse errors: {:?}",
        diags.diagnostics
    );
    let program = program.unwrap();

    let sem_diags = ailang_semantic::SemanticAnalyzer::new().analyze(&program);
    assert!(
        !sem_diags.has_errors(),
        "{name}: semantic errors: {:?}",
        sem_diags.diagnostics
    );

    let mut out: Vec<u8> = Vec::new();
    let mut interp = ailang_interpreter::Interpreter::new(&mut out);
    interp
        .run(&program)
        .unwrap_or_else(|e| panic!("{name}: runtime error: {e}"));

    String::from_utf8(out).unwrap()
}

fn check_golden(name: &str) {
    check_golden_in(name, "examples", "tests/golden")
}

fn check_golden_in(name: &str, examples_dir: &str, golden_dir: &str) {
    let actual = run_example(examples_dir, name);
    let golden_path = workspace_root()
        .join(golden_dir)
        .join(format!("{name}.out"));

    if std::env::var("UPDATE_GOLDEN").is_ok() {
        fs::write(&golden_path, &actual).unwrap();
        return;
    }

    let expected = fs::read_to_string(&golden_path)
        .unwrap_or_else(|e| panic!("reading golden fixture {golden_path:?}: {e}"));
    assert_eq!(
        actual, expected,
        "{name}: output does not match {golden_dir}/{name}.out\n\
         (run with UPDATE_GOLDEN=1 to regenerate the fixture if this change is intentional)"
    );
}

macro_rules! golden_test {
    ($fn_name:ident, $example:expr) => {
        #[test]
        fn $fn_name() {
            check_golden($example);
        }
    };
}

macro_rules! golden_test_v05 {
    ($fn_name:ident, $example:expr) => {
        #[test]
        fn $fn_name() {
            check_golden_in($example, "examples/v05", "tests/golden/v05");
        }
    };
}

macro_rules! golden_test_v10 {
    ($fn_name:ident, $example:expr) => {
        #[test]
        fn $fn_name() {
            check_golden_in($example, "examples/v10", "tests/golden/v10");
        }
    };
}

macro_rules! golden_test_v20 {
    ($fn_name:ident, $example:expr) => {
        #[test]
        fn $fn_name() {
            check_golden_in($example, "examples/v20", "tests/golden/v20");
        }
    };
}

macro_rules! golden_test_v30 {
    ($fn_name:ident, $example:expr) => {
        #[test]
        fn $fn_name() {
            check_golden_in($example, "examples/v30", "tests/golden/v30");
        }
    };
}

golden_test!(golden_hello_world, "01_hello_world");
golden_test!(golden_calculator, "02_calculator");
golden_test!(golden_loops, "03_loops");
golden_test!(golden_functions, "04_functions");
golden_test!(golden_arrays, "06_arrays");
golden_test!(golden_maps, "07_maps");
golden_test!(golden_recursion, "08_recursion");
golden_test!(golden_text_adventure, "09_text_adventure");
golden_test!(golden_expression_evaluator, "10_expression_evaluator");

// v0.5 AI-native examples. `09_permission_enforcement` (intentionally
// raises a runtime error to demonstrate enforcement) and
// `10_local_model_integration` (needs a real local model server listening
// on localhost:11434) are exercised manually/in documentation rather than
// as golden tests, since neither has a single deterministic
// always-passing outcome to snapshot in CI.
golden_test_v05!(golden_v05_hello_agent, "01_hello_agent");
golden_test_v05!(golden_v05_research_assistant, "02_research_assistant");
golden_test_v05!(
    golden_v05_multi_agent_collaboration,
    "03_multi_agent_collaboration"
);
golden_test_v05!(golden_v05_tool_usage, "04_tool_usage");
golden_test_v05!(golden_v05_memory_demonstration, "05_memory_demonstration");
golden_test_v05!(golden_v05_workflow_pipeline, "06_workflow_pipeline");
golden_test_v05!(golden_v05_parallel_tasks, "07_parallel_tasks");
golden_test_v05!(
    golden_v05_structured_output_schemas,
    "08_structured_output_schemas"
);

// v1.0 examples. `03_security_and_observability` reads `/etc/hostname`,
// whose content varies by machine, and `02_knowledge_retrieval_system`
// uses a `knowledge` source path relative to the repo root (as documented
// for running `ail run examples/v10/...` from there) which doesn't match
// this test binary's own working directory — both are exercised
// manually/in documentation and via `ailang-interpreter`'s own
// cwd-independent `retrieve_ranks_real_files_by_relevance` test instead.
golden_test_v10!(
    golden_v10_multi_agent_software_team,
    "01_multi_agent_software_team"
);
golden_test_v10!(golden_v10_ai_data_pipeline, "04_ai_data_pipeline");

golden_test_v20!(
    golden_v20_distributed_agent_network,
    "01_distributed_agent_network"
);
golden_test_v20!(
    golden_v20_agent_cluster_autoscaling,
    "02_agent_cluster_autoscaling"
);
golden_test_v20!(
    golden_v20_event_driven_and_messaging,
    "03_event_driven_and_messaging"
);
golden_test_v20!(
    golden_v20_context_and_vector_engine,
    "04_context_and_vector_engine"
);

golden_test_v30!(
    golden_v30_autonomous_research_org,
    "01_autonomous_research_org"
);
golden_test_v30!(
    golden_v30_ai_software_team,
    "02_ai_software_team"
);
golden_test_v30!(
    golden_v30_edge_ai_system,
    "03_edge_ai_system"
);
golden_test_v30!(
    golden_v30_multimodal_pipeline,
    "04_multimodal_pipeline"
);
golden_test_v30!(
    golden_v30_enterprise_knowledge_rag,
    "05_enterprise_knowledge_rag"
);
