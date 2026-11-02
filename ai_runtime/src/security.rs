//! Security basics: safe secret access and an audit trail for tool
//! invocations.
//!
//! ## Scope
//!
//! This is deliberately **not** the full "Security Framework" the v1.0
//! brief describes (role-based permissions beyond the existing capability
//! names, a policy engine, capability tokens as a distinct type, supply-
//! chain / package signing, sandboxed execution below the process level).
//! Those need infrastructure (a policy DSL, cryptographic signing, OS-level
//! sandboxing) that's a genuinely separate project from a language
//! runtime shipped as source — see `docs/roadmap.md` for what's tracked
//! there. What ships here is real and load-bearing: [`get_secret`] is the
//! *only* way an AILang program can read the host process's environment
//! (there's no other env-var access anywhere in the standard library), and
//! [`AuditLog`] gives every `invoke_tool` call a real, inspectable record.

use std::cell::RefCell;
use std::time::{SystemTime, UNIX_EPOCH};

fn now_seconds() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// Reads environment variable `name`. Returns `None` if it isn't set.
/// This is the sole environment-variable entry point in the entire
/// standard library — deliberately, so that "where could a secret have
/// come from" always has exactly one answer in an AILang program: an
/// explicit `secret("NAME")` call, never an implicit or hidden read.
pub fn get_secret(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

/// One audit-log entry for a tool invocation. Deliberately excludes the
/// call's actual argument values: since there's no taint-tracking through
/// `ailang_runtime::Value` to know whether a given argument originated
/// from a `secret()` call, the safest design is to never log call
/// arguments at all, rather than risk logging a secret in plaintext. What
/// *is* logged (agent, tool, permission, and whether it was granted) is
/// exactly what a real audit trail needs to answer "did this agent try to
/// do this, and were they allowed to."
#[derive(Debug, Clone, PartialEq)]
pub struct AuditEntry {
    pub timestamp: f64,
    pub agent: String,
    pub tool: String,
    pub required_permission: Option<String>,
    pub granted: bool,
}

/// An in-process, append-only audit trail. Like `InMemoryStore`, this is
/// explicitly a v1.0 foundation (in-memory, not persisted across process
/// runs, no tamper-evidence/signing) — see `docs/roadmap.md`.
#[derive(Default)]
pub struct AuditLog {
    entries: RefCell<Vec<AuditEntry>>,
}

impl AuditLog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record(
        &self,
        agent: &str,
        tool: &str,
        required_permission: Option<&str>,
        granted: bool,
    ) {
        self.entries.borrow_mut().push(AuditEntry {
            timestamp: now_seconds(),
            agent: agent.to_string(),
            tool: tool.to_string(),
            required_permission: required_permission.map(|s| s.to_string()),
            granted,
        });
    }

    pub fn entries(&self) -> Vec<AuditEntry> {
        self.entries.borrow().clone()
    }

    pub fn denied_count(&self) -> usize {
        self.entries.borrow().iter().filter(|e| !e.granted).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_secret_reads_a_real_env_var() {
        std::env::set_var("AILANG_TEST_SECRET_XYZ", "s3cr3t");
        assert_eq!(
            get_secret("AILANG_TEST_SECRET_XYZ"),
            Some("s3cr3t".to_string())
        );
        std::env::remove_var("AILANG_TEST_SECRET_XYZ");
    }

    #[test]
    fn get_secret_returns_none_for_unset_var() {
        assert_eq!(get_secret("AILANG_DEFINITELY_NOT_SET_XYZ"), None);
    }

    #[test]
    fn audit_log_records_granted_and_denied_calls() {
        let log = AuditLog::new();
        log.record("Analyst", "Calculator", None, true);
        log.record("Sandboxed", "Filesystem", Some("filesystem"), false);

        let entries = log.entries();
        assert_eq!(entries.len(), 2);
        assert!(entries[0].granted);
        assert!(!entries[1].granted);
        assert_eq!(log.denied_count(), 1);
    }

    #[test]
    fn audit_log_starts_empty() {
        let log = AuditLog::new();
        assert!(log.entries().is_empty());
        assert_eq!(log.denied_count(), 0);
    }
}
