use std::fmt;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    pub pre: Option<String>,
}

impl Version {
    pub fn new(major: u64, minor: u64, patch: u64) -> Self {
        Self {
            major,
            minor,
            patch,
            pre: None,
        }
    }

    pub fn parse(s: &str) -> Result<Self, VersionParseError> {
        let (version_part, pre) = match s.split_once('-') {
            Some((v, p)) => (v, Some(p.to_string())),
            None => (s, None),
        };

        let parts: Vec<&str> = version_part.split('.').collect();
        if parts.len() != 3 {
            return Err(VersionParseError(s.to_string()));
        }

        let major = parts[0]
            .parse()
            .map_err(|_| VersionParseError(s.to_string()))?;
        let minor = parts[1]
            .parse()
            .map_err(|_| VersionParseError(s.to_string()))?;
        let patch = parts[2]
            .parse()
            .map_err(|_| VersionParseError(s.to_string()))?;

        Ok(Self {
            major,
            minor,
            patch,
            pre,
        })
    }

    pub fn bump_major(&self) -> Self {
        Self::new(self.major + 1, 0, 0)
    }

    pub fn bump_minor(&self) -> Self {
        Self::new(self.major, self.minor + 1, 0)
    }

    pub fn bump_patch(&self) -> Self {
        Self::new(self.major, self.minor, self.patch + 1)
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if let Some(pre) = &self.pre {
            write!(f, "-{pre}")?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, thiserror::Error)]
#[error("invalid version string: {0}")]
pub struct VersionParseError(String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackageType {
    Cargo,
    CargoWorkspace,
    Npm,
    Python,
    Helm,
    Dotnet,
}

impl fmt::Display for PackageType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PackageType::Cargo => write!(f, "cargo"),
            PackageType::CargoWorkspace => write!(f, "cargo-workspace"),
            PackageType::Npm => write!(f, "npm"),
            PackageType::Python => write!(f, "python"),
            PackageType::Helm => write!(f, "helm"),
            PackageType::Dotnet => write!(f, "dotnet"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    pub name: String,
    pub path: PathBuf,
    pub package_type: PackageType,
    pub version: Version,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_parse_simple() {
        let v = Version::parse("1.2.3").unwrap();
        assert_eq!(v.major, 1);
        assert_eq!(v.minor, 2);
        assert_eq!(v.patch, 3);
        assert_eq!(v.pre, None);
    }

    #[test]
    fn version_parse_with_pre() {
        let v = Version::parse("1.0.0-rc.1").unwrap();
        assert_eq!(v.major, 1);
        assert_eq!(v.minor, 0);
        assert_eq!(v.patch, 0);
        assert_eq!(v.pre.as_deref(), Some("rc.1"));
    }

    #[test]
    fn version_parse_zero() {
        let v = Version::parse("0.0.0").unwrap();
        assert_eq!(v, Version::new(0, 0, 0));
    }

    #[test]
    fn version_parse_invalid() {
        assert!(Version::parse("1.2").is_err());
        assert!(Version::parse("abc").is_err());
        assert!(Version::parse("1.2.3.4").is_err());
        assert!(Version::parse("").is_err());
    }

    #[test]
    fn version_display() {
        assert_eq!(Version::new(1, 2, 3).to_string(), "1.2.3");

        let mut v = Version::new(1, 0, 0);
        v.pre = Some("beta.2".to_string());
        assert_eq!(v.to_string(), "1.0.0-beta.2");
    }

    #[test]
    fn version_bump_patch() {
        let v = Version::new(1, 2, 3).bump_patch();
        assert_eq!(v, Version::new(1, 2, 4));
    }

    #[test]
    fn version_bump_minor() {
        let v = Version::new(1, 2, 3).bump_minor();
        assert_eq!(v, Version::new(1, 3, 0));
    }

    #[test]
    fn version_bump_major() {
        let v = Version::new(1, 2, 3).bump_major();
        assert_eq!(v, Version::new(2, 0, 0));
    }

    #[test]
    fn version_roundtrip() {
        let original = "3.14.159";
        let v = Version::parse(original).unwrap();
        assert_eq!(v.to_string(), original);
    }

    #[test]
    fn version_roundtrip_pre() {
        let original = "1.0.0-alpha.3";
        let v = Version::parse(original).unwrap();
        assert_eq!(v.to_string(), original);
    }
}
