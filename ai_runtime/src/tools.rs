//! Built-in tools (`Calculator`, `Filesystem`, `HTTP`, `Search`,
//! `Browser`) plus the [`ToolRegistry`] that resolves a tool name to an
//! implementation and enforces the required capability against an agent's
//! [`PermissionSet`]. See the crate-level doc table for exactly which
//! tools are real vs. documented placeholders.

use std::fs;

use ailang_ast::{BinaryOp, Expr, Literal, UnaryOp};
use ailang_runtime::Value;

use crate::permission::PermissionSet;
use crate::providers::HttpModelProvider;

/// A tool an agent can invoke via `invoke_tool`. `required_permission`
/// returning `Some("internet")` means [`ToolRegistry::invoke`] will refuse
/// to call this tool unless the invoking agent's [`PermissionSet`] grants
/// `"internet"`.
pub trait Tool {
    fn name(&self) -> &str;
    fn required_permission(&self) -> Option<&'static str>;
    fn invoke(&self, args: &[Value]) -> Result<Value, String>;
}

/// A real calculator: evaluates an arithmetic expression string (e.g.
/// `"(2 + 3) * 4"`) using a small, self-contained recursive evaluator over
/// `ailang_ast::Expr` — reusing `ailang-parser` to parse the string, but
/// deliberately *not* depending on `ailang-interpreter` (which depends on
/// this crate), to keep the workspace's dependency graph a DAG. Supports
/// `+ - * /`, unary `-`, and parentheses; anything else (variables, calls,
/// comparisons, ...) is rejected with a clear error rather than silently
/// misinterpreted.
pub struct CalculatorTool;

impl Tool for CalculatorTool {
    fn name(&self) -> &str {
        "Calculator"
    }

    fn required_permission(&self) -> Option<&'static str> {
        None
    }

    fn invoke(&self, args: &[Value]) -> Result<Value, String> {
        let expr_str = match args.first() {
            Some(Value::String(s)) => s.as_str(),
            _ => {
                return Err(
                    "Calculator expects one string argument, e.g. \"(2 + 3) * 4\"".to_string(),
                )
            }
        };
        let (program, diags) = ailang_parser::parse(&format!("{expr_str};"));
        if diags.has_errors() {
            return Err(format!("could not parse expression `{expr_str}`"));
        }
        let program = program.ok_or("could not parse expression")?;
        let stmt = program
            .statements
            .first()
            .ok_or("expected a single expression")?;
        let ailang_ast::Stmt::Expression { expr, .. } = stmt else {
            return Err("expected a single arithmetic expression".to_string());
        };
        eval_arithmetic(expr).map(Value::Float)
    }
}

fn eval_arithmetic(expr: &Expr) -> Result<f64, String> {
    match expr {
        Expr::Literal {
            value: Literal::Int(n),
            ..
        } => Ok(*n as f64),
        Expr::Literal {
            value: Literal::Float(n),
            ..
        } => Ok(*n),
        Expr::Grouping { expr, .. } => eval_arithmetic(expr),
        Expr::Unary {
            op: UnaryOp::Neg,
            operand,
            ..
        } => Ok(-eval_arithmetic(operand)?),
        Expr::Binary {
            left, op, right, ..
        } => {
            let l = eval_arithmetic(left)?;
            let r = eval_arithmetic(right)?;
            match op {
                BinaryOp::Add => Ok(l + r),
                BinaryOp::Sub => Ok(l - r),
                BinaryOp::Mul => Ok(l * r),
                BinaryOp::Div => {
                    if r == 0.0 {
                        Err("division by zero".to_string())
                    } else {
                        Ok(l / r)
                    }
                }
                other => Err(format!("Calculator does not support operator {other:?}")),
            }
        }
        other => Err(format!("Calculator does not support expression {other:?}")),
    }
}

/// Reads a file from disk. Requires the `filesystem` permission.
pub struct FilesystemTool;

impl Tool for FilesystemTool {
    fn name(&self) -> &str {
        "Filesystem"
    }

