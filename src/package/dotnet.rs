use std::path::Path;

use crate::errors::ChangesetterError;
use crate::package::adapter::Adapter;
use crate::package::types::{Package, PackageType, Version};

pub struct DotnetAdapter;

impl DotnetAdapter {
    fn find_csproj(dir: &Path) -> Option<std::path::PathBuf> {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return None;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                if let Some(ext) = path.extension() {
                    if ext == "csproj" {
                        return Some(path);
                    }
                }
            }
        }
        None
    }

    fn extract_xml_element(content: &str, tag: &str) -> Option<String> {
        let open = format!("<{tag}>");
        let close = format!("</{tag}>");
        let start = content.find(&open)? + open.len();
        let end = content[start..].find(&close)? + start;
        Some(content[start..end].trim().to_string())
    }

    fn package_name_from_csproj(content: &str, csproj_path: &Path) -> String {
        if let Some(name) = Self::extract_xml_element(content, "PackageName") {
            if !name.is_empty() {
                return name;
            }
        }
        if let Some(name) = Self::extract_xml_element(content, "AssemblyName") {
            if !name.is_empty() {
                return name;
            }
        }
        csproj_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string()
    }
}

impl Adapter for DotnetAdapter {
    fn detect(&self, path: &Path) -> anyhow::Result<Option<Package>> {
        let csproj_path =
            if path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("csproj") {
                path.to_path_buf()
            } else {
                match Self::find_csproj(path) {
                    Some(p) => p,
                    None => return Ok(None),
                }
            };

        let content =
            std::fs::read_to_string(&csproj_path).map_err(|e| ChangesetterError::ManifestRead {
                path: csproj_path.clone(),
                reason: e.to_string(),
            })?;

        let version_str = match Self::extract_xml_element(&content, "Version") {
            Some(v) if !v.is_empty() => v,
            _ => return Ok(None),
        };

        let version = Version::parse(&version_str)?;
        let name = Self::package_name_from_csproj(&content, &csproj_path);
        let dir = csproj_path.parent().unwrap_or(Path::new("."));

        Ok(Some(Package {
            name,
            path: dir.to_path_buf(),
            package_type: PackageType::Dotnet,
            version,
        }))
    }

    fn read_version(&self, path: &Path) -> anyhow::Result<Version> {
        let csproj_path = if path.is_file() {
            path.to_path_buf()
        } else {
            Self::find_csproj(path)
                .ok_or_else(|| anyhow::anyhow!("no .csproj file found in {}", path.display()))?
        };

        let content =
            std::fs::read_to_string(&csproj_path).map_err(|e| ChangesetterError::ManifestRead {
                path: csproj_path.clone(),
                reason: e.to_string(),
            })?;

        let version_str = Self::extract_xml_element(&content, "Version")
            .ok_or_else(|| anyhow::anyhow!("no <Version> found in {}", csproj_path.display()))?;

        Ok(Version::parse(&version_str)?)
    }

    fn write_version(&self, path: &Path, version: &Version) -> anyhow::Result<()> {
        let csproj_path = if path.is_file() {
            path.to_path_buf()
        } else {
            Self::find_csproj(path)
                .ok_or_else(|| anyhow::anyhow!("no .csproj file found in {}", path.display()))?
        };

        let content =
            std::fs::read_to_string(&csproj_path).map_err(|e| ChangesetterError::ManifestRead {
                path: csproj_path.clone(),
                reason: e.to_string(),
            })?;

        let open_tag = "<Version>";
        let close_tag = "</Version>";

        let Some(start) = content.find(open_tag) else {
            anyhow::bail!("no <Version> element found in {}", csproj_path.display());
        };

        let after_open = start + open_tag.len();
        let Some(end_offset) = content[after_open..].find(close_tag) else {
            anyhow::bail!("malformed <Version> element in {}", csproj_path.display());
        };

        let mut output = String::with_capacity(content.len());
        output.push_str(&content[..after_open]);
        output.push_str(&version.to_string());
        output.push_str(&content[after_open + end_offset..]);

        std::fs::write(&csproj_path, &output).map_err(|e| ChangesetterError::ManifestWrite {
            path: csproj_path,
            reason: e.to_string(),
        })?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIMPLE_CSPROJ: &str = r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <TargetFramework>net8.0</TargetFramework>
    <Version>1.2.3</Version>
    <AssemblyName>MyLib</AssemblyName>
  </PropertyGroup>
</Project>
"#;

    const WITH_COMMENTS_CSPROJ: &str = r#"<Project Sdk="Microsoft.NET.Sdk">
  <!-- Build configuration -->
  <PropertyGroup Condition="'$(Configuration)' == 'Debug'">
    <OutputPath>bin\Debug</OutputPath>
  </PropertyGroup>

  <PropertyGroup>
    <TargetFramework>net8.0</TargetFramework>
    <!-- Package metadata -->
    <Version>2.0.0</Version>
    <PackageName>My.Cool.Package</PackageName>
    <Authors>Test Author</Authors>
  </PropertyGroup>

  <ItemGroup>
    <PackageReference Include="Newtonsoft.Json" Version="13.0.1" />
  </ItemGroup>
</Project>
"#;

    const NO_VERSION_CSPROJ: &str = r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <TargetFramework>net8.0</TargetFramework>
  </PropertyGroup>
</Project>
"#;

    #[test]
    fn detect_simple_csproj() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("MyLib.csproj"), SIMPLE_CSPROJ).unwrap();

