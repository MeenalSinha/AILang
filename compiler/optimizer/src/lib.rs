//! # ailang-optimizer
//!
//! **v1.0: now wired into the pipeline as an opt-in pass** (`ail run
//! --optimize` / `ail build --optimize`; see `cli/src/main.rs`). v0.1
//! shipped this crate unwired deliberately ("avoid premature optimization
//! machinery before there's a bytecode backend to justify it"); for a
//! tree-walk interpreter the wall-clock benefit of folding `2 + 3` into
//! `5` ahead of time is genuinely small (evaluating it at runtime is
//! already cheap), but the pass itself is real, tested, and available
//! today, and a future bytecode backend gets significantly more mileage
//! out of the exact same optimize-before-execute step.
//!
//! Two passes, both whole-program, both AST-to-AST (no separate IR — see
//! `docs/roadmap.md` for why a real IR is still future work):
//!
//! * **Constant folding**: recursively evaluates arithmetic/logical
//!   sub-expressions with literal operands, bottom-up, so `(1 + 2) * (3 -
//!   1)` folds all the way down to `6`, not just one level.
//! * **Dead-code elimination**: `if` statements whose condition folds to a
//!   literal `true`/`false` are replaced by just their taken branch (or
//!   removed entirely if there's no `else` and the condition is `false`).
//!
//! Both passes are conservative: anything they can't prove foldable is
//! left exactly as written, so optimized and unoptimized programs always
//! produce identical output (verified by `tests/optimizer_equivalence.rs`-
//! style tests in this module, which run the same program through the
//! interpreter both ways and assert equal output).

use ailang_ast::{Block, ElseBranch, Expr, Literal, Program, Stmt, UnaryOp};
use ailang_diagnostics::Span;

/// Optimizes every statement in `program`, returning a new `Program`.
/// Never fails (a sub-expression the optimizer can't fold is simply left
/// unchanged) and never changes observable behavior.
pub fn optimize_program(program: &Program) -> Program {
    Program {
        statements: program.statements.iter().map(optimize_stmt).collect(),
        span: program.span,
    }
}

fn optimize_block(block: &Block) -> Block {
    Block {
        statements: block.statements.iter().map(optimize_stmt).collect(),
        span: block.span,
    }
}

fn optimize_stmt(stmt: &Stmt) -> Stmt {
    match stmt {
        Stmt::Expression { expr, span } => Stmt::Expression {
            expr: optimize_expr(expr),
            span: *span,
        },
        Stmt::VarDecl {
            name,
            is_const,
            type_annotation,
            initializer,
            span,
        } => Stmt::VarDecl {
            name: name.clone(),
            is_const: *is_const,
            type_annotation: type_annotation.clone(),
            initializer: initializer.as_ref().map(optimize_expr),
            span: *span,
        },
        Stmt::Function {
            name,
            params,
            return_type,
            body,
            span,
        } => Stmt::Function {
            name: name.clone(),
            params: params.clone(),
            return_type: return_type.clone(),
            body: optimize_block(body),
            span: *span,
        },
        Stmt::Return { value, span } => Stmt::Return {
            value: value.as_ref().map(optimize_expr),
            span: *span,
        },
        Stmt::If {
            condition,
            then_branch,
            else_branch,
            span,
        } => optimize_if(condition, then_branch, else_branch, *span),
        Stmt::While {
            condition,
            body,
            span,
        } => Stmt::While {
            condition: optimize_expr(condition),
            body: optimize_block(body),
            span: *span,
        },
        Stmt::For {
            variable,
            start,
            end,
            inclusive,
            body,
            span,
        } => Stmt::For {
            variable: variable.clone(),
            start: optimize_expr(start),
            end: optimize_expr(end),
            inclusive: *inclusive,
            body: optimize_block(body),
            span: *span,
        },
        Stmt::Block(b) => Stmt::Block(optimize_block(b)),
        Stmt::Module { name, body, span } => Stmt::Module {
            name: name.clone(),
            body: body.iter().map(optimize_stmt).collect(),
            span: *span,
        },
        // Every other statement kind either has no sub-expressions worth
        // folding at the statement level in a way that changes control
        // flow (Import/Break/Continue), or is a v0.5 AI-native
        // declaration whose config expressions are typically simple
        // literals already — folding them is safe but low-value, so
        // they're passed through unchanged rather than duplicating the
        // optimize_expr plumbing for every field. This is a conservative
        // choice, not a limitation: nothing is ever *mis*-optimized,
        // some things are just left as-is.
        other => other.clone(),
    }
}