    fn required_permission(&self) -> Option<&'static str> {
        Some("filesystem")
    }

    fn invoke(&self, args: &[Value]) -> Result<Value, String> {
        let path = match args.first() {
            Some(Value::String(s)) => s.as_str(),
            _ => return Err("Filesystem expects one string argument: a file path".to_string()),
        };
        fs::read_to_string(path)
            .map(Value::string)
            .map_err(|e| format!("could not read `{path}`: {e}"))
    }
}

/// A real HTTP GET, using the same std-only client as
/// [`crate::providers::HttpModelProvider`]. Requires the `internet`
/// permission. Only plain `http://` URLs are supported (see
/// `HttpModelProvider`'s doc comment for why).
pub struct HttpTool;

impl Tool for HttpTool {
    fn name(&self) -> &str {
        "HTTP"
    }

    fn required_permission(&self) -> Option<&'static str> {
        Some("internet")
    }

    fn invoke(&self, args: &[Value]) -> Result<Value, String> {
        let url = match args.first() {
            Some(Value::String(s)) => s.as_str(),
            _ => return Err("HTTP expects one string argument: a URL".to_string()),
        };
        HttpModelProvider::new(url).get().map(Value::string)
    }
}

/// **Documented placeholder.** A real web-search tool needs an external
/// search API (and, typically, an API key) that this offline-by-default
/// foundation deliberately doesn't assume access to. Returns a clearly
/// labeled "not available" result — real dispatch, permission checking,
/// and error-shape, just no fabricated search results.
pub struct SearchTool;

impl Tool for SearchTool {
    fn name(&self) -> &str {
        "Search"
    }

    fn required_permission(&self) -> Option<&'static str> {
        Some("internet")
    }

    fn invoke(&self, args: &[Value]) -> Result<Value, String> {
        let query = args.first().map(|v| v.to_string()).unwrap_or_default();
        Ok(Value::string(format!(
            "[Search tool placeholder: web search for \"{query}\" is not available in this build — integrate a real search API behind ailang_ai_runtime::tools::Tool to enable it]"
        )))
    }
}

/// **Documented placeholder.** A real browser tool needs a headless
/// browser (or at minimum a full HTML/JS rendering pipeline), which is out
/// of scope for this foundation.
pub struct BrowserTool;

impl Tool for BrowserTool {
    fn name(&self) -> &str {
        "Browser"
    }

    fn required_permission(&self) -> Option<&'static str> {
        Some("internet")
    }

    fn invoke(&self, args: &[Value]) -> Result<Value, String> {
        let url = args.first().map(|v| v.to_string()).unwrap_or_default();
        Ok(Value::string(format!(
            "[Browser tool placeholder: rendering \"{url}\" is not available in this build]"
        )))
    }
}

/// Resolves tool names (built-in or custom) to implementations and
/// enforces permissions. "Custom tools" (the spec's "support registration
/// of custom tools") are supported without any extra grammar: if a name
/// doesn't match a built-in, `invoke` reports that clearly, and
/// `ailang-interpreter`'s `invoke_tool` builtin falls back to calling a
/// same-named ordinary AILang function if one exists — see that crate's
/// module doc.
pub struct ToolRegistry;

impl ToolRegistry {
    /// Looks up a built-in tool by name (case-sensitive, matching the
    /// spec's examples: `Search`, `Browser`, `Calculator`, `Filesystem`,
    /// `HTTP`). Returns `None` for anything else, letting the caller (the
    /// interpreter) fall back to a user-defined function of the same name.
    pub fn builtin(name: &str) -> Option<Box<dyn Tool>> {
        match name {
            "Calculator" => Some(Box::new(CalculatorTool)),
            "Filesystem" => Some(Box::new(FilesystemTool)),
            "HTTP" => Some(Box::new(HttpTool)),
            "Search" => Some(Box::new(SearchTool)),
            "Browser" => Some(Box::new(BrowserTool)),
            _ => None,
        }
    }

