//! `tuicoros status`: where things live and what is configured.

use std::io::{self, Write};

use crate::config::LoadedConfig;
use crate::paths::Paths;

pub fn write_status(out: &mut impl Write, paths: &Paths, loaded: &LoadedConfig) -> io::Result<()> {
    let config = &loaded.config;
    let file_state = if loaded.file_found {
        "found"
    } else {
        "not found, using defaults"
    };
    writeln!(out, "tuicoros {}", env!("CARGO_PKG_VERSION"))?;
    writeln!(out)?;
    writeln!(out, "Config file  {} ({file_state})", loaded.path.display())?;
    writeln!(
        out,
        "Region       {} — {} ({})",
        config.region,
        config.region.label(),
        config.region.base_url()
    )?;
    writeln!(out, "Data dir     {}", paths.data_dir.display())?;
    writeln!(out, "Cache dir    {}", paths.cache_dir.display())?;
    writeln!(out, "Logs         {}", paths.log_dir().display())?;
    writeln!(out, "Log level    {}", config.log.level)?;
    writeln!(out, "Session      not signed in (login arrives in Phase 1)")?;
    if !loaded.warnings.is_empty() {
        writeln!(out)?;
        writeln!(out, "Warnings:")?;
        for warning in &loaded.warnings {
            writeln!(out, "  - {warning}")?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use coros_api::Region;

    use super::*;
    use crate::config::Config;

    #[test]
    fn prints_paths_region_and_warnings() {
        let paths = Paths {
            config_dir: PathBuf::from("/cfg"),
            data_dir: PathBuf::from("/data"),
            cache_dir: PathBuf::from("/cache"),
        };
        let loaded = LoadedConfig {
            config: Config {
                region: Region::Asia,
                ..Config::default()
            },
            path: paths.config_file(),
            file_found: false,
            warnings: vec!["unknown key `x` (ignored)".into()],
        };
        let mut out = Vec::new();
        write_status(&mut out, &paths, &loaded).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(
            text.contains("/cfg/config.toml (not found, using defaults)"),
            "{text}"
        );
        assert!(
            text.contains("asia — Asia / China (https://teamcnapi.coros.com)"),
            "{text}"
        );
        assert!(text.contains("/data/logs"), "{text}");
        assert!(text.contains("unknown key `x`"), "{text}");
    }
}
