use std::collections::BTreeMap;
use std::fmt;

use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BumpLevel {
    None,
    Patch,
    Minor,
    Major,
}

impl fmt::Display for BumpLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BumpLevel::None => write!(f, "none"),
            BumpLevel::Patch => write!(f, "patch"),
            BumpLevel::Minor => write!(f, "minor"),
            BumpLevel::Major => write!(f, "major"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Changeset {
    pub packages: BTreeMap<String, BumpLevel>,
    pub body: String,
    pub filename: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bump_level_ordering() {
        assert!(BumpLevel::None < BumpLevel::Patch);
        assert!(BumpLevel::Patch < BumpLevel::Minor);
        assert!(BumpLevel::Minor < BumpLevel::Major);
    }

    #[test]
    fn bump_level_max() {
        let levels = vec![BumpLevel::Patch, BumpLevel::None, BumpLevel::Minor];
        assert_eq!(levels.into_iter().max(), Some(BumpLevel::Minor));
    }

    #[test]
    fn bump_level_display() {
        assert_eq!(BumpLevel::None.to_string(), "none");
        assert_eq!(BumpLevel::Patch.to_string(), "patch");
        assert_eq!(BumpLevel::Minor.to_string(), "minor");
        assert_eq!(BumpLevel::Major.to_string(), "major");
    }

    #[test]
    fn bump_level_deserialize() {
        let level: BumpLevel = serde_yaml::from_str("patch").unwrap();
        assert_eq!(level, BumpLevel::Patch);

        let level: BumpLevel = serde_yaml::from_str("none").unwrap();
        assert_eq!(level, BumpLevel::None);
    }

    #[test]
    fn bump_level_deserialize_unknown() {
        let result: Result<BumpLevel, _> = serde_yaml::from_str("breaking");
        assert!(result.is_err());
    }

    #[test]
    fn changeset_default_fields() {
        let cs = Changeset {
            packages: BTreeMap::from([("mylib".to_string(), BumpLevel::Patch)]),
            body: "Fixed a bug".to_string(),
            filename: Some("cool-dogs-dance".to_string()),
        };
        assert_eq!(cs.packages.len(), 1);
        assert_eq!(cs.packages["mylib"], BumpLevel::Patch);
    }
}