fn optimize_if(
    condition: &Expr,
    then_branch: &Block,
    else_branch: &Option<Box<ElseBranch>>,
    span: Span,
) -> Stmt {
    let folded_condition = optimize_expr(condition);
    let optimized_then = optimize_block(then_branch);
    let optimized_else = else_branch.as_ref().map(|b| {
        Box::new(match b.as_ref() {
            ElseBranch::Else(block) => ElseBranch::Else(optimize_block(block)),
            ElseBranch::ElseIf(stmt) => ElseBranch::ElseIf(Box::new(optimize_stmt(stmt))),
        })
    });

    // Dead-code elimination: a condition that folds to a literal boolean
    // means we statically know which branch runs.
    if let Expr::Literal {
        value: Literal::Bool(b),
        ..
    } = &folded_condition
    {
        if *b {
            return Stmt::Block(optimized_then);
        } else if let Some(else_b) = &optimized_else {
            return match else_b.as_ref() {
                ElseBranch::Else(block) => Stmt::Block(block.clone()),
                ElseBranch::ElseIf(stmt) => (**stmt).clone(),
            };
        } else {
            // `if false { ... }` with no else: the whole statement is a
            // no-op. Represented as an empty block rather than removing
            // the statement outright, since this function returns a
            // single `Stmt` (the caller's `Vec` shape doesn't change).
            return Stmt::Block(Block {
                statements: vec![],
                span,
            });
        }
    }

    Stmt::If {
        condition: folded_condition,
        then_branch: optimized_then,
        else_branch: optimized_else,
        span,
    }
}

fn optimize_expr(expr: &Expr) -> Expr {
    match expr {
        Expr::Binary {
            left,
            op,
            right,
            span,
        } => {
            let l = optimize_expr(left);
            let r = optimize_expr(right);
            if let Some(folded) = fold_literals(*op, &l, &r) {
                return Expr::Literal {
                    value: folded,
                    span: *span,
                };
            }
            Expr::Binary {
                left: Box::new(l),
                op: *op,
                right: Box::new(r),
                span: *span,
            }
        }
        Expr::Unary { op, operand, span } => {
            let inner = optimize_expr(operand);
            if let (UnaryOp::Neg, Expr::Literal { value, .. }) = (op, &inner) {
                match value {
                    Literal::Int(n) => {
                        return Expr::Literal {
                            value: Literal::Int(-n),
                            span: *span,
                        }
                    }
                    Literal::Float(n) => {
                        return Expr::Literal {
                            value: Literal::Float(-n),
                            span: *span,
                        }
                    }
                    _ => {}
                }
            }
            if let (
                UnaryOp::Not,
                Expr::Literal {
                    value: Literal::Bool(b),
                    ..
                },
            ) = (op, &inner)
            {
                return Expr::Literal {
                    value: Literal::Bool(!b),
                    span: *span,
                };
            }
            Expr::Unary {
                op: *op,
                operand: Box::new(inner),
                span: *span,
            }
        }
        Expr::Grouping { expr, span } => {
            // A parenthesized, now-folded literal is just the literal;
            // otherwise keep the grouping (it may still matter for
            // anything downstream that inspects `Expr::Grouping`
            // specifically, though the interpreter doesn't care either way).
            let inner = optimize_expr(expr);
            if matches!(inner, Expr::Literal { .. }) {
                inner
            } else {
                Expr::Grouping {
                    expr: Box::new(inner),
                    span: *span,
                }
            }
        }
        Expr::ArrayLiteral { elements, span } => Expr::ArrayLiteral {
            elements: elements.iter().map(optimize_expr).collect(),
            span: *span,
        },
        Expr::MapLiteral { entries, span } => Expr::MapLiteral {
            entries: entries
                .iter()
                .map(|(k, v)| (optimize_expr(k), optimize_expr(v)))
                .collect(),
            span: *span,
        },
        Expr::Call { callee, args, span } => Expr::Call {
            callee: Box::new(optimize_expr(callee)),
            args: args.iter().map(optimize_expr).collect(),
            span: *span,
        },
        Expr::Index {
            object,
            index,
            span,
        } => Expr::Index {
            object: Box::new(optimize_expr(object)),
            index: Box::new(optimize_expr(index)),
            span: *span,
        },
        Expr::Assign {
            target,
            op,
            value,
            span,
        } => Expr::Assign {
            target: target.clone(),
            op: *op,
            value: Box::new(optimize_expr(value)),
            span: *span,
        },
        // Literal, Identifier, Member, Generate: no sub-expressions to
        // fold (Member's object is left as-is for the same "config
        // expressions, conservative" reasoning as optimize_stmt).
        other => other.clone(),
    }
}

