//! `tuicoros login`: ask for credentials, find the account's region, store the session.

use std::io::{BufRead, IsTerminal, Write};

use color_eyre::Result;
use color_eyre::eyre::{WrapErr, bail};
use coros_api::{Account, CorosClient, CorosError, Credentials, Region, Session};
use secrecy::{ExposeSecret, SecretString};

use super::Ctx;
use crate::cli::LoginArgs;
use crate::storage::vault::StoredSession;

pub async fn run(ctx: &Ctx<'_>, args: &LoginArgs) -> Result<()> {
    let email = match args.email.clone().or_else(|| env_nonempty("COROS_EMAIL")) {
        Some(email) => email,
        None => prompt("COROS email: ")?,
    };
    let email = email.trim().to_owned();
    if email.is_empty() {
        bail!("an email is required");
    }

    // Held as a SecretString so it is wiped from memory when dropped.
    let password = match env_nonempty("COROS_PASSWORD") {
        Some(p) => SecretString::from(p),
        None => SecretString::from(
            rpassword::prompt_password("COROS password: ").wrap_err("cannot read password")?,
        ),
    };
    if password.expose_secret().is_empty() {
        bail!("an empty password can't be right");
    }
    let credentials = Credentials::from_password(&email, password.expose_secret());
    drop(password);

    let vault = ctx.vault();
    let remember = if args.remember {
        true
    } else if args.no_remember {
        false
    } else {
        confirm(&format!(
            "Remember the password hash in the {} so tuicoros can sign in again by itself? [y/N] ",
            vault.describe()
        ))?
    };

    let explicit = ctx.loaded.region_explicit || ctx.cli_region.is_some();
    let regions = regions_to_try(ctx.loaded.config.region, explicit);
    eprintln!("Signing in…");
    let signed_in = sign_in(
        &credentials,
        &regions,
        |r| CorosClient::new(ctx.client_config(r)),
        |m| eprintln!("{m}"),
    )
    .await?;

    let region = signed_in.session.region;
    let stored = StoredSession::from_api(
        &signed_in.session,
        signed_in.account.email.clone().or(Some(email)),
        signed_in.account.nickname.clone(),
    );
    vault.save_session(&stored)?;
    if remember {
        vault.save_credentials(&credentials)?;
    } else {
        vault.forget_credentials()?;
    }

    println!(
        "Signed in as {} on the `{region}` server ({}).",
        stored.who(),
        region.label()
    );
    if remember {
        println!(
            "Password hash saved: tuicoros will sign in again by itself when the token expires."
        );
    } else {
        println!(
            "Password not saved: run `tuicoros login` again when the token expires \
             (signing in on t.coros.com or the COROS app can expire it)."
        );
    }
    if !explicit && region != ctx.loaded.config.region {
        println!(
            "Tip: add `region = \"{region}\"` to {} to skip region detection next time.",
            ctx.loaded.path.display()
        );
    }
    Ok(())
}

/// The preferred region first; if it wasn't set explicitly, then the others.
pub fn regions_to_try(preferred: Region, explicit: bool) -> Vec<Region> {
    let mut regions = vec![preferred];
    if !explicit {
        for region in [Region::Asia, Region::Us, Region::Eu] {
            if !regions.contains(&region) {
                regions.push(region);
            }
        }
    }
    regions
}

pub struct SignedIn {
    pub session: Session,
    pub account: Account,
}

/// Try each region in turn. A region counts only if the token it hands out
/// also works there: COROS can accept a login on the wrong region and then
/// reject every call with `1019`.
pub async fn sign_in(
    credentials: &Credentials,
    regions: &[Region],
    make_client: impl Fn(Region) -> coros_api::Result<CorosClient>,
    mut progress: impl FnMut(String),
) -> Result<SignedIn> {
    let mut failures = Vec::new();
    for &region in regions {
        let client = make_client(region)?;
        match client.login(credentials).await {
            Ok(session) => match client.account().await {
                Ok(account) => return Ok(SignedIn { session, account }),
                Err(err) if err.is_token_invalid() => {
                    progress(format!(
                        "  {region}: password accepted but the token doesn't work there"
                    ));
                    failures.push(format!("{region}: token rejected right after login"));
                }
                Err(err) => {
                    return Err(err).wrap_err(format!(
                        "signed in on `{region}`, but reading the profile failed"
                    ));
                }
            },
            // Don't hammer COROS (or a dead network) with more attempts.
            Err(err @ (CorosError::RateLimited | CorosError::Network { .. })) => {
                return Err(err).wrap_err(format!("sign-in on `{region}` failed"));
            }
            Err(err) => {
                if regions.len() > 1 {
                    progress(format!("  {region}: {err}"));
                }
                failures.push(format!("{region}: {err}"));
            }
        }
    }
    bail!(
        "could not sign in.\n  {}\nCheck your email and password. If they are right, your account may be in a region not tried here.",
        failures.join("\n  ")
    )
}

fn env_nonempty(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.trim().is_empty())
}

fn prompt(question: &str) -> Result<String> {
    if !std::io::stdin().is_terminal() {
        bail!("not running in a terminal: pass --email or set COROS_EMAIL");
    }
    eprint!("{question}");
    std::io::stderr().flush()?;
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line)?;
    Ok(line.trim().to_owned())
}

