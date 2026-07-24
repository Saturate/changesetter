use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum ChangesetterError {
    #[error("no changeset files found in .changeset/")]
    NoChangesets,

    #[error("invalid changeset frontmatter in {path}: {reason}")]
    InvalidFrontmatter { path: PathBuf, reason: String },

    #[error("unknown package \"{name}\" in changeset {path}")]
    UnknownPackage { name: String, path: PathBuf },

    #[error("unknown bump level \"{level}\" in changeset {path}")]
    UnknownBumpLevel { level: String, path: PathBuf },

    #[error("working tree has uncommitted changes; commit or stash them before running release")]
    DirtyWorkingTree,

    #[error("git is not available on PATH")]
    GitNotFound,

    #[error("not a git repository")]
    NotAGitRepo,

    #[error(
        "base ref \"{base}\" is not available; ensure the ref is fetched (not a shallow clone)"
    )]
    BaseRefUnavailable { base: String },

    #[error("failed to read manifest at {path}: {reason}")]
    ManifestRead { path: PathBuf, reason: String },

    #[error("failed to write manifest at {path}: {reason}")]
    ManifestWrite { path: PathBuf, reason: String },
}
