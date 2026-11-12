use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::Path;

use clap::Args;

use crate::changeset::types::BumpLevel;
use crate::changeset::writer;
use crate::config::Config;
use crate::package::detector;

#[derive(Args)]
pub struct AddArgs {
    /// Package(s) affected by this change
    #[arg(long)]
    pub package: Vec<String>,

    /// Bump level (patch, minor, major)
    #[arg(long)]
    pub bump: Option<String>,

    /// Create a none-bump changeset (no version change)
    #[arg(long)]
    pub no_bump: bool,

    /// Change description (non-interactive mode)
    #[arg(long, short)]
    pub message: Option<String>,

    /// Conventional type (feat, fix, perf, docs, ...) for grouped changelogs
    #[arg(long = "type")]
    pub kind: Option<String>,

    /// Scope shown in front of the entry in grouped changelogs
    #[arg(long)]
    pub scope: Option<String>,
}

/// Checks a `--type` or `--scope` value: it goes into the front matter
/// unquoted, so it's kept to characters YAML reads as a plain string, and a
/// bump level would be read as a package named "type" or "scope".
fn validate_meta(flag: &str, value: &str, extra: &[char]) -> anyhow::Result<()> {
    let ok = value
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphanumeric())
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || extra.contains(&c));
    if !ok {
        anyhow::bail!(
            "--{flag} {value:?}: use letters, digits, '-' or '_'{}",
            if extra.is_empty() { "" } else { ", '.' or '/'" }
        );
    }
    if ["major", "minor", "patch", "none"].contains(&value) {
        anyhow::bail!("--{flag} {value:?}: a bump level can't be a {flag}");
    }
    Ok(())
}

pub fn run(args: &AddArgs) -> anyhow::Result<()> {
    let repo_root = crate::git::find_repo_root()?;
    run_in(&repo_root, args)
}

pub fn run_in(repo_root: &Path, args: &AddArgs) -> anyhow::Result<()> {
    let config = Config::load(repo_root)?;
    let changeset_dir = config.changeset_dir(repo_root);

    if !changeset_dir.exists() {
        std::fs::create_dir_all(&changeset_dir)?;
    }

    if let Some(kind) = &args.kind {
        validate_meta("type", kind, &[])?;
    }
    if let Some(scope) = &args.scope {
        validate_meta("scope", scope, &['.', '/'])?;
    }

    let is_tty = atty_check();

    if !is_tty && args.package.is_empty() && args.message.is_none() && !args.no_bump {
        anyhow::bail!(
            "Non-interactive mode requires --package, --bump, and --message flags.\n\
             Run interactively in a terminal, or use:\n  \
             changesetter add --package <name> --bump <level> --message \"description\""
        );
    }

    let packages = if !args.package.is_empty() {
        resolve_packages_noninteractive(repo_root, &config, args)?
    } else if is_tty {
        resolve_packages_interactive(repo_root, &config, args)?
    } else {
        anyhow::bail!("--package is required in non-interactive mode");
    };

    for key in ["type", "scope"] {
        let given = if key == "type" {
            &args.kind
        } else {
            &args.scope
        };
        if given.is_some() && packages.contains_key(key) {
            anyhow::bail!("--{key} can't be used with a package named \"{key}\"");
        }
    }

    let body = if let Some(msg) = &args.message {
        msg.trim().to_string()
    } else if is_tty {
        prompt_body()?
    } else {
        String::new()
    };

    let name = writer::write_changeset_typed(
        &changeset_dir,
        &packages,
        args.kind.as_deref(),
        args.scope.as_deref(),
        &body,
    )?;
    eprintln!("Created {}/{name}.md", config.changeset_dir_relative());
    crate::changelog::generator::warn_fenced_boundaries(&format!("{name}.md"), &body);
    // The same warning `status` and `release` give, for a `breaking` type
    // from --type or from a "feat!:" prefix on the message.
    for (pkg, bump) in &packages {
        let kind = crate::changelog::groups::classify(
            &body,
            args.kind.as_deref(),
            args.scope.as_deref(),
            *bump,
            &config.changelog,
        )
        .kind;
        if let Some(w) =
            crate::changelog::groups::breaking_warning(&format!("{name}.md"), &kind, pkg, *bump)
        {
            eprintln!("{w}");
        }
    }

    Ok(())
}