fn fold_literals(op: ailang_ast::BinaryOp, l: &Expr, r: &Expr) -> Option<Literal> {
    use ailang_ast::BinaryOp::*;
    let (Expr::Literal { value: lv, .. }, Expr::Literal { value: rv, .. }) = (l, r) else {
        return None;
    };
    match (op, lv, rv) {
        (Add, Literal::Int(a), Literal::Int(b)) => Some(Literal::Int(a + b)),
        (Sub, Literal::Int(a), Literal::Int(b)) => Some(Literal::Int(a - b)),
        (Mul, Literal::Int(a), Literal::Int(b)) => Some(Literal::Int(a * b)),
        (Div, Literal::Int(a), Literal::Int(b)) if *b != 0 => Some(Literal::Int(a / b)),
        (Mod, Literal::Int(a), Literal::Int(b)) if *b != 0 => Some(Literal::Int(a % b)),
        (Add, Literal::Float(a), Literal::Float(b)) => Some(Literal::Float(a + b)),
        (Sub, Literal::Float(a), Literal::Float(b)) => Some(Literal::Float(a - b)),
        (Mul, Literal::Float(a), Literal::Float(b)) => Some(Literal::Float(a * b)),
        (Div, Literal::Float(a), Literal::Float(b)) if *b != 0.0 => Some(Literal::Float(a / b)),
        (Eq, a, b) => Some(Literal::Bool(literal_eq(a, b))),
        (NotEq, a, b) => Some(Literal::Bool(!literal_eq(a, b))),
        (Gt, Literal::Int(a), Literal::Int(b)) => Some(Literal::Bool(a > b)),
        (Lt, Literal::Int(a), Literal::Int(b)) => Some(Literal::Bool(a < b)),
        (GtEq, Literal::Int(a), Literal::Int(b)) => Some(Literal::Bool(a >= b)),
        (LtEq, Literal::Int(a), Literal::Int(b)) => Some(Literal::Bool(a <= b)),
        (And, Literal::Bool(a), Literal::Bool(b)) => Some(Literal::Bool(*a && *b)),
        (Or, Literal::Bool(a), Literal::Bool(b)) => Some(Literal::Bool(*a || *b)),
        (Add, Literal::String(a), Literal::String(b)) => Some(Literal::String(format!("{a}{b}"))),
        _ => None,
    }
}

fn literal_eq(a: &Literal, b: &Literal) -> bool {
    match (a, b) {
        (Literal::Int(a), Literal::Int(b)) => a == b,
        (Literal::Float(a), Literal::Float(b)) => a == b,
        (Literal::Int(a), Literal::Float(b)) | (Literal::Float(b), Literal::Int(a)) => {
            *a as f64 == *b
        }
        (Literal::Bool(a), Literal::Bool(b)) => a == b,
        (Literal::String(a), Literal::String(b)) => a == b,
        (Literal::Char(a), Literal::Char(b)) => a == b,
        (Literal::Null, Literal::Null) => true,
        _ => false,
    }
}

