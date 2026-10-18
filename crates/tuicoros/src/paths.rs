//! Where tuicoros keeps its files. See `docs/SECURITY_AND_CONFIG.md` §4.

use std::io;
use std::path::{Path, PathBuf};

use color_eyre::Result;
use color_eyre::eyre::eyre;
use directories::ProjectDirs;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub cache_dir: PathBuf,
}

impl Paths {
    /// Platform defaults, each overridable by an environment variable
    /// (`TUICOROS_CONFIG_DIR`, `TUICOROS_DATA_DIR`, `TUICOROS_CACHE_DIR`).
    pub fn resolve(env: &impl Fn(&str) -> Option<String>) -> Result<Self> {
        let project = ProjectDirs::from("dev", "tuicoros", "tuicoros");
        let pick = |var: &str, default: Option<&Path>| -> Result<PathBuf> {
            env(var)
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
                .or_else(|| default.map(Path::to_path_buf))
                .ok_or_else(|| eyre!("cannot find a home directory; set {var}"))
        };
        Ok(Self {
            config_dir: pick(
                "TUICOROS_CONFIG_DIR",
                project.as_ref().map(|p| p.config_dir()),
            )?,
            data_dir: pick("TUICOROS_DATA_DIR", project.as_ref().map(|p| p.data_dir()))?,
            cache_dir: pick(
                "TUICOROS_CACHE_DIR",
                project.as_ref().map(|p| p.cache_dir()),
            )?,
        })
    }

    pub fn config_file(&self) -> PathBuf {
        self.config_dir.join("config.toml")
    }

    pub fn log_dir(&self) -> PathBuf {
        self.data_dir.join("logs")
    }
}

/// Create `path` (and parents) readable only by the current user on Unix.
pub fn ensure_private_dir(path: &Path) -> io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_vars_override_platform_dirs() {
        let env = |key: &str| match key {
            "TUICOROS_CONFIG_DIR" => Some("/tmp/c".to_owned()),
            "TUICOROS_DATA_DIR" => Some("/tmp/d".to_owned()),
            "TUICOROS_CACHE_DIR" => Some("/tmp/k".to_owned()),
            _ => None,
        };
        let paths = Paths::resolve(&env).unwrap();
        assert_eq!(paths.config_file(), PathBuf::from("/tmp/c/config.toml"));
        assert_eq!(paths.log_dir(), PathBuf::from("/tmp/d/logs"));
        assert_eq!(paths.cache_dir, PathBuf::from("/tmp/k"));
    }

    #[test]
    fn empty_env_var_falls_back_to_platform_dir() {
        let env = |key: &str| (key == "TUICOROS_CONFIG_DIR").then(String::new);
        let paths = Paths::resolve(&env).unwrap();
        assert!(
            paths.config_dir.ends_with("tuicoros")
                || paths.config_dir.ends_with("dev.tuicoros.tuicoros")
        );
    }

    #[cfg(unix)]
    #[test]
    fn private_dirs_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("a/b");
        ensure_private_dir(&dir).unwrap();
        let mode = std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
    }
}
