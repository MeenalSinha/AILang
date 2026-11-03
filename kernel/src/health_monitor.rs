// kernel/src/health_monitor.rs
//! Self-diagnostic health monitor — the runtime behind `ail system doctor`.
//!
//! Every kernel subsystem registers itself and reports health; the monitor
//! aggregates component statuses into a system-wide view and surfaces
//! actionable diagnostics.
//!
//! This is the backing implementation for:
//!
//! ```bash
//! ail system doctor
//! ail system status
//! ```

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Health status of a single component.
#[derive(Debug, Clone, PartialEq)]
pub enum HealthStatus {
    Healthy,
    Degraded { reason: String },
    Critical { reason: String },
    Unknown,
}

impl HealthStatus {
    pub fn icon(&self) -> &'static str {
        match self {
            HealthStatus::Healthy => "✓",
            HealthStatus::Degraded { .. } => "⚠",
            HealthStatus::Critical { .. } => "✗",
            HealthStatus::Unknown => "?",
        }
    }

    pub fn label(&self) -> String {
        match self {
            HealthStatus::Healthy => "HEALTHY".to_string(),
            HealthStatus::Degraded { reason } => format!("DEGRADED ({})", reason),
            HealthStatus::Critical { reason } => format!("CRITICAL ({})", reason),
            HealthStatus::Unknown => "UNKNOWN".to_string(),
        }
    }
}

/// Snapshot of one component's health.
#[derive(Debug, Clone)]
pub struct ComponentHealth {
    pub name: String,
    pub status: HealthStatus,
    pub last_check: Instant,
    pub error_count: u64,
    pub warning_count: u64,
    pub metrics: HashMap<String, f64>,
    pub suggestion: Option<String>,
}

/// Severity of a health alert.
#[derive(Debug, Clone, PartialEq)]
pub enum AlertSeverity {
    Info,
    Warning,
    Critical,
}

/// A single health alert.
#[derive(Debug, Clone)]
pub struct HealthAlert {
    pub component: String,
    pub severity: AlertSeverity,
    pub message: String,
    pub suggestion: Option<String>,
    pub at: Instant,
}

/// The system health monitor.
pub struct HealthMonitor {
    components: Arc<Mutex<HashMap<String, ComponentHealth>>>,
    alerts: Arc<Mutex<Vec<HealthAlert>>>,
}

