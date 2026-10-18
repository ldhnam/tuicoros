//! Runs `Command`s on background tasks and reports back as `Action`s.
//! Nothing here touches UI state (docs/ARCHITECTURE.md §3).

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use chrono::{DateTime, Local, NaiveDate, Utc};
use coros_api::{
    ActivityDetail, ActivityQuery, ActivitySummary, ClientConfig, CorosClient, Credentials,
    DateRange, LabelId, Page, Region, SportType, yyyymmdd,
};
use tokio::sync::mpsc::UnboundedSender;

use crate::action::{Action, Fetched, RequestId, UiError, UiErrorKind};
use crate::commands::{build_client, login::sign_in};
use crate::storage::cache::Cache;
use crate::storage::vault::{StoredSession, Vault};

/// Activity list pages younger than this are not re-fetched.
const LIST_FRESH_FOR: chrono::Duration = chrono::Duration::minutes(10);
/// Samples kept per activity (enough for any chart; keeps the cache small).
const MAX_SAMPLES: usize = 1000;

/// Work for the service. Built by `App::update`.
#[derive(Debug, Clone)]
pub enum Command {
    Login {
        credentials: Credentials,
        regions: Vec<Region>,
        remember: bool,
    },
    LoadActivities {
        req: RequestId,
        query: ActivityQuery,
        force: bool,
    },
    LoadDetail {
        req: RequestId,
        id: LabelId,
        sport: SportType,
        force: bool,
    },
    Fetch {
        req: RequestId,
        what: FetchKind,
        force: bool,
    },
}

/// Data the insight screens need. Each kind has its own cache key and freshness.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FetchKind {
    Dashboard,
    /// Daily EvoLab metrics and weekly loads.
    Daily(DateRange),
    /// Training calendar.
    Schedule(DateRange),
    /// Every activity in the range (weekly distance).
    Activities(DateRange),
}

impl FetchKind {
    fn key(&self) -> String {
        let r = |r: &DateRange| format!("{}-{}", yyyymmdd(r.start), yyyymmdd(r.end));
        match self {
            Self::Dashboard => "dashboard".to_owned(),
            Self::Daily(range) => format!("daily-{}", r(range)),
            Self::Schedule(range) => format!("schedule-{}", r(range)),
            Self::Activities(range) => format!("range-{}", r(range)),
        }
    }

    /// How long a cached copy is good without refetching.
    fn fresh_for(&self, today: NaiveDate) -> chrono::Duration {
        match self {
            Self::Dashboard | Self::Schedule(_) => chrono::Duration::minutes(10),
            // Ranges that end before today don't change any more.
            Self::Daily(r) | Self::Activities(r) if r.end < today => chrono::Duration::days(1),
            Self::Daily(_) | Self::Activities(_) => chrono::Duration::minutes(30),
        }
    }
}

type ConfigFn = dyn Fn(Region) -> ClientConfig + Send + Sync;

#[derive(Clone)]
pub struct DataService {
    inner: Arc<Inner>,
}

struct Inner {
    tx: UnboundedSender<Action>,
    vault: Vault,
    cache_root: PathBuf,
    client_config: Box<ConfigFn>,
    signed_in: RwLock<Option<SignedIn>>,
}

#[derive(Clone)]
struct SignedIn {
    client: Arc<CorosClient>,
    cache: Cache,
}

