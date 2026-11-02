//! # ailang-pm
//!
//! **Package manager foundation.** Implements the `ai.toml` manifest format
//! and local project scaffolding (`ail init`, `ail new`). Dependency
//! *resolution* is intentionally local-only in v0.1 (a project's
//! dependencies must point at local paths); the [`Manifest`] schema already
//! has a `dependencies` map keyed by name so a future registry client can
//! populate version requirements and a lockfile without changing this
//! struct's shape — see `docs/roadmap.md`.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

pub mod semver;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub name: String,
    pub version: String,
    pub edition: String,
    pub authors: Vec<String>,
    #[serde(default)]
    pub dependencies: std::collections::BTreeMap<String, DependencySpec>,
}

/// A dependency requirement. v0.1 only supports `{ path = "..." }` local
/// dependencies; `version`/`registry` fields are reserved for the future
/// public registry (see roadmap) and are accepted-but-unused today so
/// manifests written against the future schema don't fail to parse.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencySpec {
    pub path: Option<String>,
    pub version: Option<String>,
    pub registry: Option<String>,
}

impl Manifest {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            version: "0.1.0".to_string(),
            edition: "2026".to_string(),
            authors: vec!["AILang Team".to_string()],
            dependencies: Default::default(),
        }
    }

    pub fn to_toml_string(&self) -> anyhow::Result<String> {
        Ok(toml::to_string_pretty(self)?)
    }

    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let text = fs::read_to_string(path)?;
        Ok(toml::from_str(&text)?)
    }
}

/// `ail init`: write an `ai.toml` in `dir`, using the directory name as the
/// package name. Fails if `ai.toml` already exists.
pub fn init_manifest(dir: &Path) -> anyhow::Result<()> {
    let manifest_path = dir.join("ai.toml");
    if manifest_path.exists() {
        anyhow::bail!("ai.toml already exists in {}", dir.display());
    }
    let name = dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("ailang-project");
    let manifest = Manifest::new(name);
    fs::write(&manifest_path, manifest.to_toml_string()?)?;
    println!("created {}", manifest_path.display());
    Ok(())
}

/// `ail new <name>`: create a new directory `<name>/` with an `ai.toml`
/// and a starter `src/main.ail`.
pub fn new_project(name: &str) -> anyhow::Result<()> {
    let dir = Path::new(name);
    if dir.exists() {
        anyhow::bail!("directory `{name}` already exists");
    }
    fs::create_dir_all(dir.join("src"))?;

    let manifest = Manifest::new(name);
    fs::write(dir.join("ai.toml"), manifest.to_toml_string()?)?;

    let main_ail = "fn main() {\n    println(\"Hello from AILang!\");\n}\n\nmain();\n";
    fs::write(dir.join("src").join("main.ail"), main_ail)?;

    println!("created new AILang project `{name}`");
    println!("  {name}/ai.toml");
    println!("  {name}/src/main.ail");
    println!();
    println!("run it with:");
    println!("  cd {name} && ail run src/main.ail");
    Ok(())
}

/// A resolved dependency graph, written as `ai.lock` next to `ai.toml`.
///
/// v0.1's dependency resolution is local-path-only (no registry, no
/// version solving), so the lockfile's job today is narrower than a full
/// `Cargo.lock`/`package-lock.json`: it records exactly which local paths
/// were resolved for each dependency at the last successful `ail build` /
/// `ail check`, plus a content hash, so a future registry-backed resolver
/// has a file format to extend (adding `version`/`checksum-from-registry`
/// fields) rather than inventing one from scratch. This is the "lockfile
/// support" foundation called out in the v0.1 audit.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Lockfile {
    pub package: String,
    pub version: String,
    #[serde(default)]
    pub dependencies: std::collections::BTreeMap<String, LockedDependency>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockedDependency {
    /// Local path the dependency was resolved to (v0.1 supports only
    /// local-path dependencies; see `DependencySpec`).
    pub resolved_path: String,
    /// A simple, fast content fingerprint (not cryptographic) of the
    /// dependency's `ai.toml`, so `ail build` can warn if a locked
    /// dependency's manifest changed since the lockfile was written.
    pub content_hash: String,
}