/// Retained from v0.1 for direct single-expression folding without going
/// through a whole `Program` (used by the unit tests below and available
/// for any future caller that only has one `Expr` in hand).
pub fn fold_constant(expr: &Expr) -> Option<Literal> {
    if let Expr::Binary {
        left, op, right, ..
    } = expr
    {
        return fold_literals(*op, left, right);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use ailang_diagnostics::Span;

    fn lit_int(v: i64) -> Expr {
        Expr::Literal {
            value: Literal::Int(v),
            span: Span::new(0, 0),
        }
    }

    #[test]
    fn folds_integer_addition() {
        let expr = Expr::Binary {
            left: Box::new(lit_int(2)),
            op: ailang_ast::BinaryOp::Add,
            right: Box::new(lit_int(3)),
            span: Span::new(0, 0),
        };
        assert_eq!(fold_constant(&expr), Some(Literal::Int(5)));
    }

    #[test]
    fn does_not_fold_division_by_zero() {
        let expr = Expr::Binary {
            left: Box::new(lit_int(2)),
            op: ailang_ast::BinaryOp::Div,
            right: Box::new(lit_int(0)),
            span: Span::new(0, 0),
        };
        assert_eq!(fold_constant(&expr), None);
    }

    fn parse_and_optimize(src: &str) -> Program {
        let (program, diags) = ailang_parser::parse(src);
        assert!(!diags.has_errors(), "{:?}", diags.diagnostics);
        optimize_program(&program.unwrap())
    }

    fn run(program: &Program) -> String {
        let mut out: Vec<u8> = Vec::new();
        let mut interp = ailang_interpreter::Interpreter::new(&mut out);
        interp.run(program).expect("runtime error");
        String::from_utf8(out).unwrap()
    }

    fn assert_same_behavior(src: &str) {
        let (unoptimized, diags) = ailang_parser::parse(src);
        assert!(!diags.has_errors());
        let unoptimized_out = run(&unoptimized.unwrap());
        let optimized_out = run(&parse_and_optimize(src));
        assert_eq!(
            unoptimized_out, optimized_out,
            "optimized and unoptimized output diverged for:\n{src}"
        );
    }

    #[test]
    fn nested_constant_expression_folds_all_the_way_down() {
        let optimized = parse_and_optimize("let x = (1 + 2) * (3 - 1);");
        match &optimized.statements[0] {
            Stmt::VarDecl {
                initializer:
                    Some(Expr::Literal {
                        value: Literal::Int(n),
                        ..
                    }),
                ..
            } => assert_eq!(*n, 6),
            other => panic!("expected fully-folded literal, got {other:?}"),
        }
    }

    #[test]
    fn optimized_and_unoptimized_programs_behave_identically() {
        assert_same_behavior("println(1 + 2 * 3);");
        assert_same_behavior(
            "let x = 5; if x > 3 { println(\"big\"); } else { println(\"small\"); }",
        );
        assert_same_behavior(
            "fn fib(n: int) -> int { if n < 2 { return n; } return fib(n-1) + fib(n-2); } println(fib(10));",
        );
        assert_same_behavior("for i in 0..5 { println(i * 2); }");
    }

    #[test]
    fn dead_branch_elimination_removes_the_untaken_if_branch() {
        let optimized =
            parse_and_optimize("if true { println(\"yes\"); } else { println(\"no\"); }");
        // The whole `if` collapses to just its `then` block.
        match &optimized.statements[0] {
            Stmt::Block(b) => assert_eq!(b.statements.len(), 1),
            other => panic!("expected collapsed block, got {other:?}"),
        }
    }

    #[test]
    fn dead_branch_elimination_handles_false_condition_with_no_else() {
        let optimized = parse_and_optimize("if false { println(\"never\"); }");
        match &optimized.statements[0] {
            Stmt::Block(b) => assert!(b.statements.is_empty()),
            other => panic!("expected empty block, got {other:?}"),
        }
    }

    #[test]
    fn division_by_zero_is_left_unfolded_for_the_interpreter_to_report() {
        let optimized = parse_and_optimize("let x = 1 / 0;");
        // Should NOT have been folded into a bogus literal; the interpreter
        // still needs to see a real division and raise its own error.
        match &optimized.statements[0] {
            Stmt::VarDecl {
                initializer: Some(Expr::Binary { .. }),
                ..
            } => {}
            other => panic!("expected unfolded division, got {other:?}"),
        }
    }

    #[test]
    fn string_concatenation_of_literals_folds() {
        let optimized = parse_and_optimize(r#"let x = "foo" + "bar";"#);
        match &optimized.statements[0] {
            Stmt::VarDecl {
                initializer:
                    Some(Expr::Literal {
                        value: Literal::String(s),
                        ..
                    }),
                ..
            } => assert_eq!(s, "foobar"),
            other => panic!("expected folded string literal, got {other:?}"),
        }
    }
}