impl DataService {
    pub fn new(
        tx: UnboundedSender<Action>,
        vault: Vault,
        cache_root: PathBuf,
        client_config: impl Fn(Region) -> ClientConfig + Send + Sync + 'static,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                tx,
                vault,
                cache_root,
                client_config: Box::new(client_config),
                signed_in: RwLock::new(None),
            }),
        }
    }

    /// Use a stored session (no network).
    pub fn resume(&self, stored: &StoredSession) -> color_eyre::Result<()> {
        let config = (self.inner.client_config)(stored.region);
        let client = build_client(self.inner.vault.clone(), stored, config)?;
        let cache = Cache::for_user(&self.inner.cache_root, &stored.user_id);
        if let Ok(mut guard) = self.inner.signed_in.write() {
            *guard = Some(SignedIn {
                client: Arc::new(client),
                cache,
            });
        }
        Ok(())
    }

    pub fn dispatch(&self, command: Command) {
        let this = self.clone();
        tokio::spawn(async move { this.run(command).await });
    }

    /// Run a command to completion (tests call this directly).
    pub async fn run(&self, command: Command) {
        match command {
            Command::Login {
                credentials,
                regions,
                remember,
            } => self.login(credentials, regions, remember).await,
            Command::LoadActivities { req, query, force } => {
                self.load_activities(req, query, force).await
            }
            Command::LoadDetail {
                req,
                id,
                sport,
                force,
            } => self.load_detail(req, id, sport, force).await,
            Command::Fetch { req, what, force } => self.fetch(req, what, force).await,
        }
    }

    /// Cache-first fetch: send a cached copy at once, then refresh if it's stale.
    async fn fetch(&self, req: RequestId, what: FetchKind, force: bool) {
        let Some(s) = self.signed_in() else {
            return self.send(not_signed_in(req));
        };
        let key = what.key();
        if !force && let Some((data, fetched_at)) = s.cache.get::<Fetched>(&key) {
            let fresh = Utc::now() - fetched_at < what.fresh_for(Local::now().date_naive());
            self.send(Action::Fetched {
                req,
                data: Box::new(data),
                fetched_at,
                from_cache: true,
                refreshing: !fresh,
            });
            if fresh {
                return;
            }
        }
        let result = match what {
            FetchKind::Dashboard => s.client.dashboard().await.map(Fetched::Dashboard),
            FetchKind::Daily(range) => s.client.day_detail(range).await.map(Fetched::Daily),
            FetchKind::Schedule(range) => s.client.schedule(range).await.map(Fetched::Schedule),
            FetchKind::Activities(range) => {
                let query = ActivityQuery {
                    page: 1,
                    size: 200,
                    from: Some(range.start),
                    to: Some(range.end),
                    sports: Vec::new(),
                };
                s.client
                    .all_activities(&query, Some(5000))
                    .await
                    .map(Fetched::Activities)
            }
        };
        match result {
            Ok(data) => {
                let now = Utc::now();
                if let Err(err) = s.cache.put(&key, &data, now) {
                    tracing::warn!(error = %format!("{err:#}"), key, "could not cache");
                }
                self.send(Action::Fetched {
                    req,
                    data: Box::new(data),
                    fetched_at: now,
                    from_cache: false,
                    refreshing: false,
                });
            }
            Err(err) => self.fail(req, "loading data", &err),
        }
    }

    fn send(&self, action: Action) {
        // The receiver only disappears when the app is shutting down.
        let _ = self.inner.tx.send(action);
    }

    fn signed_in(&self) -> Option<SignedIn> {
        self.inner.signed_in.read().ok().and_then(|g| g.clone())
    }

    fn fail(&self, req: RequestId, what: &str, err: &coros_api::CorosError) {
        tracing::warn!(error = %err, "{what} failed");
        self.send(Action::RequestFailed {
            req,
            error: UiError::from(err),
        });
    }

    async fn login(&self, credentials: Credentials, regions: Vec<Region>, remember: bool) {
        let make = |region| CorosClient::new((self.inner.client_config)(region));
        let result = sign_in(&credentials, &regions, make, |m| tracing::info!("{m}")).await;
        let signed_in = match result {
            Ok(s) => s,
            Err(err) => {
                tracing::warn!(error = %format!("{err:#}"), "sign-in failed");
                return self.send(Action::LoginFailed {
                    message: format!("{err:#}"),
                });
            }
        };
        let stored = StoredSession::from_api(
            &signed_in.session,
            signed_in
                .account
                .email
                .clone()
                .or_else(|| Some(credentials.account.clone())),
            signed_in.account.nickname.clone(),
        );
        let vault = &self.inner.vault;
        let saved = vault
            .save_session(&stored)
            .and_then(|()| {
                if remember {
                    vault.save_credentials(&credentials)
                } else {
                    vault.forget_credentials()
                }
            })
            .and_then(|()| self.resume(&stored));
        match saved {
            Ok(()) => self.send(Action::LoginSucceeded {
                who: stored.short_name(),
                region: stored.region,
            }),
            Err(err) => self.send(Action::LoginFailed {
                message: format!("signed in, but saving the session failed: {err:#}"),
            }),
        }
    }

    async fn load_activities(&self, req: RequestId, query: ActivityQuery, force: bool) {
        let Some(s) = self.signed_in() else {
            return self.send(not_signed_in(req));
        };
        let key = list_key(&query);
        if !force && let Some((page, fetched_at)) = s.cache.get::<Page<ActivitySummary>>(&key) {
            let fresh = Utc::now() - fetched_at < LIST_FRESH_FOR;
            self.send(Action::ActivitiesLoaded {
                req,
                page,
                fetched_at,
                from_cache: true,
                refreshing: !fresh,
            });
            if fresh {
                return;
            }
        }
        match s.client.activities(&query).await {
            Ok(page) => {
                let now = Utc::now();
                if let Err(err) = s.cache.put(&key, &page, now) {
                    tracing::warn!(error = %format!("{err:#}"), "could not cache activity list");
                }
                self.send(Action::ActivitiesLoaded {
                    req,
                    page,
                    fetched_at: now,
                    from_cache: false,
                    refreshing: false,
                });
            }
            Err(err) => self.fail(req, "loading activities", &err),
        }
    }

    async fn load_detail(&self, req: RequestId, id: LabelId, sport: SportType, force: bool) {
        let Some(s) = self.signed_in() else {
            return self.send(not_signed_in(req));
        };
        let key = format!("detail-{id}");
        // Finished activities don't change: a cached detail is good forever.
        if !force && let Some((detail, fetched_at)) = s.cache.get::<ActivityDetail>(&key) {
            return self.send(Action::DetailLoaded {
                req,
                detail: Box::new(detail),
                fetched_at,
                from_cache: true,
            });
        }
        match s.client.activity_detail(&id, sport).await {
            Ok(mut detail) => {
                thin_samples(&mut detail, MAX_SAMPLES);
                let now = Utc::now();
                if let Err(err) = s.cache.put(&key, &detail, now) {
                    tracing::warn!(error = %format!("{err:#}"), "could not cache activity detail");
                }
                self.send(Action::DetailLoaded {
                    req,
                    detail: Box::new(detail),
                    fetched_at: now,
                    from_cache: false,
                });
            }
            Err(err) => self.fail(req, "loading activity", &err),
        }
    }
}

