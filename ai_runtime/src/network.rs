//! Distributed Agent Network foundation: real membership bookkeeping and
//! a real, deterministic leader-election algorithm — genuinely computed,
//! not hardcoded, but a **single-process simulation** of leader election
//! (no real distributed consensus protocol like Raft/Paxos, since there's
//! only one process here to have consensus *across*). See
//! `ailang-interpreter`'s module doc for the full honesty note on what
//! "network"/"cluster" mean in this release.

/// Deterministically picks a "leader" from `members`: the
/// lexicographically smallest name. This is a real, computed,
/// reproducible result — not a random or hardcoded pick — and is a
/// reasonable single-process stand-in for what a real leader-election
/// protocol (bully algorithm, Raft) would need actual multi-node
/// communication to decide. Returns `None` for an empty membership list.
pub fn elect_leader(members: &[String]) -> Option<String> {
    members.iter().min().cloned()
}

#[derive(Debug, Clone, PartialEq)]
pub struct HealthStatus {
    pub member: String,
    /// Always `true` in this release: every member is a real, currently-
    /// declared agent in the same process, so there's nothing to actually
    /// be unhealthy in the way a real distributed system's health check
    /// (network partition, crashed node) would detect. Kept as a real
    /// field (not hardcoded into the return type) so a future release
    /// that adds real inter-process agents has somewhere to report actual
    /// health into.
    pub healthy: bool,
}

pub fn health_check(members: &[String]) -> Vec<HealthStatus> {
    members
        .iter()
        .map(|m| HealthStatus {
            member: m.clone(),
            healthy: true,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elects_the_lexicographically_smallest_member() {
        let members = vec![
            "Reviewer".to_string(),
            "Engineer".to_string(),
            "Planner".to_string(),
        ];
        assert_eq!(elect_leader(&members), Some("Engineer".to_string()));
    }

    #[test]
    fn election_is_deterministic_across_calls() {
        let members = vec!["B".to_string(), "A".to_string()];
        assert_eq!(elect_leader(&members), elect_leader(&members));
    }

    #[test]
    fn election_returns_none_for_empty_membership() {
        assert_eq!(elect_leader(&[]), None);
    }

    #[test]
    fn health_check_reports_every_member() {
        let members = vec!["A".to_string(), "B".to_string()];
        let statuses = health_check(&members);
        assert_eq!(statuses.len(), 2);
        assert!(statuses.iter().all(|s| s.healthy));
    }
}