        let adapter = DotnetAdapter;
        let pkg = adapter.detect(dir.path()).unwrap().unwrap();
        assert_eq!(pkg.name, "MyLib");
        assert_eq!(pkg.version, Version::new(1, 2, 3));
        assert_eq!(pkg.package_type, PackageType::Dotnet);
    }

    #[test]
    fn detect_with_package_name() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("MyLib.csproj"), WITH_COMMENTS_CSPROJ).unwrap();

        let adapter = DotnetAdapter;
        let pkg = adapter.detect(dir.path()).unwrap().unwrap();
        assert_eq!(pkg.name, "My.Cool.Package");
        assert_eq!(pkg.version, Version::new(2, 0, 0));
    }

    #[test]
    fn detect_no_version_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("MyLib.csproj"), NO_VERSION_CSPROJ).unwrap();

        let adapter = DotnetAdapter;
        assert!(adapter.detect(dir.path()).unwrap().is_none());
    }

    #[test]
    fn detect_no_csproj_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let adapter = DotnetAdapter;
        assert!(adapter.detect(dir.path()).unwrap().is_none());
    }

    #[test]
    fn detect_falls_back_to_filename_stem() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("FallbackName.csproj"),
            r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <Version>0.1.0</Version>
  </PropertyGroup>
</Project>"#,
        )
        .unwrap();

        let adapter = DotnetAdapter;
        let pkg = adapter.detect(dir.path()).unwrap().unwrap();
        assert_eq!(pkg.name, "FallbackName");
    }

    #[test]
    fn read_write_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let csproj = dir.path().join("MyLib.csproj");
        std::fs::write(&csproj, SIMPLE_CSPROJ).unwrap();

        let adapter = DotnetAdapter;
        let v = adapter.read_version(dir.path()).unwrap();
        assert_eq!(v, Version::new(1, 2, 3));

        adapter
            .write_version(dir.path(), &Version::new(2, 0, 0))
            .unwrap();

        let v2 = adapter.read_version(dir.path()).unwrap();
        assert_eq!(v2, Version::new(2, 0, 0));
    }

    #[test]
    fn write_preserves_comments_and_structure() {
        let dir = tempfile::tempdir().unwrap();
        let csproj = dir.path().join("MyLib.csproj");
        std::fs::write(&csproj, WITH_COMMENTS_CSPROJ).unwrap();

        let adapter = DotnetAdapter;
        adapter
            .write_version(dir.path(), &Version::new(3, 0, 0))
            .unwrap();

        let content = std::fs::read_to_string(&csproj).unwrap();
        assert!(content.contains("<Version>3.0.0</Version>"));
        assert!(content.contains("<!-- Build configuration -->"));
        assert!(content.contains("<!-- Package metadata -->"));
        assert!(content.contains("<PackageName>My.Cool.Package</PackageName>"));
        assert!(content.contains("Condition=\"'$(Configuration)' == 'Debug'\""));
        assert!(content.contains("<PackageReference Include=\"Newtonsoft.Json\""));
    }

    #[test]
    fn read_version_from_file_path() {
        let dir = tempfile::tempdir().unwrap();
        let csproj = dir.path().join("MyLib.csproj");
        std::fs::write(&csproj, SIMPLE_CSPROJ).unwrap();

        let adapter = DotnetAdapter;
        let v = adapter.read_version(&csproj).unwrap();
        assert_eq!(v, Version::new(1, 2, 3));
    }
}
