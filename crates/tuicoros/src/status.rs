//! `tuicoros status`: where things live, what is configured, who is signed in.

use std::io::{self, Write};

use chrono::Utc;

use crate::config::LoadedConfig;
use crate::paths::Paths;
use crate::storage::vault::StoredSession;

pub struct SessionInfo {
    /// Where secrets are kept (keychain / file).
    pub store: String,
    pub session: Option<StoredSession>,
    /// A password hash is stored for automatic re-login.
    pub remembered: bool,
}

pub fn write_status(
    out: &mut impl Write,
    paths: &Paths,
    loaded: &LoadedConfig,
    info: &SessionInfo,
) -> io::Result<()> {
    let config = &loaded.config;
    let file_state = if loaded.file_found {
        "found"
    } else {
        "not found, using defaults"
    };
    writeln!(out, "tuicoros {}", env!("CARGO_PKG_VERSION"))?;
    writeln!(out)?;
    match &info.session {
        Some(s) => {
            let age = Utc::now().signed_duration_since(s.saved_at);
            let age = if age.num_hours() >= 1 {
                format!("{} h", age.num_hours())
            } else {
                format!("{} min", age.num_minutes().max(0))
            };
            writeln!(
                out,
                "Signed in    {} on `{}` ({})",
                s.who(),
                s.region,
                s.region.base_url()
            )?;
            writeln!(out, "Token        saved {age} ago")?;
            writeln!(
                out,
                "Re-login     {}",
                if info.remembered {
                    "automatic (password hash stored)"
                } else {
                    "manual: run `tuicoros login` when the token expires"
                }
            )?;
        }
        None => writeln!(out, "Signed in    no. Run `tuicoros login`")?,
    }
    writeln!(out, "Secrets      {}", info.store)?;
    writeln!(out)?;
    writeln!(out, "Config file  {} ({file_state})", loaded.path.display())?;
    writeln!(
        out,
        "Region       {} — {} (used for login; API calls use the signed-in region)",
        config.region,
        config.region.label()
    )?;
    writeln!(out, "Data dir     {}", paths.data_dir.display())?;
    writeln!(
        out,
        "Cache        {} ({} KB)",
        paths.cache_dir.display(),
        crate::storage::cache::size(&paths.cache_dir) / 1024
    )?;
    writeln!(out, "Logs         {}", paths.log_dir().display())?;
    writeln!(out, "Log level    {}", config.log.level)?;
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

    use coros_api::{Region, Session};
    use secrecy::SecretString;

    use super::*;
    use crate::config::Config;

    fn paths() -> Paths {
        Paths {
            config_dir: PathBuf::from("/cfg"),
            data_dir: PathBuf::from("/data"),
            cache_dir: PathBuf::from("/cache"),
        }
    }

    fn loaded(paths: &Paths) -> LoadedConfig {
        LoadedConfig {
            config: Config {
                region: Region::Asia,
                ..Config::default()
            },
            path: paths.config_file(),
            file_found: false,
            warnings: vec!["unknown key `x` (ignored)".into()],
            region_explicit: false,
        }
    }

    fn render(info: &SessionInfo) -> String {
        let paths = paths();
        let mut out = Vec::new();
        write_status(&mut out, &paths, &loaded(&paths), info).unwrap();
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn signed_out() {
        let text = render(&SessionInfo {
            store: "system keychain".into(),
            session: None,
            remembered: false,
        });
        assert!(text.contains("Signed in    no"), "{text}");
        assert!(
            text.contains("/cfg/config.toml (not found, using defaults)"),
            "{text}"
        );
        assert!(text.contains("asia — Asia / China"), "{text}");
        assert!(text.contains("/data/logs"), "{text}");
        assert!(text.contains("unknown key `x`"), "{text}");
    }

    #[test]
    fn signed_in_never_prints_the_token() {
        let api = Session {
            token: SecretString::from("tok-secret"),
            user_id: "1".into(),
            region: Region::Us,
        };
        let session = StoredSession::from_api(&api, Some("a@b.c".into()), Some("Nam".into()));
        let text = render(&SessionInfo {
            store: "system keychain".into(),
            session: Some(session),
            remembered: true,
        });
        assert!(text.contains("Signed in    Nam <a@b.c> on `us`"), "{text}");
        assert!(text.contains("automatic"), "{text}");
        assert!(!text.contains("tok-secret"), "{text}");
    }
}
