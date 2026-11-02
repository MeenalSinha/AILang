//! A small, real, hand-rolled semantic-version parser and constraint
//! matcher — no external `semver` crate. This is a deliberate choice
//! consistent with the rest of this workspace's dependency discipline
//! (see the root README's toolchain note): semver parsing/comparison for
//! three-integer versions plus a handful of constraint operators is
//! genuinely simple to implement correctly, and doing so avoids one more
//! external dependency for a single-purpose need.
//!
//! Supports `MAJOR.MINOR.PATCH` versions (optionally with a `-prerelease`
//! suffix, compared lexicographically after the numeric parts — this
//! covers the common case but is not the full SemVer 2.0 prerelease
//! precedence algorithm) and constraint strings:
//!
//! * `1.2.3` — exact match
//! * `=1.2.3` — exact match (explicit form)
//! * `>=1.2.3` — greater than or equal
//! * `>1.2.3` — strictly greater
//! * `<=1.2.3` / `<1.2.3` — less than or equal / strictly less
//! * `~1.2.3` — "tilde": `>=1.2.3, <1.3.0` (patch-level updates only)
//! * `^1.2.3` — "caret": `>=1.2.3, <2.0.0` (updates that don't change the
//!   leftmost non-zero component — the same convention npm/Cargo use)

use std::cmp::Ordering;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    pub prerelease: Option<String>,
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if let Some(pre) = &self.prerelease {
            write!(f, "-{pre}")?;
        }
        Ok(())
    }
}

impl Version {
    pub fn parse(s: &str) -> Result<Version, String> {
        let (core, prerelease) = match s.split_once('-') {
            Some((core, pre)) => (core, Some(pre.to_string())),
            None => (s, None),
        };
        let parts: Vec<&str> = core.split('.').collect();
        if parts.len() != 3 {
            return Err(format!("invalid version `{s}`: expected MAJOR.MINOR.PATCH"));
        }
        let parse_component = |p: &str| {
            p.parse::<u64>()
                .map_err(|_| format!("invalid version `{s}`: `{p}` is not a non-negative integer"))
        };
        Ok(Version {
            major: parse_component(parts[0])?,
            minor: parse_component(parts[1])?,
            patch: parse_component(parts[2])?,
            prerelease,
        })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.major, self.minor, self.patch)
            .cmp(&(other.major, other.minor, other.patch))
            .then_with(|| {
                // A version WITHOUT a prerelease outranks one with —
                // matches standard SemVer precedence (1.0.0 > 1.0.0-rc1).
                match (&self.prerelease, &other.prerelease) {
                    (None, None) => Ordering::Equal,
                    (None, Some(_)) => Ordering::Greater,
                    (Some(_), None) => Ordering::Less,
                    (Some(a), Some(b)) => a.cmp(b),
                }
            })
    }
}