/// Yes/no, default no. Non-interactive → no.
fn confirm(question: &str) -> Result<bool> {
    if !std::io::stdin().is_terminal() {
        return Ok(false);
    }
    let answer = prompt(question)?;
    Ok(matches!(answer.to_ascii_lowercase().as_str(), "y" | "yes"))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use coros_api::ClientConfig;
    use wiremock::matchers::path;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    #[test]
    fn region_order() {
        assert_eq!(
            regions_to_try(Region::Us, false),
            vec![Region::Us, Region::Asia, Region::Eu]
        );
        assert_eq!(
            regions_to_try(Region::Asia, false),
            vec![Region::Asia, Region::Us, Region::Eu]
        );
        assert_eq!(regions_to_try(Region::Eu, true), vec![Region::Eu]);
    }

    fn ok(data: serde_json::Value) -> ResponseTemplate {
        ResponseTemplate::new(200)
            .set_body_json(serde_json::json!({"result": "0000", "message": "OK", "data": data}))
    }

    fn err(code: &str, message: &str) -> ResponseTemplate {
        ResponseTemplate::new(200)
            .set_body_json(serde_json::json!({"result": code, "message": message}))
    }

    /// One mock server per region.
    async fn servers() -> HashMap<Region, MockServer> {
        let mut map = HashMap::new();
        for region in Region::ALL {
            map.insert(region, MockServer::start().await);
        }
        map
    }

    fn factory(
        servers: &HashMap<Region, MockServer>,
    ) -> impl Fn(Region) -> coros_api::Result<CorosClient> + '_ {
        move |region| {
            CorosClient::new(ClientConfig {
                base_url: Some(servers[&region].uri()),
                retry_base_delay: std::time::Duration::ZERO,
                ..ClientConfig::new(region)
            })
        }
    }

    fn creds() -> Credentials {
        Credentials::from_password("runner@example.invalid", "pw")
    }

    #[tokio::test]
    async fn finds_the_region_where_the_token_works() {
        let s = servers().await;
        // Asia: login works but the token is useless there.
        Mock::given(path("/account/login"))
            .respond_with(ok(serde_json::json!({"accessToken": "a", "userId": 1})))
            .mount(&s[&Region::Asia])
            .await;
        Mock::given(path("/account/query"))
            .respond_with(err("1019", "Access token is invalid"))
            .mount(&s[&Region::Asia])
            .await;
        // US: the real home of the account.
        Mock::given(path("/account/login"))
            .respond_with(ok(serde_json::json!({"accessToken": "u", "userId": 1})))
            .mount(&s[&Region::Us])
            .await;
        Mock::given(path("/account/query"))
            .respond_with(ok(serde_json::json!({"userId": 1, "nickname": "Runner"})))
            .mount(&s[&Region::Us])
            .await;

        let mut messages = Vec::new();
        let signed_in = sign_in(
            &creds(),
            &regions_to_try(Region::Asia, false),
            factory(&s),
            |m| messages.push(m),
        )
        .await
        .unwrap();
        assert_eq!(signed_in.session.region, Region::Us);
        assert_eq!(signed_in.account.nickname.as_deref(), Some("Runner"));
        assert_eq!(messages.len(), 1, "{messages:?}");
        assert!(
            s[&Region::Eu].received_requests().await.unwrap().is_empty(),
            "stopped after success"
        );
    }

    #[tokio::test]
    async fn reports_every_region_when_all_fail() {
        let s = servers().await;
        for server in s.values() {
            Mock::given(path("/account/login"))
                .respond_with(err("1030", "Incorrect password"))
                .mount(server)
                .await;
        }
        let e = sign_in(
            &creds(),
            &regions_to_try(Region::Us, false),
            factory(&s),
            |_| {},
        )
        .await
        .err()
        .unwrap();
        let text = format!("{e:#}");
        for region in ["us:", "asia:", "eu:"] {
            assert!(text.contains(region), "{text}");
        }
    }

    #[tokio::test]
    async fn explicit_region_tries_only_that_one() {
        let s = servers().await;
        Mock::given(path("/account/login"))
            .respond_with(err("1030", "Incorrect password"))
            .mount(&s[&Region::Eu])
            .await;
        assert!(
            sign_in(
                &creds(),
                &regions_to_try(Region::Eu, true),
                factory(&s),
                |_| {}
            )
            .await
            .is_err()
        );
        assert!(s[&Region::Us].received_requests().await.unwrap().is_empty());
        assert!(
            s[&Region::Asia]
                .received_requests()
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn rate_limit_stops_the_search() {
        let s = servers().await;
        Mock::given(path("/account/login"))
            .respond_with(ResponseTemplate::new(429))
            .mount(&s[&Region::Us])
            .await;
        let e = sign_in(
            &creds(),
            &regions_to_try(Region::Us, false),
            factory(&s),
            |_| {},
        )
        .await
        .err()
        .unwrap();
        assert!(format!("{e:#}").contains("rate limiting"), "{e:#}");
        assert!(
            s[&Region::Asia]
                .received_requests()
                .await
                .unwrap()
                .is_empty()
        );
    }
}
