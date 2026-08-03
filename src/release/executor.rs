use std::path::Path;

use crate::changelog::generator;
use crate::changeset::reader;
use crate::config::Config;
use crate::errors::ChangesetterError;
use crate::package::adapter::Adapter;
use crate::package::cargo::CargoAdapter;
use crate::package::dotnet::DotnetAdapter;
use crate::package::helm::HelmAdapter;
use crate::package::npm::NpmAdapter;
use crate::package::python::PythonAdapter;
use crate::package::types::Version;
use crate::release::plan::{self, ReleasePlan};

pub struct ExecuteOptions {
    pub dry_run: bool,
    pub no_commit: bool,
    pub snapshot: Option<String>,
}

pub struct ExecuteResult {
    pub plan: ReleasePlan,
    pub date: String,
    pub config: Config,
    pub is_monorepo: bool,
}

pub fn execute_version(repo_root: &Path, opts: &ExecuteOptions) -> anyhow::Result<ExecuteResult> {
    let config = Config::load(repo_root)?;
    let changeset_dir = repo_root.join(".changeset");
    let changesets = reader::read_changesets(&changeset_dir)?;

    if changesets.is_empty() {
        eprintln!("No pending changesets, nothing to release.");
        return Ok(ExecuteResult {
            plan: ReleasePlan {
                releases: vec![],
                none_entries: vec![],
            },
            date: String::new(),
            is_monorepo: false,
            config,
        });
    }

    let packages = crate::package::detector::detect_packages(repo_root, &config)?;
    let is_monorepo = packages.len() > 1;
    let changeset_dir_for_pre = changeset_dir.clone();
    let pre_state = crate::release::pre::read_pre_state(&changeset_dir_for_pre);
    let pre_ref = if opts.snapshot.is_some() {
        None
    } else {
        pre_state.as_ref()
    };
    let release_plan = plan::assemble(&changesets, &packages, &config, pre_ref);

    if release_plan.releases.is_empty() && release_plan.none_entries.is_empty() {
        eprintln!("No pending changesets, nothing to release.");
        return Ok(ExecuteResult {
            plan: release_plan,
            date: String::new(),
            is_monorepo,
            config,
        });
    }

    let date = today();

    if opts.dry_run {
        print_dry_run(&release_plan, &date);
        return Ok(ExecuteResult {
            plan: release_plan,
            date,
            is_monorepo,
            config,
        });
    }

    if !opts.no_commit && !crate::git::is_working_tree_clean(repo_root)? {
        anyhow::bail!(ChangesetterError::DirtyWorkingTree);
    }

    if let Some(tag) = &opts.snapshot {
        apply_snapshot(repo_root, &release_plan, tag, &packages)?;
        return Ok(ExecuteResult {
            plan: release_plan,
            date,
            is_monorepo,
            config,
        });
    }

    for release in &release_plan.releases {
        apply_version_bump(repo_root, &release.name, &release.version, &packages)?;
    }

    if !config.hooks.post_bump.is_empty() {
        for hook in &config.hooks.post_bump {
            eprintln!("Running post-bump hook: {hook}");
            let status = std::process::Command::new("sh")
                .args(["-c", hook])
                .current_dir(repo_root)
                .status()?;
            if !status.success() {
                anyhow::bail!("post-bump hook failed: {hook}");
            }
        }
    }

    if config.changelog.per_package && is_monorepo {
        generator::write_per_package_changelogs(&release_plan, &config.changelog, &date)?;
    } else {
        let entry = generator::generate_changelog_entry(
            &release_plan,
            &config.changelog,
            &date,
            is_monorepo,
        );
        let changelog_path = repo_root.join(&config.changelog.file);
        generator::update_changelog_file(&changelog_path, &entry)?;
    }

    for cs in &changesets {
        if let Some(name) = &cs.filename {
            let path = changeset_dir.join(format!("{name}.md"));
            if path.exists() {
                std::fs::remove_file(&path)?;
            }
        }
    }

    if let Some(state) = &pre_state {
        if state.mode == "pre" {
            let mut updated = state.clone();
            for release in &release_plan.releases {
                let counter = updated
                    .packages_released
                    .entry(release.name.clone())
                    .or_insert(0);
                *counter += 1;
            }
            crate::release::pre::write_pre_state(&changeset_dir, &updated)?;
        } else if state.mode == "exit" {
            crate::release::pre::remove_pre_state(&changeset_dir)?;
        }
    }

    if !opts.no_commit {
        commit_version_changes(repo_root, &release_plan, &config, is_monorepo)?;
    }

    for release in &release_plan.releases {
        eprintln!(
            "{} {} -> {}",
            release.name, release.previous_version, release.version
        );
    }

    Ok(ExecuteResult {
        plan: release_plan,
        date,
        is_monorepo,
        config,
    })
}