impl HealthMonitor {
    pub fn new() -> Self {
        Self {
            components: Arc::new(Mutex::new(HashMap::new())),
            alerts: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Register a component for monitoring.
    pub fn register(&self, name: &str) {
        self.components.lock().unwrap().insert(
            name.to_string(),
            ComponentHealth {
                name: name.to_string(),
                status: HealthStatus::Unknown,
                last_check: Instant::now(),
                error_count: 0,
                warning_count: 0,
                metrics: HashMap::new(),
                suggestion: None,
            },
        );
    }

    /// Report a component as healthy with optional metrics.
    pub fn report_healthy(&self, name: &str, metrics: HashMap<String, f64>) {
        let mut components = self.components.lock().unwrap();
        if let Some(c) = components.get_mut(name) {
            c.status = HealthStatus::Healthy;
            c.last_check = Instant::now();
            c.metrics = metrics;
            c.suggestion = None;
        }
    }

    /// Report a component as degraded.
    pub fn report_degraded(&self, name: &str, reason: &str, suggestion: Option<&str>) {
        {
            let mut components = self.components.lock().unwrap();
            if let Some(c) = components.get_mut(name) {
                c.status = HealthStatus::Degraded { reason: reason.to_string() };
                c.warning_count += 1;
                c.last_check = Instant::now();
                c.suggestion = suggestion.map(|s| s.to_string());
            }
        }
        self.alerts.lock().unwrap().push(HealthAlert {
            component: name.to_string(),
            severity: AlertSeverity::Warning,
            message: reason.to_string(),
            suggestion: suggestion.map(|s| s.to_string()),
            at: Instant::now(),
        });
    }

    /// Report a component as critically failed.
    pub fn report_critical(&self, name: &str, reason: &str, suggestion: Option<&str>) {
        {
            let mut components = self.components.lock().unwrap();
            if let Some(c) = components.get_mut(name) {
                c.status = HealthStatus::Critical { reason: reason.to_string() };
                c.error_count += 1;
                c.last_check = Instant::now();
                c.suggestion = suggestion.map(|s| s.to_string());
            }
        }
        self.alerts.lock().unwrap().push(HealthAlert {
            component: name.to_string(),
            severity: AlertSeverity::Critical,
            message: reason.to_string(),
            suggestion: suggestion.map(|s| s.to_string()),
            at: Instant::now(),
        });
    }

    /// Overall system health (worst single component drives the aggregate).
    pub fn overall_status(&self) -> HealthStatus {
        let components = self.components.lock().unwrap();
        if components.values().any(|c| matches!(c.status, HealthStatus::Critical { .. })) {
            let reason = components
                .values()
                .find(|c| matches!(c.status, HealthStatus::Critical { .. }))
                .map(|c| c.status.label())
                .unwrap_or_default();
            return HealthStatus::Critical { reason };
        }
        if components.values().any(|c| matches!(c.status, HealthStatus::Degraded { .. })) {
            return HealthStatus::Degraded {
                reason: "one or more components degraded".to_string(),
            };
        }
        if components.values().all(|c| c.status == HealthStatus::Healthy) {
            return HealthStatus::Healthy;
        }
        HealthStatus::Unknown
    }

    /// Generate the `ail system doctor` report.
    pub fn system_doctor_report(&self) -> String {
        let mut out = String::new();
        out.push_str("AILang System Doctor\n");
        out.push_str("════════════════════════════════════\n\n");
        out.push_str("Components:\n");

        {
            let components = self.components.lock().unwrap();
            let mut names: Vec<_> = components.keys().collect();
            names.sort();
            for name in names {
                let c = &components[name];
                out.push_str(&format!(
                    "  {} {:<30} {}\n",
                    c.status.icon(),
                    c.name,
                    c.status.label()
                ));
                if let Some(sug) = &c.suggestion {
                    out.push_str(&format!("      → {}\n", sug));
                }
            }
        }

        {
            let alerts = self.alerts.lock().unwrap();
            if !alerts.is_empty() {
                out.push_str("\nRecent Alerts:\n");
                for alert in alerts.iter().rev().take(10) {
                    let sev = match alert.severity {
                        AlertSeverity::Info => "INFO",
                        AlertSeverity::Warning => "WARN",
                        AlertSeverity::Critical => "CRIT",
                    };
                    out.push_str(&format!("  [{}] {}: {}\n", sev, alert.component, alert.message));
                    if let Some(sug) = &alert.suggestion {
                        out.push_str(&format!("       → {}\n", sug));
                    }
                }
            }
        }

        let overall = self.overall_status();
        out.push_str(&format!("\nOverall: {} {}\n", overall.icon(), overall.label()));
        out
    }

    pub fn component_status(&self, name: &str) -> Option<HealthStatus> {
        self.components.lock().unwrap().get(name).map(|c| c.status.clone())
    }

    pub fn all_components(&self) -> Vec<ComponentHealth> {
        self.components.lock().unwrap().values().cloned().collect()
    }

    pub fn recent_alerts(&self, limit: usize) -> Vec<HealthAlert> {
        let alerts = self.alerts.lock().unwrap();
        alerts.iter().rev().take(limit).cloned().collect()
    }
}

impl Default for HealthMonitor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn healthy_after_registration_and_report() {
        let m = HealthMonitor::new();
        m.register("compiler");
        m.report_healthy("compiler", HashMap::new());
        assert_eq!(m.component_status("compiler"), Some(HealthStatus::Healthy));
    }

    #[test]
    fn overall_critical_when_any_component_critical() {
        let m = HealthMonitor::new();
        m.register("compiler");
        m.register("runtime");
        m.report_healthy("compiler", HashMap::new());
        m.report_critical("runtime", "OOM", Some("reduce agent count"));
        assert!(matches!(m.overall_status(), HealthStatus::Critical { .. }));
    }

    #[test]
    fn degraded_status_triggers_alert() {
        let m = HealthMonitor::new();
        m.register("model_fabric");
        m.report_degraded("model_fabric", "primary model slow", Some("switch to fallback"));

        let alerts = m.recent_alerts(5);
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].severity, AlertSeverity::Warning);
        assert_eq!(alerts[0].component, "model_fabric");
    }

    #[test]
    fn system_doctor_report_includes_all_components() {
        let m = HealthMonitor::new();
        m.register("compiler");
        m.register("kernel");
        m.report_healthy("compiler", HashMap::new());
        m.report_critical("kernel", "panic", None);

        let report = m.system_doctor_report();
        assert!(report.contains("compiler"));
        assert!(report.contains("kernel"));
        assert!(report.contains("CRITICAL"));
        assert!(report.contains("HEALTHY"));
    }

    #[test]
    fn unknown_before_first_report() {
        let m = HealthMonitor::new();
        m.register("new_component");
        assert_eq!(m.component_status("new_component"), Some(HealthStatus::Unknown));
    }
}