/// A non-cryptographic FNV-1a hash, sufficient for "did this file change"
/// lockfile fingerprints without pulling in a hashing crate.
fn fingerprint(content: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in content.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

/// Resolve every dependency in `manifest` (local-path only, per v0.1) and
/// write `ai.lock` next to `manifest_dir/ai.toml`. Returns the resolved
/// lockfile so callers (e.g. `ail build`) can inspect it without a second
/// file read.
pub fn write_lockfile(manifest_dir: &Path, manifest: &Manifest) -> anyhow::Result<Lockfile> {
    let mut dependencies = std::collections::BTreeMap::new();
    for (name, spec) in &manifest.dependencies {
        let Some(rel_path) = &spec.path else {
            anyhow::bail!(
                "dependency `{name}` has no `path`; only local-path dependencies are supported in v0.1 (see docs/roadmap.md)"
            );
        };
        let dep_manifest_path = manifest_dir.join(rel_path).join("ai.toml");
        let content = fs::read_to_string(&dep_manifest_path).map_err(|e| {
            anyhow::anyhow!(
                "could not resolve dependency `{name}` at {}: {e}",
                dep_manifest_path.display()
            )
        })?;

        // v1.0: if the dependency spec gives a version constraint, verify
        // the dependency's own `ai.toml` actually satisfies it — real
        // semantic-version checking (see `semver.rs`), not just a
        // recorded-but-unenforced string like in v0.1/v0.5.
        if let Some(constraint) = &spec.version {
            let dep_manifest: Manifest = toml::from_str(&content).map_err(|e| {
                anyhow::anyhow!("could not parse manifest for dependency `{name}`: {e}")
            })?;
            let dep_version = semver::Version::parse(&dep_manifest.version)
                .map_err(|e| anyhow::anyhow!("dependency `{name}` has an invalid version: {e}"))?;
            let ok = semver::satisfies(&dep_version, constraint).map_err(|e| {
                anyhow::anyhow!(
                    "invalid version constraint `{constraint}` for dependency `{name}`: {e}"
                )
            })?;
            if !ok {
                anyhow::bail!(
                    "dependency `{name}` version {dep_version} does not satisfy constraint `{constraint}`"
                );
            }
        }

        dependencies.insert(
            name.clone(),
            LockedDependency {
                resolved_path: rel_path.clone(),
                content_hash: fingerprint(&content),
            },
        );
    }

    let lockfile = Lockfile {
        package: manifest.name.clone(),
        version: manifest.version.clone(),
        dependencies,
    };
    let toml_text = toml::to_string_pretty(&lockfile)?;
    fs::write(manifest_dir.join("ai.lock"), toml_text)?;
    Ok(lockfile)
}

#[cfg(test)]
mod lockfile_tests {
    use super::*;

    #[test]
    fn writes_and_reads_back_a_lockfile_with_no_dependencies() {
        let tmp = std::env::temp_dir().join(format!("ailang-pm-lock-test-{}", std::process::id()));
        fs::create_dir_all(&tmp).unwrap();
        let manifest = Manifest::new("demo");
        let lockfile = write_lockfile(&tmp, &manifest).unwrap();
        assert_eq!(lockfile.package, "demo");
        assert!(tmp.join("ai.lock").exists());
        fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn resolves_a_local_path_dependency() {
        let tmp =
            std::env::temp_dir().join(format!("ailang-pm-lock-dep-test-{}", std::process::id()));
        let dep_dir = tmp.join("libs/mathutils");
        fs::create_dir_all(&dep_dir).unwrap();
        fs::write(
            dep_dir.join("ai.toml"),
            Manifest::new("mathutils").to_toml_string().unwrap(),
        )
        .unwrap();

        let mut manifest = Manifest::new("demo");
        manifest.dependencies.insert(
            "mathutils".to_string(),
            DependencySpec {
                path: Some("libs/mathutils".to_string()),
                version: None,
                registry: None,
            },
        );

        let lockfile = write_lockfile(&tmp, &manifest).unwrap();
        let locked = lockfile.dependencies.get("mathutils").unwrap();
        assert_eq!(locked.resolved_path, "libs/mathutils");
        assert!(!locked.content_hash.is_empty());

        fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn accepts_a_local_dependency_satisfying_its_version_constraint() {
        let tmp = std::env::temp_dir().join(format!("ailang-pm-semver-ok-{}", std::process::id()));
        let dep_dir = tmp.join("libs/mathutils");
        fs::create_dir_all(&dep_dir).unwrap();
        let mut dep_manifest = Manifest::new("mathutils");
        dep_manifest.version = "1.5.0".to_string();
        fs::write(
            dep_dir.join("ai.toml"),
            dep_manifest.to_toml_string().unwrap(),
        )
        .unwrap();

        let mut manifest = Manifest::new("demo");
        manifest.dependencies.insert(
            "mathutils".to_string(),
            DependencySpec {
                path: Some("libs/mathutils".to_string()),
                version: Some("^1.0.0".to_string()),
                registry: None,
            },
        );

        assert!(write_lockfile(&tmp, &manifest).is_ok());
        fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn rejects_a_local_dependency_violating_its_version_constraint() {
        let tmp =
            std::env::temp_dir().join(format!("ailang-pm-semver-fail-{}", std::process::id()));
        let dep_dir = tmp.join("libs/mathutils");
        fs::create_dir_all(&dep_dir).unwrap();
        let mut dep_manifest = Manifest::new("mathutils");
        dep_manifest.version = "2.0.0".to_string();
        fs::write(
            dep_dir.join("ai.toml"),
            dep_manifest.to_toml_string().unwrap(),
        )
        .unwrap();

        let mut manifest = Manifest::new("demo");
        manifest.dependencies.insert(
            "mathutils".to_string(),
            DependencySpec {
                path: Some("libs/mathutils".to_string()),
                version: Some("^1.0.0".to_string()),
                registry: None,
            },
        );

        let result = write_lockfile(&tmp, &manifest);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("does not satisfy"));
        fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn fingerprint_is_stable_and_content_sensitive() {
        assert_eq!(fingerprint("a"), fingerprint("a"));
        assert_ne!(fingerprint("a"), fingerprint("b"));
    }
}

/// A deployment manifest, written as `deploy.json` inside the output
/// bundle directory by [`bundle_for_deploy`]. Records what was packaged,
/// for which target, and a content fingerprint per file — enough to
/// answer "what exactly got deployed and when," which is the real,
/// achievable slice of "Deployment System" for a language runtime shipped
/// as source (see this function's doc for the full scope boundary).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentManifest {
    pub package: String,
    pub version: String,
    pub target: String,
    pub deployed_at_unix_seconds: f64,
    pub files: Vec<DeployedFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeployedFile {
    pub relative_path: String,
    pub content_hash: String,
}

/// Packages a project directory for deployment: copies `ai.toml` and every
/// `.ail` file under `project_dir` (recursively) into
/// `project_dir/dist/<target>/`, alongside a `deploy.json` manifest.
///
/// **Scope**: this is real file bundling and a real manifest — genuinely
/// useful for "here is exactly what would be deployed." It does **not**
/// provision cloud/edge infrastructure, upload anywhere, or manage runtime
/// environments — `deploy cloud`/`deploy edge`/`deploy local` in the
/// language itself are v0.5's generic `AiPlaceholder` declarations (see
/// `ailang-ast`), and actually provisioning real infrastructure behind
/// them is out of scope for a language runtime shipped as source (it
/// would mean depending on and authenticating against specific cloud
/// providers' APIs). What ships here is the honest, achievable slice:
/// deterministic, inspectable packaging.
pub fn bundle_for_deploy(project_dir: &Path, target: &str) -> anyhow::Result<DeploymentManifest> {
    let manifest_path = project_dir.join("ai.toml");
    let manifest = Manifest::load(&manifest_path)
        .map_err(|e| anyhow::anyhow!("could not read ai.toml in {}: {e}", project_dir.display()))?;

    let dist_dir = project_dir.join("dist").join(target);
    fs::create_dir_all(&dist_dir)?;

    let mut files = Vec::new();

    // ai.toml itself
    let toml_content = fs::read_to_string(&manifest_path)?;
    fs::copy(&manifest_path, dist_dir.join("ai.toml"))?;
    files.push(DeployedFile {
        relative_path: "ai.toml".to_string(),
        content_hash: fingerprint(&toml_content),
    });

    // Every .ail file, recursively, preserving relative directory structure.
    let mut ail_files = Vec::new();
    collect_ail_files(project_dir, project_dir, &mut ail_files)?;
    ail_files.sort();
    for rel_path in ail_files {
        let src = project_dir.join(&rel_path);
        let dst = dist_dir.join(&rel_path);
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = fs::read_to_string(&src)?;
        fs::copy(&src, &dst)?;
        files.push(DeployedFile {
            relative_path: rel_path.to_string_lossy().replace('\\', "/"),
            content_hash: fingerprint(&content),
        });
    }

    let deployed_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);

    let deployment_manifest = DeploymentManifest {
        package: manifest.name,
        version: manifest.version,
        target: target.to_string(),
        deployed_at_unix_seconds: deployed_at,
        files,
    };
    fs::write(
        dist_dir.join("deploy.json"),
        serde_json::to_string_pretty(&deployment_manifest)?,
    )?;

    Ok(deployment_manifest)
}

fn collect_ail_files(
    root: &Path,
    dir: &Path,
    out: &mut Vec<std::path::PathBuf>,
) -> anyhow::Result<()> {
    // Never descend into a previous run's own dist/ output.
    if dir.file_name().map(|n| n == "dist").unwrap_or(false) {
        return Ok(());
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_ail_files(root, &path, out)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("ail") {
            out.push(path.strip_prefix(root).unwrap().to_path_buf());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundle_for_deploy_packages_ai_toml_and_ail_files_with_a_manifest() {
        let tmp =
            std::env::temp_dir().join(format!("ailang-pm-deploy-test-{}", std::process::id()));
        fs::create_dir_all(tmp.join("src")).unwrap();
        fs::write(
            tmp.join("ai.toml"),
            Manifest::new("demo").to_toml_string().unwrap(),
        )
        .unwrap();
        fs::write(tmp.join("src/main.ail"), "println(\"hello\");\n").unwrap();

        let manifest = bundle_for_deploy(&tmp, "cloud").unwrap();
        assert_eq!(manifest.package, "demo");
        assert_eq!(manifest.target, "cloud");
        assert_eq!(manifest.files.len(), 2); // ai.toml + src/main.ail

        let dist = tmp.join("dist/cloud");
        assert!(dist.join("ai.toml").exists());
        assert!(dist.join("src/main.ail").exists());
        assert!(dist.join("deploy.json").exists());

        fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn bundle_for_deploy_does_not_recurse_into_a_previous_dist_output() {
        let tmp =
            std::env::temp_dir().join(format!("ailang-pm-deploy-norecurse-{}", std::process::id()));
        fs::create_dir_all(&tmp).unwrap();
        fs::write(
            tmp.join("ai.toml"),
            Manifest::new("demo").to_toml_string().unwrap(),
        )
        .unwrap();
        fs::write(tmp.join("main.ail"), "println(1);\n").unwrap();

        bundle_for_deploy(&tmp, "cloud").unwrap();
        // Deploying again must not pick up dist/cloud/main.ail as a
        // second source file to bundle.
        let manifest = bundle_for_deploy(&tmp, "edge").unwrap();
        assert_eq!(manifest.files.len(), 2); // ai.toml + main.ail, not more

        fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn manifest_round_trips_through_toml() {
        let m = Manifest::new("demo");
        let s = m.to_toml_string().unwrap();
        let parsed: Manifest = toml::from_str(&s).unwrap();
        assert_eq!(parsed.name, "demo");
        assert_eq!(parsed.edition, "2026");
    }

    #[test]
    fn new_project_creates_expected_files() {
        let tmp = std::env::temp_dir().join(format!("ailang-pm-test-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let prev = std::env::current_dir().unwrap();
        std::env::set_current_dir(&tmp).unwrap();

        new_project("demoproj").unwrap();
        assert!(Path::new("demoproj/ai.toml").exists());
        assert!(Path::new("demoproj/src/main.ail").exists());

        std::env::set_current_dir(prev).unwrap();
        std::fs::remove_dir_all(&tmp).ok();
    }
}