fn apply_version_bump(
    _repo_root: &Path,
    pkg_name: &str,
    new_version: &Version,
    packages: &[crate::package::types::Package],
) -> anyhow::Result<()> {
    let pkg = packages
        .iter()
        .find(|p| p.name == pkg_name)
        .ok_or_else(|| anyhow::anyhow!("package not found: {pkg_name}"))?;

    let adapter: Box<dyn Adapter> = match pkg.package_type {
        crate::package::types::PackageType::Cargo
        | crate::package::types::PackageType::CargoWorkspace => Box::new(CargoAdapter),
        crate::package::types::PackageType::Npm => Box::new(NpmAdapter),
        crate::package::types::PackageType::Python => Box::new(PythonAdapter),
        crate::package::types::PackageType::Helm => Box::new(HelmAdapter),
        crate::package::types::PackageType::Dotnet => Box::new(DotnetAdapter),
    };

    adapter.write_version(&pkg.path, new_version)?;
    Ok(())
}

fn apply_snapshot(
    repo_root: &Path,
    plan: &ReleasePlan,
    tag: &str,
    packages: &[crate::package::types::Package],
) -> anyhow::Result<()> {
    let timestamp = chrono_lite_timestamp();

    for release in &plan.releases {
        let snapshot_version = Version {
            major: 0,
            minor: 0,
            patch: 0,
            pre: Some(format!("{tag}-{timestamp}")),
        };

        apply_version_bump(repo_root, &release.name, &snapshot_version, packages)?;
        eprintln!("{} -> {snapshot_version}", release.name);
    }

    Ok(())
}

fn commit_version_changes(
    repo_root: &Path,
    plan: &ReleasePlan,
    config: &Config,
    is_monorepo: bool,
) -> anyhow::Result<()> {
    let mut paths_to_stage: Vec<String> = Vec::new();

    paths_to_stage.push(".changeset/".to_string());

    let manifest_names = ["Cargo.toml", "package.json"];
    for release in &plan.releases {
        let rel_path = release
            .path
            .strip_prefix(repo_root)
            .unwrap_or(&release.path);
        for name in &manifest_names {
            let full = repo_root.join(rel_path).join(name);
            if full.exists() {
                paths_to_stage.push(rel_path.join(name).to_string_lossy().to_string());
            }
        }

        if is_monorepo && config.changelog.per_package {
            paths_to_stage.push(
                rel_path
                    .join(&config.changelog.file)
                    .to_string_lossy()
                    .to_string(),
            );
        }
    }

    if !is_monorepo || !config.changelog.per_package {
        paths_to_stage.push(config.changelog.file.clone());
    }

    let path_refs: Vec<&str> = paths_to_stage.iter().map(|s| s.as_str()).collect();
    crate::git::git_add(repo_root, &path_refs)?;

    let versions: Vec<String> = plan
        .releases
        .iter()
        .map(|r| format!("{}@{}", r.name, r.version))
        .collect();
    let versions_str = versions.join(", ");

    let message = config
        .release
        .commit_message
        .replace("{versions}", &versions_str);

    crate::git::git_commit(repo_root, &message)?;
    Ok(())
}

fn print_dry_run(plan: &ReleasePlan, date: &str) {
    println!("Dry run - the following changes would be made:\n");

    for release in &plan.releases {
        println!(
            "  {} {} -> {} ({})",
            release.name, release.previous_version, release.version, release.bump
        );
    }

    for entry in &plan.none_entries {
        println!("  {} (no version change)", entry.title);
    }

    println!("\nChangelog date: {date}");
}

fn today() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let days = now / 86400;
    let mut y = 1970i64;
    let mut remaining = days as i64;

    loop {
        let days_in_year = if is_leap(y) { 366 } else { 365 };
        if remaining < days_in_year {
            break;
        }
        remaining -= days_in_year;
        y += 1;
    }

    let months = if is_leap(y) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };

    let mut m = 0;
    for days_in_month in months {
        if remaining < days_in_month {
            break;
        }
        remaining -= days_in_month;
        m += 1;
    }

    format!("{y}-{:02}-{:02}", m + 1, remaining + 1)
}