fn resolve_packages_noninteractive(
    repo_root: &Path,
    config: &Config,
    args: &AddArgs,
) -> anyhow::Result<BTreeMap<String, BumpLevel>> {
    let bump = if args.no_bump {
        BumpLevel::None
    } else {
        match args.bump.as_deref() {
            Some("patch") => BumpLevel::Patch,
            Some("minor") => BumpLevel::Minor,
            Some("major") => BumpLevel::Major,
            Some("none") => BumpLevel::None,
            Some(other) => anyhow::bail!("unknown bump level: {other}"),
            None => anyhow::bail!("--bump is required (or use --no-bump)"),
        }
    };

    let detected = detector::detect_packages(repo_root, config)?;
    let mut packages = BTreeMap::new();

    for pkg_name in &args.package {
        if pkg_name == "default" || detected.iter().any(|p| &p.name == pkg_name) {
            packages.insert(pkg_name.clone(), bump);
        } else {
            anyhow::bail!("unknown package: {pkg_name}");
        }
    }

    Ok(packages)
}

fn resolve_packages_interactive(
    repo_root: &Path,
    config: &Config,
    args: &AddArgs,
) -> anyhow::Result<BTreeMap<String, BumpLevel>> {
    let detected = detector::detect_packages(repo_root, config)?;

    if detected.is_empty() {
        eprintln!("No packages detected. Using 'default'.");
        let bump = prompt_bump(args)?;
        return Ok(BTreeMap::from([("default".to_string(), bump)]));
    }

    eprintln!("Detected packages:");
    for (i, pkg) in detected.iter().enumerate() {
        eprintln!("  {}: {} ({})", i + 1, pkg.name, pkg.version);
    }

    eprint!("Select packages (comma-separated numbers, or 'all'): ");
    io::stderr().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let input = input.trim();

    let selected: Vec<&str> = if input == "all" {
        detected.iter().map(|p| p.name.as_str()).collect()
    } else {
        input
            .split(',')
            .filter_map(|s| {
                let idx: usize = s.trim().parse().ok()?;
                detected.get(idx - 1).map(|p| p.name.as_str())
            })
            .collect()
    };

    if selected.is_empty() {
        anyhow::bail!("no packages selected");
    }

    let bump = prompt_bump(args)?;

    Ok(selected
        .into_iter()
        .map(|name| (name.to_string(), bump))
        .collect())
}

fn prompt_bump(args: &AddArgs) -> anyhow::Result<BumpLevel> {
    if args.no_bump {
        return Ok(BumpLevel::None);
    }

    eprintln!("Bump level:");
    eprintln!("  1: patch");
    eprintln!("  2: minor");
    eprintln!("  3: major");
    eprintln!("  4: none");
    eprint!("Select [1-4]: ");
    io::stderr().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;

    match input.trim() {
        "1" | "patch" => Ok(BumpLevel::Patch),
        "2" | "minor" => Ok(BumpLevel::Minor),
        "3" | "major" => Ok(BumpLevel::Major),
        "4" | "none" => Ok(BumpLevel::None),
        other => anyhow::bail!("invalid bump level: {other}"),
    }
}

fn prompt_body() -> anyhow::Result<String> {
    eprint!("Description (one line, or empty to open $EDITOR): ");
    io::stderr().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let input = input.trim();

    if input.is_empty() {
        open_editor()
    } else {
        Ok(input.to_string())
    }
}

fn open_editor() -> anyhow::Result<String> {
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
    let path = std::env::temp_dir().join(format!("changesetter-{}.md", std::process::id()));

    std::fs::write(&path, "")?;

    let status = std::process::Command::new(&editor).arg(&path).status()?;

    if !status.success() {
        let _ = std::fs::remove_file(&path);
        anyhow::bail!("editor exited with non-zero status");
    }

    let content = std::fs::read_to_string(&path)?;
    let _ = std::fs::remove_file(&path);
    Ok(content.trim().to_string())
}

