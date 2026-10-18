//! Non-TUI subcommands: `login`, `logout`, `api …`.

pub mod api;
pub mod login;

use color_eyre::Result;
use color_eyre::eyre::eyre;
use coros_api::{ClientConfig, CorosClient, Region};
use std::time::Duration;

use crate::config::{LoadedConfig, NetworkConfig};
use crate::paths::Paths;
use crate::storage::secrets;
use crate::storage::vault::{StoredSession, Vault};

/// What every command needs.
pub struct Ctx<'a> {
    pub paths: &'a Paths,
    pub loaded: &'a LoadedConfig,
    /// `--region` from the command line, if given.
    pub cli_region: Option<Region>,
}

impl Ctx<'_> {
    pub fn vault(&self) -> Vault {
        Vault::new(secrets::open(
            self.loaded.config.secrets.backend,
            &self.paths.data_dir,
        ))
    }

    pub fn client_config(&self, region: Region) -> ClientConfig {
        client_config(&self.loaded.config.network, region)
    }

    /// A client for the stored session, with auto re-login if a password hash
    /// is stored. New tokens are saved back to the vault.
    ///
    /// `COROS_ACCESS_TOKEN` overrides the stored token (scripting/CI).
    pub fn signed_in_client(&self) -> Result<(CorosClient, StoredSession)> {
        let vault = self.vault();
        let stored = match std::env::var("COROS_ACCESS_TOKEN")
            .ok()
            .filter(|t| !t.is_empty())
        {
            Some(token) => {
                let base = vault.session()?;
                StoredSession {
                    token,
                    user_id: std::env::var("COROS_USER_ID")
                        .ok()
                        .or_else(|| base.as_ref().map(|b| b.user_id.clone()))
                        .unwrap_or_default(),
                    region: base
                        .as_ref()
                        .map_or(self.loaded.config.region, |b| b.region),
                    email: base.as_ref().and_then(|b| b.email.clone()),
                    nickname: base.as_ref().and_then(|b| b.nickname.clone()),
                    saved_at: chrono::Utc::now(),
                }
            }
            None => vault
                .session()?
                .ok_or_else(|| eyre!("not signed in. Run `tuicoros login` first"))?,
        };
        if let Some(region) = self.cli_region
            && region != stored.region
        {
            eprintln!(
                "note: you are signed in on `{}`; ignoring --region {region}. Run `tuicoros login --region {region}` to switch.",
                stored.region
            );
        }

        let client = build_client(vault, &stored, self.client_config(stored.region))?;
        Ok((client, stored))
    }
}

/// HTTP settings from config. `TUICOROS_API_BASE_URL` (development only)
/// points every region at a mock server.
pub fn client_config(net: &NetworkConfig, region: Region) -> ClientConfig {
    ClientConfig {
        base_url: std::env::var("TUICOROS_API_BASE_URL")
            .ok()
            .filter(|u| !u.is_empty()),
        user_agent: net.user_agent.clone(),
        timeout: Duration::from_secs(net.timeout_secs),
        max_concurrent: usize::from(net.max_concurrent_requests),
        ..ClientConfig::new(region)
    }
}

/// A client for a stored session: auto re-login when a password hash is
/// stored, and refreshed tokens saved back to the vault. Shared by the CLI and the TUI.
pub fn build_client(
    vault: Vault,
    stored: &StoredSession,
    config: ClientConfig,
) -> Result<CorosClient> {
    let mut client = CorosClient::new(config)?.with_session(stored.to_api());
    if let Some(credentials) = vault.credentials()? {
        client = client.with_credentials(credentials);
    }
    let (email, nickname) = (stored.email.clone(), stored.nickname.clone());
    Ok(client.on_session_change(move |session| {
        let updated = StoredSession::from_api(session, email.clone(), nickname.clone());
        if let Err(err) = vault.save_session(&updated) {
            tracing::warn!(error = %format!("{err:#}"), "could not save the refreshed token");
        }
    }))
}

/// `tuicoros logout [--purge]`.
pub fn logout(ctx: &Ctx, purge: bool) -> Result<()> {
    if purge {
        let freed = crate::storage::cache::clear(&ctx.paths.cache_dir)?;
        println!("Deleted the cache ({} KB).", freed / 1024);
    }
    let vault = ctx.vault();
    match vault.clear()? {
        (false, false) => println!("You were not signed in."),
        (_, true) => println!(
            "Signed out. Removed the token and the stored password hash ({}).",
            vault.describe()
        ),
        (true, false) => println!("Signed out. Removed the token ({}).", vault.describe()),
    }
    Ok(())
}
