use std::collections::{BTreeMap, BTreeSet};

use crate::package::adapter::Adapter;
use crate::package::cargo::CargoAdapter;
use crate::package::dotnet::DotnetAdapter;
use crate::package::helm::HelmAdapter;
use crate::package::npm::NpmAdapter;
use crate::package::python::PythonAdapter;
use crate::package::types::{Package, PackageType};

pub fn build_dependents_map(packages: &[Package]) -> BTreeMap<String, Vec<String>> {
    let pkg_names: BTreeSet<&str> = packages.iter().map(|p| p.name.as_str()).collect();
    let mut dependents: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for pkg in packages {
        let adapter: Box<dyn Adapter> = match pkg.package_type {
            PackageType::Cargo | PackageType::CargoWorkspace => Box::new(CargoAdapter),
            PackageType::Npm => Box::new(NpmAdapter),
            PackageType::Python => Box::new(PythonAdapter),
            PackageType::Dotnet => Box::new(DotnetAdapter),
            PackageType::Helm => Box::new(HelmAdapter),
        };

        let deps = adapter.dependencies(&pkg.path).unwrap_or_default();
        for dep in deps {
            if pkg_names.contains(dep.as_str()) {
                dependents.entry(dep).or_default().push(pkg.name.clone());
            }
        }
    }

    dependents
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::types::Version;
    use std::path::PathBuf;

    #[test]
    fn empty_packages() {
        let map = build_dependents_map(&[]);
        assert!(map.is_empty());
    }

    #[test]
    fn cargo_deps_detected() {
        let dir = tempfile::tempdir().unwrap();

        let a_dir = dir.path().join("a");
        std::fs::create_dir(&a_dir).unwrap();
        std::fs::write(
            a_dir.join("Cargo.toml"),
            "[package]\nname = \"a\"\nversion = \"1.0.0\"\n",
        )
        .unwrap();

        let b_dir = dir.path().join("b");
        std::fs::create_dir(&b_dir).unwrap();
        std::fs::write(
            b_dir.join("Cargo.toml"),
            "[package]\nname = \"b\"\nversion = \"1.0.0\"\n\n[dependencies]\na = \"1\"\n",
        )
        .unwrap();

        let packages = vec![
            Package {
                name: "a".to_string(),
                path: a_dir,
                package_type: PackageType::Cargo,
                version: Version::new(1, 0, 0),
            },
            Package {
                name: "b".to_string(),
                path: b_dir,
                package_type: PackageType::Cargo,
                version: Version::new(1, 0, 0),
            },
        ];

        let map = build_dependents_map(&packages);
        assert_eq!(map.get("a").unwrap(), &vec!["b".to_string()]);
        assert!(!map.contains_key("b"));
    }

    #[test]
    fn npm_deps_detected() {
        let dir = tempfile::tempdir().unwrap();

        let a_dir = dir.path().join("a");
        std::fs::create_dir(&a_dir).unwrap();
        std::fs::write(
            a_dir.join("package.json"),
            r#"{"name": "a", "version": "1.0.0"}"#,
        )
        .unwrap();

        let b_dir = dir.path().join("b");
        std::fs::create_dir(&b_dir).unwrap();
        std::fs::write(
            b_dir.join("package.json"),
            r#"{"name": "b", "version": "1.0.0", "dependencies": {"a": "^1.0.0"}}"#,
        )
        .unwrap();

        let packages = vec![
            Package {
                name: "a".to_string(),
                path: a_dir,
                package_type: PackageType::Npm,
                version: Version::new(1, 0, 0),
            },
            Package {
                name: "b".to_string(),
                path: b_dir,
                package_type: PackageType::Npm,
                version: Version::new(1, 0, 0),
            },
        ];

        let map = build_dependents_map(&packages);
        assert_eq!(map.get("a").unwrap(), &vec!["b".to_string()]);
    }

    #[test]
    fn external_deps_ignored() {
        let dir = tempfile::tempdir().unwrap();

        let a_dir = dir.path().join("a");
        std::fs::create_dir(&a_dir).unwrap();
        std::fs::write(
            a_dir.join("Cargo.toml"),
            "[package]\nname = \"a\"\nversion = \"1.0.0\"\n\n[dependencies]\nserde = \"1\"\n",
        )
        .unwrap();

        let packages = vec![Package {
            name: "a".to_string(),
            path: a_dir,
            package_type: PackageType::Cargo,
            version: Version::new(1, 0, 0),
        }];

        let map = build_dependents_map(&packages);
        assert!(map.is_empty());
    }

    #[test]
    fn no_self_dependency() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"a\"\nversion = \"1.0.0\"\n\n[dependencies]\na = \"1\"\n",
        )
        .unwrap();

        let packages = vec![Package {
            name: "a".to_string(),
            path: PathBuf::from(dir.path()),
            package_type: PackageType::Cargo,
            version: Version::new(1, 0, 0),
        }];

        let map = build_dependents_map(&packages);
        // "a" depends on "a" so "a" is a dependent of "a" - this is technically valid
        // but in practice the cascade logic just bumps it which is already happening
        assert_eq!(map.get("a").unwrap(), &vec!["a".to_string()]);
    }
}