fn atty_check() -> bool {
    std::io::IsTerminal::is_terminal(&std::io::stdin())
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
            "[package]\nname = \"testpkg\"\nversion = \"1.0.0\"\n",
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

    #[test]
    fn add_noninteractive() {
        let dir = setup_repo();
        let args = AddArgs {
            package: vec!["testpkg".to_string()],
            bump: Some("patch".to_string()),
            no_bump: false,
            message: Some("Fixed a bug".to_string()),
            kind: None,
            scope: None,
        };
        run_in(dir.path(), &args).unwrap();

        let changeset_dir = dir.path().join(".changeset");
        let files: Vec<_> = std::fs::read_dir(&changeset_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "md"))
            .collect();
        assert_eq!(files.len(), 1);

        let content = std::fs::read_to_string(files[0].path()).unwrap();
        assert!(content.contains("testpkg: patch"));
        assert!(content.contains("Fixed a bug"));
    }

    #[test]
    fn add_no_bump() {
        let dir = setup_repo();
        let args = AddArgs {
            package: vec!["testpkg".to_string()],
            bump: None,
            no_bump: true,
            message: Some("Docs update".to_string()),
            kind: None,
            scope: None,
        };
        run_in(dir.path(), &args).unwrap();

        let changeset_dir = dir.path().join(".changeset");
        let files: Vec<_> = std::fs::read_dir(&changeset_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "md"))
            .collect();
        assert_eq!(files.len(), 1);

        let content = std::fs::read_to_string(files[0].path()).unwrap();
        assert!(content.contains("testpkg: none"));
    }

    #[test]
    fn add_unknown_package_errors() {
        let dir = setup_repo();
        let args = AddArgs {
            package: vec!["nonexistent".to_string()],
            bump: Some("patch".to_string()),
            no_bump: false,
            message: Some("Fix".to_string()),
            kind: None,
            scope: None,
        };
        let result = run_in(dir.path(), &args);
        assert!(result.is_err());
    }

    fn only_changeset(dir: &Path) -> String {
        let changeset_dir = dir.join(".changeset");
        let files: Vec<_> = std::fs::read_dir(&changeset_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "md"))
            .collect();
        assert_eq!(files.len(), 1);
        std::fs::read_to_string(files[0].path()).unwrap()
    }

    #[test]
    fn add_message_is_plain_text() {
        let dir = setup_repo();
        let args = AddArgs {
            package: vec!["testpkg".to_string()],
            bump: Some("patch".to_string()),
            no_bump: false,
            message: Some("Fixed a bug".to_string()),
            kind: None,
            scope: None,
        };
        run_in(dir.path(), &args).unwrap();
        assert_eq!(
            only_changeset(dir.path()),
            "---\ntestpkg: patch\n---\n\nFixed a bug\n"
        );
    }

    #[test]
    fn add_with_type_and_scope() {
        let dir = setup_repo();
        let args = AddArgs {
            package: vec!["testpkg".to_string()],
            bump: Some("minor".to_string()),
            no_bump: false,
            message: Some("New biome".to_string()),
            kind: Some("feat".to_string()),
            scope: Some("world/gen".to_string()),
        };
        run_in(dir.path(), &args).unwrap();
        assert_eq!(
            only_changeset(dir.path()),
            "---\ntestpkg: minor\ntype: feat\nscope: world/gen\n---\n\nNew biome\n"
        );
    }

    #[test]
    fn add_rejects_bad_type_and_scope() {
        for (kind, scope, expected) in [
            (Some("fe at"), None, "--type \"fe at\""),
            (Some("patch"), None, "a bump level can't be a type"),
            (Some("x: y"), None, "--type"),
            (None, Some("-ui"), "--scope \"-ui\""),
        ] {
            let dir = setup_repo();
            let args = AddArgs {
                package: vec!["testpkg".to_string()],
                bump: Some("patch".to_string()),
                no_bump: false,
                message: Some("Fix".to_string()),
                kind: kind.map(str::to_string),
                scope: scope.map(str::to_string),
            };
            let err = run_in(dir.path(), &args).unwrap_err().to_string();
            assert!(err.contains(expected), "{expected}: got {err}");
        }
    }
}
