use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::Config;
use crate::package::adapter::Adapter;
use crate::package::cargo::CargoAdapter;
use crate::package::npm::NpmAdapter;
use crate::package::types::Package;

const EXCLUDED_DIRS: &[&str] = &["node_modules", "target", ".git", "vendor", "dist", "build"];

pub fn detect_packages(repo_root: &Path, config: &Config) -> anyhow::Result<Vec<Package>> {
    let manifest_dirs = find_manifest_dirs(repo_root)?;

    let adapters: Vec<Box<dyn Adapter>> = vec![Box::new(CargoAdapter), Box::new(NpmAdapter)];

    let mut packages: BTreeMap<String, Package> = BTreeMap::new();

    for dir in manifest_dirs {
        for adapter in &adapters {
            if let Some(pkg) = adapter.detect(&dir)? {
                packages.entry(pkg.name.clone()).or_insert(pkg);
            }
        }
    }

    for pkg_config in &config.packages {
        let pkg_path = repo_root.join(&pkg_config.path);
        for adapter in &adapters {
            if let Some(pkg) = adapter.detect(&pkg_path)? {
                packages.insert(pkg.name.clone(), pkg);
                break;
            }
        }
    }

    let ignored: std::collections::HashSet<&str> =
        config.ignore.iter().map(|s| s.as_str()).collect();
    packages.retain(|name, _| !ignored.contains(name.as_str()));

    Ok(packages.into_values().collect())
}

fn find_manifest_dirs(repo_root: &Path) -> anyhow::Result<Vec<PathBuf>> {
    if let Ok(dirs) = find_via_git(repo_root) {
        return Ok(dirs);
    }
    Ok(find_via_walk(repo_root))
}

fn find_via_git(repo_root: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let output = Command::new("git")
        .args(["ls-files", "--full-name"])
        .current_dir(repo_root)
        .output()?;

    if !output.status.success() {
        anyhow::bail!("git ls-files failed");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut dirs: Vec<PathBuf> = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for line in stdout.lines() {
        let filename = Path::new(line)
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or_default();

        if is_manifest(filename) {
            let dir = Path::new(line).parent().unwrap_or(Path::new(""));
            let full = repo_root.join(dir);
            if seen.insert(full.clone()) {
                dirs.push(full);
            }
        }
    }

    Ok(dirs)
}

fn find_via_walk(repo_root: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    walk_dir(repo_root, &mut dirs);
    dirs
}

fn walk_dir(current: &Path, dirs: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(current) else {
        return;
    };

    let mut has_manifest = false;
    let mut subdirs = Vec::new();

    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        if path.is_file() && is_manifest(&name_str) {
            has_manifest = true;
        } else if path.is_dir() && !EXCLUDED_DIRS.contains(&name_str.as_ref()) {
            subdirs.push(path);
        }
    }

    if has_manifest {
        dirs.push(current.to_path_buf());
    }

    for subdir in subdirs {
        walk_dir(&subdir, dirs);
    }
}

fn is_manifest(filename: &str) -> bool {
    matches!(filename, "Cargo.toml" | "package.json")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::types::PackageType;

    fn init_git(dir: &Path) {
        Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .current_dir(dir)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(dir)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(dir)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "commit.gpgsign", "false"])
            .current_dir(dir)
            .output()
            .unwrap();
    }

    fn git_add_all(dir: &Path) {
        Command::new("git")
            .args(["add", "."])
            .current_dir(dir)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-q", "-m", "init", "--allow-empty"])
            .current_dir(dir)
            .output()
            .unwrap();
    }

    #[test]
    fn detect_single_cargo_package() {
        let dir = tempfile::tempdir().unwrap();
        init_git(dir.path());
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"mylib\"\nversion = \"1.0.0\"\n",
        )
        .unwrap();
        git_add_all(dir.path());

        let config = Config::default();
        let pkgs = detect_packages(dir.path(), &config).unwrap();
        assert_eq!(pkgs.len(), 1);
        assert_eq!(pkgs[0].name, "mylib");
        assert_eq!(pkgs[0].package_type, PackageType::Cargo);
    }

    #[test]
    fn detect_npm_package() {
        let dir = tempfile::tempdir().unwrap();
        init_git(dir.path());
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"name": "my-app", "version": "2.0.0"}"#,
        )
        .unwrap();
        git_add_all(dir.path());

        let config = Config::default();
        let pkgs = detect_packages(dir.path(), &config).unwrap();
        assert_eq!(pkgs.len(), 1);
        assert_eq!(pkgs[0].name, "my-app");
        assert_eq!(pkgs[0].package_type, PackageType::Npm);
    }

    #[test]
    fn detect_polyglot() {
        let dir = tempfile::tempdir().unwrap();
        init_git(dir.path());
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"backend\"\nversion = \"1.0.0\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join("web")).unwrap();
        std::fs::write(
            dir.path().join("web/package.json"),
            r#"{"name": "frontend", "version": "1.0.0"}"#,
        )
        .unwrap();
        git_add_all(dir.path());

        let config = Config::default();
        let pkgs = detect_packages(dir.path(), &config).unwrap();
        assert_eq!(pkgs.len(), 2);
        let names: Vec<&str> = pkgs.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"backend"));
        assert!(names.contains(&"frontend"));
    }

    #[test]
    fn ignore_filters_packages() {
        let dir = tempfile::tempdir().unwrap();
        init_git(dir.path());
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"mylib\"\nversion = \"1.0.0\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join("examples")).unwrap();
        std::fs::write(
            dir.path().join("examples/Cargo.toml"),
            "[package]\nname = \"example-app\"\nversion = \"0.0.0\"\n",
        )
        .unwrap();
        git_add_all(dir.path());

        let config = Config {
            ignore: vec!["example-app".to_string()],
            ..Default::default()
        };

        let pkgs = detect_packages(dir.path(), &config).unwrap();
        assert_eq!(pkgs.len(), 1);
        assert_eq!(pkgs[0].name, "mylib");
    }

    #[test]
    fn fallback_walk_skips_node_modules() {
        let dir = tempfile::tempdir().unwrap();
        // No git init, so walk fallback is used
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"mylib\"\nversion = \"1.0.0\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join("node_modules/foo")).unwrap();
        std::fs::write(
            dir.path().join("node_modules/foo/package.json"),
            r#"{"name": "foo", "version": "1.0.0"}"#,
        )
        .unwrap();

        let config = Config::default();
        let pkgs = detect_packages(dir.path(), &config).unwrap();
        assert_eq!(pkgs.len(), 1);
        assert_eq!(pkgs[0].name, "mylib");
    }
}
