//! On-disk cache of model data (docs/ARCHITECTURE.md §6).
//!
//! One JSON file per key under `<cache dir>/<user id>/`, written atomically
//! with owner-only permissions. Anything unreadable (old schema, corrupted,
//! another version) is treated as a miss, never an error.

use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use color_eyre::Result;
use color_eyre::eyre::WrapErr;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::paths::ensure_private_dir;

/// Bump when cached model types change shape; old entries are then ignored.
const SCHEMA: u32 = 1;

#[derive(Debug, Clone)]
pub struct Cache {
    dir: PathBuf,
}

#[derive(Serialize, Deserialize)]
struct Entry<T> {
    schema: u32,
    fetched_at: DateTime<Utc>,
    value: T,
}

impl Cache {
    /// A cache for one account (ids are scrubbed of path characters).
    pub fn for_user(root: &Path, user_id: &str) -> Self {
        let user = sanitize(if user_id.is_empty() {
            "default"
        } else {
            user_id
        });
        Self {
            dir: root.join(user),
        }
    }

    #[cfg(test)]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The value and when it was fetched, or `None` on any problem.
    pub fn get<T: DeserializeOwned>(&self, key: &str) -> Option<(T, DateTime<Utc>)> {
        let path = self.path(key);
        let text = std::fs::read_to_string(&path).ok()?;
        match serde_json::from_str::<Entry<T>>(&text) {
            Ok(entry) if entry.schema == SCHEMA => Some((entry.value, entry.fetched_at)),
            Ok(_) => None,
            Err(err) => {
                tracing::debug!(%err, key, "ignoring unreadable cache entry");
                None
            }
        }
    }

    pub fn put<T: Serialize>(&self, key: &str, value: &T, fetched_at: DateTime<Utc>) -> Result<()> {
        ensure_private_dir(&self.dir)?;
        let path = self.path(key);
        let tmp = path.with_extension("json.tmp");
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&tmp)
            .wrap_err_with(|| format!("cannot write {}", tmp.display()))?;
        serde_json::to_writer(
            &mut file,
            &Entry {
                schema: SCHEMA,
                fetched_at,
                value,
            },
        )?;
        file.flush()?;
        std::fs::rename(&tmp, &path).wrap_err("cannot replace cache entry")?;
        Ok(())
    }

    fn path(&self, key: &str) -> PathBuf {
        self.dir.join(format!("{}.json", sanitize(key)))
    }
}

/// Total size in bytes of everything under `root` (all accounts).
pub fn size(root: &Path) -> u64 {
    fn walk(path: &Path) -> u64 {
        match std::fs::read_dir(path) {
            Ok(entries) => entries
                .flatten()
                .map(|e| match e.metadata() {
                    Ok(m) if m.is_dir() => walk(&e.path()),
                    Ok(m) => m.len(),
                    Err(_) => 0,
                })
                .sum(),
            Err(_) => 0,
        }
    }
    walk(root)
}

/// Delete the whole cache (all accounts). Returns the bytes freed.
pub fn clear(root: &Path) -> Result<u64> {
    let bytes = size(root);
    match std::fs::remove_dir_all(root) {
        Ok(()) => Ok(bytes),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(err) => Err(err).wrap_err_with(|| format!("cannot delete {}", root.display())),
    }
}

/// Keep keys to `[A-Za-z0-9_.-]` so they are safe file names.
fn sanitize(key: &str) -> String {
    key.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_start_matches('.')
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_get_round_trip_with_timestamp() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = Cache::for_user(tmp.path(), "42");
        let at = DateTime::from_timestamp(1_792_280_520, 0).unwrap();
        cache.put("activities-p1", &vec![1, 2, 3], at).unwrap();
        assert_eq!(
            cache.get::<Vec<i32>>("activities-p1"),
            Some((vec![1, 2, 3], at))
        );
        assert_eq!(cache.get::<Vec<i32>>("missing"), None);
        assert!(cache.dir().ends_with("42"));
    }

    #[test]
    fn wrong_shape_old_schema_or_garbage_is_a_miss() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = Cache::for_user(tmp.path(), "42");
        cache.put("k", &"text", Utc::now()).unwrap();
        assert_eq!(cache.get::<Vec<i32>>("k"), None, "wrong type");
        std::fs::write(
            cache.path("old"),
            r#"{"schema":0,"fetched_at":"2026-01-01T00:00:00Z","value":1}"#,
        )
        .unwrap();
        assert_eq!(cache.get::<i32>("old"), None, "old schema");
        std::fs::write(cache.path("bad"), "{not json").unwrap();
        assert_eq!(cache.get::<i32>("bad"), None, "garbage");
    }

    #[test]
    fn keys_and_users_cannot_escape_the_directory() {
        assert_eq!(sanitize("../../etc/passwd"), "_.._etc_passwd");
        assert_eq!(sanitize("detail-4761/x"), "detail-4761_x");
        let tmp = tempfile::tempdir().unwrap();
        let cache = Cache::for_user(tmp.path(), "../evil");
        assert!(cache.dir().starts_with(tmp.path()));
        assert!(cache.path("../../x").starts_with(cache.dir()));
    }

    #[cfg(unix)]
    #[test]
    fn entries_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let cache = Cache::for_user(tmp.path(), "42");
        cache.put("k", &1, Utc::now()).unwrap();
        let mode = std::fs::metadata(cache.path("k"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn size_and_clear() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("cache");
        Cache::for_user(&root, "a")
            .put("k", &"x".repeat(100), Utc::now())
            .unwrap();
        Cache::for_user(&root, "b")
            .put("k", &"y".repeat(100), Utc::now())
            .unwrap();
        assert!(size(&root) > 200);
        assert!(clear(&root).unwrap() > 200);
        assert_eq!(size(&root), 0);
        assert_eq!(clear(&root).unwrap(), 0, "clearing twice is fine");
    }
}