fn not_signed_in(req: RequestId) -> Action {
    Action::RequestFailed {
        req,
        error: UiError {
            kind: UiErrorKind::SessionExpired,
            message: "Not signed in.".to_owned(),
        },
    }
}

/// Cache key for one page of a query.
fn list_key(q: &ActivityQuery) -> String {
    let day = |d: Option<chrono::NaiveDate>| d.map_or_else(|| "any".to_owned(), yyyymmdd);
    let sports: Vec<String> = q.sports.iter().map(|s| s.code().to_string()).collect();
    let sports = if sports.is_empty() {
        "all".to_owned()
    } else {
        sports.join(".")
    };
    format!(
        "activities-{}-{}-{sports}-s{}-p{}",
        day(q.from),
        day(q.to),
        q.size,
        q.page
    )
}

/// Keep at most `max` samples, evenly spaced, always keeping the last one.
pub fn thin_samples(detail: &mut ActivityDetail, max: usize) {
    let n = detail.samples.len();
    if n <= max || max < 2 {
        return;
    }
    let step = n.div_ceil(max - 1);
    let last = detail.samples[n - 1].clone();
    let mut i = 0;
    detail.samples.retain(|_| {
        let keep = i % step == 0;
        i += 1;
        keep
    });
    if detail.samples.last() != Some(&last) {
        detail.samples.push(last);
    }
}