    /// Checks a tool's required permission (if any) against `permissions`,
    /// then invokes it. Returns a clear error rather than invoking on a
    /// missing grant.
    pub fn invoke_checked(
        tool: &dyn Tool,
        permissions: &PermissionSet,
        args: &[Value],
    ) -> Result<Value, String> {
        if let Some(required) = tool.required_permission() {
            if !permissions.grants(required) {
                return Err(format!(
                    "tool `{}` requires the `{required}` permission, which was not granted",
                    tool.name()
                ));
            }
        }
        tool.invoke(args)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calculator_evaluates_arithmetic() {
        let result = CalculatorTool
            .invoke(&[Value::string("(2 + 3) * 4")])
            .unwrap();
        assert_eq!(result, Value::Float(20.0));
    }

    #[test]
    fn calculator_rejects_division_by_zero() {
        assert!(CalculatorTool.invoke(&[Value::string("1 / 0")]).is_err());
    }

    #[test]
    fn calculator_rejects_non_arithmetic_input() {
        assert!(CalculatorTool
            .invoke(&[Value::string("\"hello\"")])
            .is_err());
    }

    #[test]
    fn calculator_requires_no_permission() {
        assert_eq!(CalculatorTool.required_permission(), None);
    }

    #[test]
    fn filesystem_tool_reads_a_real_file() {
        let tmp =
            std::env::temp_dir().join(format!("ailang-fs-tool-test-{}.txt", std::process::id()));
        std::fs::write(&tmp, "hello from disk").unwrap();
        let result = FilesystemTool
            .invoke(&[Value::string(tmp.to_str().unwrap())])
            .unwrap();
        assert_eq!(result, Value::string("hello from disk"));
        std::fs::remove_file(&tmp).ok();
    }

    #[test]
    fn filesystem_tool_requires_filesystem_permission() {
        assert_eq!(FilesystemTool.required_permission(), Some("filesystem"));
    }

    #[test]
    fn filesystem_tool_reports_missing_file_clearly() {
        let result = FilesystemTool.invoke(&[Value::string("/nonexistent/path/xyz")]);
        assert!(result.is_err());
    }

    #[test]
    fn search_and_browser_tools_return_labeled_placeholders() {
        let s = SearchTool
            .invoke(&[Value::string("rust programming")])
            .unwrap();
        assert!(s.to_string().contains("placeholder"));
        let b = BrowserTool
            .invoke(&[Value::string("http://example.com")])
            .unwrap();
        assert!(b.to_string().contains("placeholder"));
    }

    #[test]
    fn tool_registry_resolves_builtins_by_name() {
        assert!(ToolRegistry::builtin("Calculator").is_some());
        assert!(ToolRegistry::builtin("Filesystem").is_some());
        assert!(ToolRegistry::builtin("HTTP").is_some());
        assert!(ToolRegistry::builtin("Search").is_some());
        assert!(ToolRegistry::builtin("Browser").is_some());
        assert!(ToolRegistry::builtin("NotARealTool").is_none());
    }

    #[test]
    fn invoke_checked_blocks_missing_permission() {
        let tool = FilesystemTool;
        let perms = PermissionSet::default();
        let result = ToolRegistry::invoke_checked(&tool, &perms, &[Value::string("/etc/hosts")]);
        assert!(result.unwrap_err().contains("filesystem"));
    }

    #[test]
    fn invoke_checked_allows_granted_permission() {
        let tmp = std::env::temp_dir().join(format!(
            "ailang-fs-tool-perm-test-{}.txt",
            std::process::id()
        ));
        std::fs::write(&tmp, "granted").unwrap();
        let tool = FilesystemTool;
        let perms = PermissionSet::new(vec!["filesystem".to_string()]);
        let result =
            ToolRegistry::invoke_checked(&tool, &perms, &[Value::string(tmp.to_str().unwrap())]);
        assert_eq!(result.unwrap(), Value::string("granted"));
        std::fs::remove_file(&tmp).ok();
    }

    #[test]
    fn invoke_checked_allows_permission_free_tools_unconditionally() {
        let perms = PermissionSet::default();
        let result =
            ToolRegistry::invoke_checked(&CalculatorTool, &perms, &[Value::string("1 + 1")]);
        assert_eq!(result.unwrap(), Value::Float(2.0));
    }
}