fn chrono_lite_timestamp() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let date = today();
    let time_secs = now % 86400;
    let h = time_secs / 3600;
    let m = (time_secs % 3600) / 60;
    let s = time_secs % 60;
    format!("{}T{:02}{:02}{:02}", date.replace('-', ""), h, m, s)
}

fn is_leap(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn setup_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "commit.gpgsign", "false"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"testpkg\"\nversion = \"1.0.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-q", "-m", "init"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        dir
    }

    fn add_changeset(dir: &Path, name: &str, content: &str) {
        let changeset_dir = dir.join(".changeset");
        std::fs::create_dir_all(&changeset_dir).unwrap();
        std::fs::write(changeset_dir.join(format!("{name}.md")), content).unwrap();
    }

    #[test]
    fn version_no_changesets() {
        let dir = setup_repo();
        let opts = ExecuteOptions {
            dry_run: false,
            no_commit: true,
            snapshot: None,
        };
        let result = execute_version(dir.path(), &opts).unwrap();
        assert!(result.plan.releases.is_empty());
    }

    #[test]
    fn version_dry_run() {
        let dir = setup_repo();
        add_changeset(
            dir.path(),
            "test",
            "---\ntestpkg: minor\n---\n\n#### Feature\n",
        );

        let opts = ExecuteOptions {
            dry_run: true,
            no_commit: false,
            snapshot: None,
        };
        let result = execute_version(dir.path(), &opts).unwrap();
        assert_eq!(result.plan.releases.len(), 1);

        // Verify no files changed
        let v = CargoAdapter.read_version(dir.path()).unwrap();
        assert_eq!(v, Version::new(1, 0, 0));
    }

    #[test]
    fn version_bumps_and_writes_changelog() {
        let dir = setup_repo();
        add_changeset(
            dir.path(),
            "test",
            "---\ntestpkg: patch\n---\n\n#### Bug fix\n",
        );

        let opts = ExecuteOptions {
            dry_run: false,
            no_commit: true,
            snapshot: None,
        };
        let result = execute_version(dir.path(), &opts).unwrap();
        assert_eq!(result.plan.releases.len(), 1);
        assert_eq!(result.plan.releases[0].version, Version::new(1, 0, 1));

        // Verify version bumped in manifest
        let v = CargoAdapter.read_version(dir.path()).unwrap();
        assert_eq!(v, Version::new(1, 0, 1));

        // Verify changelog created
        let changelog = std::fs::read_to_string(dir.path().join("CHANGELOG.md")).unwrap();
        assert!(changelog.contains("## 1.0.1"));
        assert!(changelog.contains("Bug fix"));

        // Verify changeset removed
        assert!(!dir.path().join(".changeset/test.md").exists());
    }

    #[test]
    fn version_commits_changes() {
        let dir = setup_repo();
        add_changeset(
            dir.path(),
            "test",
            "---\ntestpkg: minor\n---\n\n#### Feature\n",
        );

        // Stage the changeset first so working tree is clean for the version command
        Command::new("git")
            .args(["add", "."])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-q", "-m", "add changeset"])
            .current_dir(dir.path())
            .output()
            .unwrap();

        let opts = ExecuteOptions {
            dry_run: false,
            no_commit: false,
            snapshot: None,
        };
        execute_version(dir.path(), &opts).unwrap();

        // Verify commit was made
        let output = Command::new("git")
            .args(["log", "--oneline", "-1"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        let msg = String::from_utf8_lossy(&output.stdout);
        assert!(msg.contains("release"));
    }

    #[test]
    fn version_snapshot() {
        let dir = setup_repo();
        add_changeset(
            dir.path(),
            "test",
            "---\ntestpkg: minor\n---\n\n#### Feature\n",
        );

        let opts = ExecuteOptions {
            dry_run: false,
            no_commit: true,
            snapshot: Some("canary".to_string()),
        };
        execute_version(dir.path(), &opts).unwrap();

        let v = CargoAdapter.read_version(dir.path()).unwrap();
        assert_eq!(v.major, 0);
        assert_eq!(v.minor, 0);
        assert_eq!(v.patch, 0);
        assert!(v.pre.as_ref().unwrap().starts_with("canary-"));

        // Changeset should NOT be consumed
        assert!(dir.path().join(".changeset/test.md").exists());
    }

    #[test]
    fn today_returns_valid_date() {
        let d = today();
        assert!(d.len() == 10);
        assert!(d.starts_with("20"));
    }
}