/// When the data shown was fetched, for "cached 5 min ago".
pub fn age_label(fetched_at: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let mins = (now - fetched_at).num_minutes().max(0);
    match mins {
        0 => "just now".to_owned(),
        1..=59 => format!("{mins} min ago"),
        60..=1439 => format!("{} h ago", mins / 60),
        _ => format!("{} d ago", mins / 1440),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::sync::mpsc;
    use wiremock::matchers::path;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;
    use crate::storage::secrets::FileStore;

    struct Setup {
        _tmp: tempfile::TempDir,
        server: MockServer,
        service: DataService,
        rx: mpsc::UnboundedReceiver<Action>,
    }

    async fn setup(signed_in: bool) -> Setup {
        let tmp = tempfile::tempdir().unwrap();
        let server = MockServer::start().await;
        let (tx, rx) = mpsc::unbounded_channel();
        let vault = Vault::new(Box::new(FileStore::new(tmp.path().join("secrets.json"))));
        let uri = server.uri();
        let service = DataService::new(tx, vault, tmp.path().join("cache"), move |region| {
            ClientConfig {
                base_url: Some(uri.clone()),
                retry_base_delay: Duration::ZERO,
                max_retries: 0,
                ..ClientConfig::new(region)
            }
        });
        if signed_in {
            let stored = StoredSession {
                token: "tok".into(),
                user_id: "42".into(),
                region: Region::Us,
                email: Some("a@b.c".into()),
                nickname: Some("Runner".into()),
                saved_at: Utc::now(),
            };
            service.resume(&stored).unwrap();
        }
        Setup {
            _tmp: tmp,
            server,
            service,
            rx,
        }
    }

    fn page_body(n: usize) -> serde_json::Value {
        let items: Vec<_> = (0..n)
            .map(|i| serde_json::json!({"labelId": format!("id{i}"), "sportType": 100, "distance": 5000, "totalTime": 1500}))
            .collect();
        serde_json::json!({"result": "0000", "data": {"count": n, "totalPage": 1, "pageNumber": 1, "dataList": items}})
    }

    fn drain(rx: &mut mpsc::UnboundedReceiver<Action>) -> Vec<Action> {
        let mut out = Vec::new();
        while let Ok(a) = rx.try_recv() {
            out.push(a);
        }
        out
    }

    fn load(req: u64, force: bool) -> Command {
        Command::LoadActivities {
            req: RequestId(req),
            query: ActivityQuery::default(),
            force,
        }
    }

    #[tokio::test]
    async fn list_is_fetched_cached_then_served_from_cache() {
        let mut s = setup(true).await;
        Mock::given(path("/activity/query"))
            .respond_with(ResponseTemplate::new(200).set_body_json(page_body(3)))
            .expect(2) // first load + forced refresh; the second plain load is served by the cache
            .mount(&s.server)
            .await;

        s.service.run(load(1, false)).await;
        let first = drain(&mut s.rx);
        assert!(
            matches!(&first[..], [Action::ActivitiesLoaded { from_cache: false, page, .. }] if page.items.len() == 3)
        );

        s.service.run(load(2, false)).await;
        let second = drain(&mut s.rx);
        assert!(
            matches!(
                &second[..],
                [Action::ActivitiesLoaded {
                    req: RequestId(2),
                    from_cache: true,
                    refreshing: false,
                    ..
                }]
            ),
            "fresh cache, no refetch"
        );

        s.service.run(load(3, true)).await;
        let forced = drain(&mut s.rx);
        assert!(
            matches!(
                &forced[..],
                [Action::ActivitiesLoaded {
                    from_cache: false,
                    ..
                }]
            ),
            "force skips the cache"
        );
    }

    #[tokio::test]
    async fn detail_is_cached_forever_and_thinned() {
        let mut s = setup(true).await;
        let samples: Vec<_> = (0..5000)
            .map(|i| serde_json::json!({"timestamp": 179_000_000_000_i64 + i * 100, "heart": 120}))
            .collect();
        let body = serde_json::json!({"result": "0000", "data": {"summary": {"sportType": 100, "distance": 500000}, "frequencyList": samples}});
        Mock::given(path("/activity/detail/query"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .expect(1)
            .mount(&s.server)
            .await;
        let cmd = |req| Command::LoadDetail {
            req: RequestId(req),
            id: LabelId("7".into()),
            sport: SportType::Run,
            force: false,
        };

        s.service.run(cmd(1)).await;
        let Some(Action::DetailLoaded {
            detail,
            from_cache: false,
            ..
        }) = drain(&mut s.rx).pop()
        else {
            panic!("no detail")
        };
        assert!(detail.samples.len() <= MAX_SAMPLES);

        s.service.run(cmd(2)).await;
        let Some(Action::DetailLoaded {
            detail: cached,
            from_cache: true,
            ..
        }) = drain(&mut s.rx).pop()
        else {
            panic!("not cached")
        };
        assert_eq!(cached, detail, "cache returns exactly what was shown");
    }

    #[tokio::test]
    async fn rejected_token_becomes_session_expired() {
        let mut s = setup(true).await;
        Mock::given(path("/activity/query"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"result": "1019", "message": "invalid"})),
            )
            .mount(&s.server)
            .await;
        s.service.run(load(9, false)).await;
        let actions = drain(&mut s.rx);
        assert!(
            matches!(&actions[..], [Action::RequestFailed { req: RequestId(9), error }] if error.kind == UiErrorKind::SessionExpired)
        );
    }

    #[tokio::test]
    async fn requests_before_sign_in_fail_cleanly() {
        let mut s = setup(false).await;
        s.service.run(load(1, false)).await;
        assert!(
            matches!(&drain(&mut s.rx)[..], [Action::RequestFailed { error, .. }] if error.kind == UiErrorKind::SessionExpired)
        );
        assert!(s.server.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn login_saves_session_and_signs_in() {
        let mut s = setup(false).await;
        Mock::given(path("/account/login"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                serde_json::json!({"result": "0000", "data": {"accessToken": "t", "userId": 7}}),
            ))
            .mount(&s.server)
            .await;
        Mock::given(path("/account/query"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                serde_json::json!({"result": "0000", "data": {"userId": 7, "nickname": "Runner"}}),
            ))
            .mount(&s.server)
            .await;
        Mock::given(path("/activity/query"))
            .respond_with(ResponseTemplate::new(200).set_body_json(page_body(1)))
            .mount(&s.server)
            .await;
        let credentials = Credentials::from_password("a@b.c", "pw");
        s.service
            .run(Command::Login {
                credentials,
                regions: vec![Region::Us],
                remember: true,
            })
            .await;
        assert_eq!(
            drain(&mut s.rx),
            vec![Action::LoginSucceeded {
                who: "Runner".into(),
                region: Region::Us
            }]
        );
        assert!(
            s.service.inner.vault.credentials().unwrap().is_some(),
            "remember = store hash"
        );

        s.service.run(load(1, false)).await;
        assert!(
            matches!(&drain(&mut s.rx)[..], [Action::ActivitiesLoaded { .. }]),
            "client is usable after login"
        );
    }

    #[tokio::test]
    async fn failed_login_reports_the_reason() {
        let mut s = setup(false).await;
        Mock::given(path("/account/login"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                serde_json::json!({"result": "1030", "message": "Incorrect password"}),
            ))
            .mount(&s.server)
            .await;
        let credentials = Credentials::from_password("a@b.c", "bad");
        s.service
            .run(Command::Login {
                credentials,
                regions: vec![Region::Us],
                remember: false,
            })
            .await;
        let actions = drain(&mut s.rx);
        assert!(
            matches!(&actions[..], [Action::LoginFailed { message }] if message.contains("Incorrect password")),
            "{actions:?}"
        );
    }

    #[test]
    fn list_keys_differ_by_query_and_page() {
        let a = ActivityQuery::default();
        let b = ActivityQuery {
            page: 2,
            ..a.clone()
        };
        let c = ActivityQuery {
            sports: vec![SportType::Run],
            ..a.clone()
        };
        assert_ne!(list_key(&a), list_key(&b));
        assert_ne!(list_key(&a), list_key(&c));
        assert_eq!(list_key(&a), "activities-any-any-all-s20-p1");
    }

    #[test]
    fn age_labels() {
        let now = Utc::now();
        assert_eq!(age_label(now, now), "just now");
        assert_eq!(
            age_label(now - chrono::Duration::minutes(5), now),
            "5 min ago"
        );
        assert_eq!(age_label(now - chrono::Duration::hours(3), now), "3 h ago");
        assert_eq!(age_label(now - chrono::Duration::days(2), now), "2 d ago");
    }
}