/// Checks whether `version` satisfies `constraint` (see this module's doc
/// comment for supported constraint syntax).
pub fn satisfies(version: &Version, constraint: &str) -> Result<bool, String> {
    let constraint = constraint.trim();
    if let Some(rest) = constraint.strip_prefix(">=") {
        return Ok(*version >= Version::parse(rest.trim())?);
    }
    if let Some(rest) = constraint.strip_prefix("<=") {
        return Ok(*version <= Version::parse(rest.trim())?);
    }
    if let Some(rest) = constraint.strip_prefix('>') {
        return Ok(*version > Version::parse(rest.trim())?);
    }
    if let Some(rest) = constraint.strip_prefix('<') {
        return Ok(*version < Version::parse(rest.trim())?);
    }
    if let Some(rest) = constraint.strip_prefix('=') {
        return Ok(*version == Version::parse(rest.trim())?);
    }
    if let Some(rest) = constraint.strip_prefix('~') {
        let base = Version::parse(rest.trim())?;
        let upper = Version {
            major: base.major,
            minor: base.minor + 1,
            patch: 0,
            prerelease: None,
        };
        return Ok(*version >= base && *version < upper);
    }
    if let Some(rest) = constraint.strip_prefix('^') {
        let base = Version::parse(rest.trim())?;
        let upper = if base.major > 0 {
            Version {
                major: base.major + 1,
                minor: 0,
                patch: 0,
                prerelease: None,
            }
        } else if base.minor > 0 {
            Version {
                major: 0,
                minor: base.minor + 1,
                patch: 0,
                prerelease: None,
            }
        } else {
            Version {
                major: 0,
                minor: 0,
                patch: base.patch + 1,
                prerelease: None,
            }
        };
        return Ok(*version >= base && *version < upper);
    }
    // Bare version string: exact match.
    Ok(*version == Version::parse(constraint)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_simple_version() {
        let v = Version::parse("1.2.3").unwrap();
        assert_eq!(
            v,
            Version {
                major: 1,
                minor: 2,
                patch: 3,
                prerelease: None
            }
        );
    }

    #[test]
    fn parses_a_prerelease_version() {
        let v = Version::parse("1.0.0-alpha").unwrap();
        assert_eq!(v.prerelease.as_deref(), Some("alpha"));
    }

    #[test]
    fn rejects_malformed_versions() {
        assert!(Version::parse("1.2").is_err());
        assert!(Version::parse("1.2.x").is_err());
        assert!(Version::parse("not-a-version").is_err());
    }

    #[test]
    fn orders_versions_correctly() {
        assert!(Version::parse("1.2.3").unwrap() < Version::parse("1.2.4").unwrap());
        assert!(Version::parse("1.2.3").unwrap() < Version::parse("1.3.0").unwrap());
        assert!(Version::parse("1.2.3").unwrap() < Version::parse("2.0.0").unwrap());
        assert!(Version::parse("2.0.0").unwrap() > Version::parse("1.99.99").unwrap());
    }

    #[test]
    fn release_outranks_prerelease() {
        assert!(Version::parse("1.0.0").unwrap() > Version::parse("1.0.0-rc1").unwrap());
    }

    #[test]
    fn exact_constraint_matches_only_that_version() {
        let v = Version::parse("1.2.3").unwrap();
        assert!(satisfies(&v, "1.2.3").unwrap());
        assert!(satisfies(&v, "=1.2.3").unwrap());
        assert!(!satisfies(&v, "1.2.4").unwrap());
    }

    #[test]
    fn gte_and_lte_constraints_work() {
        let v = Version::parse("1.5.0").unwrap();
        assert!(satisfies(&v, ">=1.0.0").unwrap());
        assert!(satisfies(&v, "<=2.0.0").unwrap());
        assert!(!satisfies(&v, ">=2.0.0").unwrap());
        assert!(!satisfies(&v, "<1.0.0").unwrap());
    }

    #[test]
    fn caret_constraint_allows_minor_and_patch_updates_but_not_major() {
        assert!(satisfies(&Version::parse("1.5.0").unwrap(), "^1.2.0").unwrap());
        assert!(satisfies(&Version::parse("1.2.0").unwrap(), "^1.2.0").unwrap());
        assert!(!satisfies(&Version::parse("2.0.0").unwrap(), "^1.2.0").unwrap());
        assert!(!satisfies(&Version::parse("1.1.0").unwrap(), "^1.2.0").unwrap());
    }

    #[test]
    fn caret_constraint_for_zero_major_only_allows_patch_updates() {
        // ^0.2.3 means >=0.2.3, <0.3.0 (npm/Cargo convention: leading zero
        // components are treated as significant for compatibility).
        assert!(satisfies(&Version::parse("0.2.5").unwrap(), "^0.2.3").unwrap());
        assert!(!satisfies(&Version::parse("0.3.0").unwrap(), "^0.2.3").unwrap());
    }

    #[test]
    fn tilde_constraint_allows_only_patch_updates() {
        assert!(satisfies(&Version::parse("1.2.9").unwrap(), "~1.2.0").unwrap());
        assert!(!satisfies(&Version::parse("1.3.0").unwrap(), "~1.2.0").unwrap());
    }
}
