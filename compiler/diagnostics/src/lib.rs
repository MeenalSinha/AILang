//! # ailang-diagnostics
//!
//! Shared diagnostic and source-location primitives used across every phase
//! of the AILang toolchain (lexer, parser, semantic analyzer, type checker,
//! interpreter). Centralizing these types means every phase can produce
//! diagnostics that render consistently, with line/column info, source
//! highlighting, and suggestions, via [`ariadne`].

use std::fmt;
use std::ops::Range;

use serde::{Deserialize, Serialize};

/// A byte-offset range into a single source file.
///
/// Kept intentionally tiny and `Copy` so every AST node can carry one
/// without meaningfully increasing memory usage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    /// Merge two spans into the smallest span that contains both.
    pub fn merge(self, other: Span) -> Span {
        Span {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }
}

impl From<Range<usize>> for Span {
    fn from(r: Range<usize>) -> Self {
        Span::new(r.start, r.end)
    }
}

impl From<Span> for Range<usize> {
    fn from(s: Span) -> Self {
        s.start..s.end
    }
}

/// Human-friendly `line:column` location, computed on demand from a `Span`
/// plus the original source text (line/column are not stored on every AST
/// node to keep them cheap; they're derived only when a diagnostic needs to
/// be rendered).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineCol {
    pub line: usize,   // 1-indexed
    pub column: usize, // 1-indexed
}

pub fn line_col(source: &str, byte_offset: usize) -> LineCol {
    let mut line = 1;
    let mut col = 1;
    for (i, ch) in source.char_indices() {
        if i >= byte_offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    LineCol { line, column: col }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Severity {
    Error,
    Warning,
    Note,
    Help,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Severity::Error => write!(f, "error"),
            Severity::Warning => write!(f, "warning"),
            Severity::Note => write!(f, "note"),
            Severity::Help => write!(f, "help"),
        }
    }
}

/// A single, structured compiler diagnostic.
///
/// Every phase of AILang (lexer, parser, semantic analyzer, type checker,
/// interpreter) produces `Diagnostic`s rather than ad-hoc `String`s so the
/// CLI/LSP/REPL can all share one rendering path.
#[derive(Debug, Clone, Serialize)]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: &'static str,
    pub message: String,
    pub span: Span,
    pub label: Option<String>,
    pub help: Option<String>,
    pub notes: Vec<String>,
}

impl Diagnostic {
    pub fn error(code: &'static str, message: impl Into<String>, span: Span) -> Self {
        Self {
            severity: Severity::Error,
            code,
            message: message.into(),
            span,
            label: None,
            help: None,
            notes: Vec::new(),
        }
    }

    pub fn warning(code: &'static str, message: impl Into<String>, span: Span) -> Self {
        Self {
            severity: Severity::Warning,
            code,
            message: message.into(),
            span,
            label: None,
            help: None,
            notes: Vec::new(),
        }
    }

    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }

    /// Render this diagnostic as a rich, source-highlighted report using
    /// `ariadne`. Falls back gracefully if the span is out of bounds.
    pub fn render(&self, filename: &str, source: &str) -> String {
        use ariadne::{Color, Label, Report, ReportKind, Source};

        let kind = match self.severity {
            Severity::Error => ReportKind::Error,
            Severity::Warning => ReportKind::Warning,
            Severity::Note => ReportKind::Advice,
            Severity::Help => ReportKind::Advice,
        };

        let color = match self.severity {
            Severity::Error => Color::Red,
            Severity::Warning => Color::Yellow,
            _ => Color::Blue,
        };

        let start = self.span.start.min(source.len());
        let end = self.span.end.min(source.len()).max(start);

        let mut report = Report::build(kind, filename, start)
            .with_code(self.code)
            .with_message(&self.message)
            .with_label(
                Label::new((filename, start..end.max(start + 1).min(source.len().max(1))))
                    .with_message(self.label.clone().unwrap_or_else(|| self.message.clone()))
                    .with_color(color),
            );

        for note in &self.notes {
            report = report.with_note(note);
        }
        if let Some(help) = &self.help {
            report = report.with_help(help);
        }

        let mut buf = Vec::new();
        let cache = (filename, Source::from(source));
        if report.finish().write(cache, &mut buf).is_ok() {
            String::from_utf8_lossy(&buf).into_owned()
        } else {
            // Fallback plain-text rendering (used e.g. for degenerate spans).
            let lc = line_col(source, start);
            format!(
                "{}[{}]: {}\n  --> {}:{}:{}\n",
                self.severity, self.code, self.message, filename, lc.line, lc.column
            )
        }
    }

    /// Plain non-fancy rendering, useful for tests where terminal colors
    /// would make snapshots noisy.
    pub fn render_plain(&self, filename: &str, source: &str) -> String {
        let lc = line_col(source, self.span.start.min(source.len()));
        let mut s = format!(
            "{}[{}]: {}\n  --> {}:{}:{}",
            self.severity, self.code, self.message, filename, lc.line, lc.column
        );
        if let Some(help) = &self.help {
            s.push_str(&format!("\n  help: {help}"));
        }
        for note in &self.notes {
            s.push_str(&format!("\n  note: {note}"));
        }
        s
    }
}

/// A bag of diagnostics accumulated during a compilation phase.
#[derive(Debug, Clone, Default, Serialize)]
pub struct DiagnosticBag {
    pub diagnostics: Vec<Diagnostic>,
}

impl DiagnosticBag {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, d: Diagnostic) {
        self.diagnostics.push(d);
    }

    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error)
    }

    pub fn error_count(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .count()
    }

    pub fn warning_count(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Warning)
            .count()
    }

    pub fn extend(&mut self, other: DiagnosticBag) {
        self.diagnostics.extend(other.diagnostics);
    }

    pub fn render_all(&self, filename: &str, source: &str) -> String {
        self.diagnostics
            .iter()
            .map(|d| d.render(filename, source))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_col_basic() {
        let src = "let x = 1;\nlet y = 2;";
        let lc = line_col(src, 11); // start of second line
        assert_eq!(lc.line, 2);
        assert_eq!(lc.column, 1);
    }

    #[test]
    fn diagnostic_bag_counts_errors_and_warnings() {
        let mut bag = DiagnosticBag::new();
        bag.push(Diagnostic::error("E001", "bad thing", Span::new(0, 1)));
        bag.push(Diagnostic::warning("W001", "meh thing", Span::new(2, 3)));
        assert!(bag.has_errors());
        assert_eq!(bag.error_count(), 1);
        assert_eq!(bag.warning_count(), 1);
    }
}
