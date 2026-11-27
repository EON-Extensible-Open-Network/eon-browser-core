// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 EON contributors

//! The profile: history with full-text search, bookmarks, the session, per-site settings.
//! One SQLite file in the profile folder; nothing leaves it (sync, when it comes, goes
//! through EON Account's sealed vault — madde 46b, 50g).
//!
//! A private window uses [`Profile::in_memory`]: the same code, no file, gone with the
//! window.
//!
//! What is never stored: user names and passwords inside addresses
//! (`https://user:pass@host/`), and addresses that are not web pages.

use std::collections::BTreeMap;
use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::Result;

/// Per-site settings, keyed by site ([`crate::site::of`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
// Four independent switches the person flips one at a time; an enum would not be clearer.
#[allow(clippy::struct_excessive_bools)]
pub struct SiteSettings {
    /// Blocking on this site.
    pub shields: bool,
    /// JavaScript on this site.
    pub javascript: bool,
    /// The JavaScript JIT on this site. Off removes the largest class of engine exploits
    /// at a cost in speed (madde 50, the cost of Servo's missing sandbox on Windows).
    pub jit: bool,
    /// The person chose to continue on plain HTTP here.
    pub http_allowed: bool,
    /// Permission decisions the person made: `"geolocation" -> Deny`.
    pub permissions: BTreeMap<String, Decision>,
}

impl Default for SiteSettings {
    fn default() -> Self {
        SiteSettings {
            shields: true,
            javascript: true,
            jit: true,
            http_allowed: false,
            permissions: BTreeMap::new(),
        }
    }
}

/// A remembered answer to a permission prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Decision {
    Allow,
    Deny,
}

/// One history entry offered in the address bar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    pub url: String,
    pub title: Option<String>,
    pub visits: u32,
    pub bookmarked: bool,
}

/// One visit, for the history page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Visit {
    pub url: String,
    pub title: Option<String>,
    pub at: u64,
}

/// A bookmark or a folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bookmark {
    pub id: i64,
    pub parent: Option<i64>,
    pub position: i64,
    pub title: String,
    /// `None` for a folder.
    pub url: Option<String>,
    pub added: u64,
}

/// The open windows and tabs, restored at the next start.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub windows: Vec<WindowState>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowState {
    pub tabs: Vec<Tab>,
    pub active: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tab {
    pub url: String,
    pub title: Option<String>,
    #[serde(default)]
    pub pinned: bool,
    /// The Space the tab belongs to (plan `v0.0.3`); empty for the default.
    #[serde(default)]
    pub space: String,
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS pages (
    id INTEGER PRIMARY KEY,
    url TEXT NOT NULL UNIQUE,
    site TEXT NOT NULL,
    title TEXT,
    visits INTEGER NOT NULL DEFAULT 0,
    last_visit INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS pages_site ON pages(site);
CREATE TABLE IF NOT EXISTS visits (
    page INTEGER NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS visits_at ON visits(at);
CREATE VIRTUAL TABLE IF NOT EXISTS pages_fts USING fts5(
    url, title, content='pages', content_rowid='id', tokenize='unicode61 remove_diacritics 2'
);
CREATE TRIGGER IF NOT EXISTS pages_ai AFTER INSERT ON pages BEGIN
    INSERT INTO pages_fts(rowid, url, title) VALUES (new.id, new.url, new.title);
END;
CREATE TRIGGER IF NOT EXISTS pages_ad AFTER DELETE ON pages BEGIN
    INSERT INTO pages_fts(pages_fts, rowid, url, title) VALUES ('delete', old.id, old.url, old.title);
END;
CREATE TRIGGER IF NOT EXISTS pages_au AFTER UPDATE OF url, title ON pages BEGIN
    INSERT INTO pages_fts(pages_fts, rowid, url, title) VALUES ('delete', old.id, old.url, old.title);
    INSERT INTO pages_fts(rowid, url, title) VALUES (new.id, new.url, new.title);
END;
CREATE TABLE IF NOT EXISTS bookmarks (
    id INTEGER PRIMARY KEY,
    parent INTEGER REFERENCES bookmarks(id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    title TEXT NOT NULL,
    url TEXT,
    added INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS bookmarks_url ON bookmarks(url);
CREATE TABLE IF NOT EXISTS sites (
    site TEXT PRIMARY KEY,
    settings TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS session (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    state TEXT NOT NULL,
    saved INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS extension_storage (
    extension TEXT NOT NULL,
    area TEXT NOT NULL,
    key TEXT NOT NULL,
    value TEXT NOT NULL,
    PRIMARY KEY (extension, area, key)
);
";

/// The storage areas an extension may use (`chrome.storage.<area>`). `session` is kept in
/// memory by the application and never reaches this table.
pub const STORAGE_AREAS: [&str; 2] = ["local", "sync"];

/// Most an extension may keep in one area: Chrome's `storage.local` quota.
pub const STORAGE_QUOTA_BYTES: usize = 10 * 1024 * 1024;

/// A person's profile.
#[derive(Debug)]
pub struct Profile {
    db: Connection,
}

impl Profile {
    /// Open (or create) the profile in `dir`.
    pub fn open(dir: &Path) -> Result<Profile> {
        std::fs::create_dir_all(dir)?;
        let db = Connection::open(dir.join("profile.sqlite"))?;
        db.pragma_update(None, "journal_mode", "WAL")?;
        Profile::init(db)
    }

    /// A profile that lives only in memory: for private windows.
    pub fn in_memory() -> Result<Profile> {
        Profile::init(Connection::open_in_memory()?)
    }

    fn init(db: Connection) -> Result<Profile> {
        db.pragma_update(None, "foreign_keys", "ON")?;
        db.execute_batch(SCHEMA)?;
        Ok(Profile { db })
    }

    // ---------------------------------------------------------------- history

    /// Record a visit to `url` at `at` (Unix seconds). Non-web addresses are not recorded;
    /// credentials in the address are removed first.
    pub fn visit(&self, url: &Url, title: Option<&str>, at: u64) -> Result<()> {
        let Some(url) = storable(url) else {
            return Ok(());
        };
        let site = crate::site::of(&url).unwrap_or_default();
        let at = at as i64;
        self.db.execute(
            "INSERT INTO pages(url, site, title, visits, last_visit) VALUES (?1, ?2, ?3, 1, ?4)
             ON CONFLICT(url) DO UPDATE SET visits = visits + 1, last_visit = ?4,
                 title = COALESCE(?3, title)",
            params![url.as_str(), site, title, at],
        )?;
        self.db.execute(
            "INSERT INTO visits(page, at) SELECT id, ?2 FROM pages WHERE url = ?1",
            params![url.as_str(), at],
        )?;
        Ok(())
    }

    /// Set the title of a page already in history (titles arrive after the visit).
    pub fn set_title(&self, url: &Url, title: &str) -> Result<()> {
        if let Some(url) = storable(url) {
            self.db.execute(
                "UPDATE pages SET title = ?2 WHERE url = ?1",
                params![url.as_str(), title],
            )?;
        }
        Ok(())
    }

    /// Pages matching what is typed, every word as a prefix, in the address or the title;
    /// the most visited and most recent first.
    pub fn suggest(&self, typed: &str, limit: usize) -> Result<Vec<Suggestion>> {
        let words: Vec<String> = typed
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .map(|w| format!("\"{}\"*", w.replace('"', "")))
            .collect();
        if words.is_empty() {
            return Ok(Vec::new());
        }
        let query = words.join(" ");
        let mut stmt = self.db.prepare(
            "SELECT p.url, p.title, p.visits,
                    EXISTS(SELECT 1 FROM bookmarks b WHERE b.url = p.url)
             FROM pages_fts f JOIN pages p ON p.id = f.rowid
             WHERE pages_fts MATCH ?1
             ORDER BY p.visits DESC, p.last_visit DESC
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![query, limit as i64], |r| {
            Ok(Suggestion {
                url: r.get(0)?,
                title: r.get(1)?,
                visits: r.get::<_, i64>(2)? as u32,
                bookmarked: r.get(3)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// The most recent visits, newest first, before `before` (Unix seconds) if given.
    pub fn history(&self, limit: usize, before: Option<u64>) -> Result<Vec<Visit>> {
        let before = before.map_or(i64::MAX, |b| b as i64);
        let mut stmt = self.db.prepare(
            "SELECT p.url, p.title, v.at FROM visits v JOIN pages p ON p.id = v.page
             WHERE v.at < ?1 ORDER BY v.at DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![before, limit as i64], |r| {
            Ok(Visit {
                url: r.get(0)?,
                title: r.get(1)?,
                at: r.get::<_, i64>(2)? as u64,
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// Forget everything about one site: its history and its settings.
    pub fn forget_site(&self, site: &str) -> Result<()> {
        self.db
            .execute("DELETE FROM pages WHERE site = ?1", params![site])?;
        self.db
            .execute("DELETE FROM sites WHERE site = ?1", params![site])?;
        Ok(())
    }

    /// Delete visits from `since` (Unix seconds) onward; pages left with none go too.
    pub fn clear_history(&self, since: u64) -> Result<()> {
        let since = since as i64;
        self.db
            .execute("DELETE FROM visits WHERE at >= ?1", params![since])?;
        self.db.execute(
            "DELETE FROM pages WHERE NOT EXISTS (SELECT 1 FROM visits v WHERE v.page = pages.id)",
            [],
        )?;
        self.db.execute(
            "UPDATE pages SET
                visits = (SELECT COUNT(*) FROM visits v WHERE v.page = pages.id),
                last_visit = (SELECT MAX(at) FROM visits v WHERE v.page = pages.id)",
            [],
        )?;
        Ok(())
    }

    // ---------------------------------------------------------------- bookmarks

    /// Add a bookmark at the end of `parent` (`None`: the top level). Returns its id.
    pub fn add_bookmark(
        &self,
        parent: Option<i64>,
        title: &str,
        url: &Url,
        at: u64,
    ) -> Result<i64> {
        let url = storable(url).map_or_else(|| url.to_string(), |u| u.to_string());
        self.insert_bookmark(parent, title, Some(&url), at)
    }

    /// Add a folder at the end of `parent`. Returns its id.
    pub fn add_folder(&self, parent: Option<i64>, title: &str, at: u64) -> Result<i64> {
        self.insert_bookmark(parent, title, None, at)
    }

    fn insert_bookmark(
        &self,
        parent: Option<i64>,
        title: &str,
        url: Option<&str>,
        at: u64,
    ) -> Result<i64> {
        let position: i64 = self.db.query_row(
            "SELECT COALESCE(MAX(position) + 1, 0) FROM bookmarks WHERE parent IS ?1",
            params![parent],
            |r| r.get(0),
        )?;
        self.db.execute(
            "INSERT INTO bookmarks(parent, position, title, url, added) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![parent, position, title, url, at as i64],
        )?;
        Ok(self.db.last_insert_rowid())
    }

    /// The bookmarks and folders directly inside `parent`, in order.
    pub fn bookmarks(&self, parent: Option<i64>) -> Result<Vec<Bookmark>> {
        let mut stmt = self.db.prepare(
            "SELECT id, parent, position, title, url, added FROM bookmarks
             WHERE parent IS ?1 ORDER BY position",
        )?;
        let rows = stmt.query_map(params![parent], |r| {
            Ok(Bookmark {
                id: r.get(0)?,
                parent: r.get(1)?,
                position: r.get(2)?,
                title: r.get(3)?,
                url: r.get(4)?,
                added: r.get::<_, i64>(5)? as u64,
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// The bookmark for `url`, if there is one.
    pub fn bookmark_for(&self, url: &Url) -> Result<Option<i64>> {
        let url = storable(url).map_or_else(|| url.to_string(), |u| u.to_string());
        Ok(self
            .db
            .query_row(
                "SELECT id FROM bookmarks WHERE url = ?1",
                params![url],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// Remove a bookmark, or a folder with everything in it.
    pub fn remove_bookmark(&self, id: i64) -> Result<()> {
        self.db
            .execute("DELETE FROM bookmarks WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// Rename a bookmark or folder.
    pub fn rename_bookmark(&self, id: i64, title: &str) -> Result<()> {
        self.db.execute(
            "UPDATE bookmarks SET title = ?2 WHERE id = ?1",
            params![id, title],
        )?;
        Ok(())
    }

    // ---------------------------------------------------------------- sites

    /// The settings for the site of `url` (defaults if the person never changed any).
    pub fn site(&self, url: &Url) -> Result<SiteSettings> {
        let Some(site) = crate::site::of(url) else {
            return Ok(SiteSettings::default());
        };
        let json: Option<String> = self
            .db
            .query_row(
                "SELECT settings FROM sites WHERE site = ?1",
                params![site],
                |r| r.get(0),
            )
            .optional()?;
        match json {
            Some(j) => Ok(serde_json::from_str(&j)?),
            None => Ok(SiteSettings::default()),
        }
    }

    /// Store settings for the site of `url`. Settings equal to the defaults are removed,
    /// so the table lists exactly the sites the person changed something on.
    pub fn set_site(&self, url: &Url, settings: &SiteSettings) -> Result<()> {
        let Some(site) = crate::site::of(url) else {
            return Ok(());
        };
        if *settings == SiteSettings::default() {
            self.db
                .execute("DELETE FROM sites WHERE site = ?1", params![site])?;
        } else {
            self.db.execute(
                "INSERT INTO sites(site, settings) VALUES (?1, ?2)
                 ON CONFLICT(site) DO UPDATE SET settings = ?2",
                params![site, serde_json::to_string(settings)?],
            )?;
        }
        Ok(())
    }

    /// Every site with settings that differ from the defaults.
    pub fn changed_sites(&self) -> Result<Vec<(String, SiteSettings)>> {
        let mut stmt = self
            .db
            .prepare("SELECT site, settings FROM sites ORDER BY site")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        let mut out = Vec::new();
        for row in rows {
            let (site, json) = row?;
            out.push((site, serde_json::from_str(&json)?));
        }
        Ok(out)
    }

    // ---------------------------------------------------------------- extension storage

    /// `chrome.storage.<area>.get`: the values stored under `keys` (all when `None`), as
    /// JSON text by key.
    pub fn ext_storage_get(
        &self,
        extension: &str,
        area: &str,
        keys: Option<&[String]>,
    ) -> Result<BTreeMap<String, String>> {
        let mut stmt = self.db.prepare(
            "SELECT key, value FROM extension_storage WHERE extension = ?1 AND area = ?2",
        )?;
        let rows = stmt.query_map(params![extension, area], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;
        let mut out = BTreeMap::new();
        for row in rows {
            let (k, v) = row?;
            if keys.is_none_or(|ks| ks.contains(&k)) {
                out.insert(k, v);
            }
        }
        Ok(out)
    }

    /// `chrome.storage.<area>.set`: each value is JSON text. Refused as a whole if the
    /// area would grow past [`STORAGE_QUOTA_BYTES`].
    pub fn ext_storage_set(
        &self,
        extension: &str,
        area: &str,
        items: &BTreeMap<String, String>,
    ) -> Result<()> {
        let mut size: usize = self
            .ext_storage_get(extension, area, None)?
            .iter()
            .filter(|(k, _)| !items.contains_key(*k))
            .map(|(k, v)| k.len() + v.len())
            .sum();
        size += items.iter().map(|(k, v)| k.len() + v.len()).sum::<usize>();
        if size > STORAGE_QUOTA_BYTES {
            return Err(crate::Error::BadUrl(format!(
                "storage quota exceeded for {extension} ({size} bytes)"
            )));
        }
        let tx = self.db.unchecked_transaction()?;
        for (k, v) in items {
            tx.execute(
                "INSERT INTO extension_storage(extension, area, key, value) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(extension, area, key) DO UPDATE SET value = ?4",
                params![extension, area, k, v],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// `chrome.storage.<area>.remove`, or `clear` when `keys` is `None`.
    pub fn ext_storage_remove(
        &self,
        extension: &str,
        area: &str,
        keys: Option<&[String]>,
    ) -> Result<()> {
        match keys {
            None => {
                self.db.execute(
                    "DELETE FROM extension_storage WHERE extension = ?1 AND area = ?2",
                    params![extension, area],
                )?;
            }
            Some(ks) => {
                for k in ks {
                    self.db.execute(
                        "DELETE FROM extension_storage WHERE extension = ?1 AND area = ?2 AND key = ?3",
                        params![extension, area, k],
                    )?;
                }
            }
        }
        Ok(())
    }

    /// Everything an extension stored, gone (when it is removed).
    pub fn ext_storage_forget(&self, extension: &str) -> Result<()> {
        self.db.execute(
            "DELETE FROM extension_storage WHERE extension = ?1",
            params![extension],
        )?;
        Ok(())
    }

    // ---------------------------------------------------------------- session

    pub fn save_session(&self, session: &Session, at: u64) -> Result<()> {
        self.db.execute(
            "INSERT INTO session(id, state, saved) VALUES (1, ?1, ?2)
             ON CONFLICT(id) DO UPDATE SET state = ?1, saved = ?2",
            params![serde_json::to_string(session)?, at as i64],
        )?;
        Ok(())
    }

    pub fn load_session(&self) -> Result<Option<Session>> {
        let json: Option<String> = self
            .db
            .query_row("SELECT state FROM session WHERE id = 1", [], |r| r.get(0))
            .optional()?;
        json.map(|j| serde_json::from_str(&j).map_err(Into::into))
            .transpose()
    }
}

/// The form of `url` that may be stored: web pages only, without credentials.
fn storable(url: &Url) -> Option<Url> {
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    let mut u = url.clone();
    if !u.username().is_empty() || u.password().is_some() {
        let _ = u.set_username("");
        let _ = u.set_password(None);
    }
    Some(u)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> Url {
        Url::parse(s).unwrap_or_else(|e| unreachable!("{s}: {e}"))
    }

    fn p() -> Profile {
        Profile::in_memory().unwrap_or_else(|e| unreachable!("{e}"))
    }

    #[test]
    fn history_and_suggestions() -> Result<()> {
        let p = p();
        p.visit(
            &u("https://en.wikipedia.org/wiki/Servo_(software)"),
            Some("Servo (software) - Wikipedia"),
            100,
        )?;
        p.visit(
            &u("https://en.wikipedia.org/wiki/Servo_(software)"),
            None,
            200,
        )?;
        p.visit(
            &u("https://www.rust-lang.org/"),
            Some("Rust Programming Language"),
            150,
        )?;
        p.visit(
            &u("https://www.ankara.bel.tr/"),
            Some("Ankara Büyükşehir Belediyesi"),
            160,
        )?;

        let s = p.suggest("wiki", 10)?;
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].visits, 2);
        assert_eq!(
            s[0].title.as_deref(),
            Some("Servo (software) - Wikipedia"),
            "a later visit without a title keeps it"
        );

        assert_eq!(p.suggest("rust prog", 10)?.len(), 1);
        // Diacritics are folded: "buyuksehir" finds "Büyükşehir".
        assert_eq!(p.suggest("buyuksehir", 10)?.len(), 1);
        assert!(p.suggest("", 10)?.is_empty());
        assert!(p.suggest("\"; DROP TABLE pages; --", 10)?.is_empty());

        let h = p.history(10, None)?;
        assert_eq!(
            h.iter().map(|v| v.at).collect::<Vec<_>>(),
            vec![200, 160, 150, 100]
        );
        Ok(())
    }

    #[test]
    fn never_stores_credentials_or_non_web_addresses() -> Result<()> {
        let p = p();
        p.visit(
            &u("https://alice:hunter2@example.com/inbox"),
            Some("Inbox"),
            1,
        )?;
        p.visit(&u("file:///C:/Users/x/secret.html"), Some("secret"), 2)?;
        p.visit(&u("data:text/html,hi"), None, 3)?;
        let h = p.history(10, None)?;
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].url, "https://example.com/inbox");
        Ok(())
    }

    #[test]
    fn clearing_and_forgetting() -> Result<()> {
        let p = p();
        p.visit(&u("https://a.example.com/"), None, 10)?;
        p.visit(&u("https://b.example.com/"), None, 20)?;
        p.visit(&u("https://other.org/"), None, 30)?;
        p.forget_site("example.com")?;
        assert_eq!(p.history(10, None)?.len(), 1);
        p.visit(&u("https://other.org/"), None, 40)?;
        p.clear_history(35)?;
        let h = p.history(10, None)?;
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].at, 30);
        assert_eq!(p.suggest("other", 10)?[0].visits, 1);
        Ok(())
    }

    #[test]
    fn bookmarks() -> Result<()> {
        let p = p();
        let folder = p.add_folder(None, "Rust", 1)?;
        let a = p.add_bookmark(Some(folder), "Servo", &u("https://servo.org/"), 2)?;
        p.add_bookmark(
            Some(folder),
            "Book",
            &u("https://doc.rust-lang.org/book/"),
            3,
        )?;
        p.add_bookmark(
            None,
            "EON",
            &u("https://github.com/EON-Extensible-Open-Network"),
            4,
        )?;
        assert_eq!(p.bookmarks(None)?.len(), 2);
        let inside = p.bookmarks(Some(folder))?;
        assert_eq!(
            inside.iter().map(|b| b.position).collect::<Vec<_>>(),
            vec![0, 1]
        );
        assert_eq!(p.bookmark_for(&u("https://servo.org/"))?, Some(a));
        p.visit(&u("https://servo.org/"), Some("Servo"), 5)?;
        assert!(p.suggest("servo", 5)?[0].bookmarked);
        p.rename_bookmark(a, "Servo engine")?;
        p.remove_bookmark(folder)?;
        assert_eq!(
            p.bookmarks(Some(folder))?.len(),
            0,
            "a folder takes its contents with it"
        );
        assert_eq!(p.bookmark_for(&u("https://servo.org/"))?, None);
        Ok(())
    }

    #[test]
    fn site_settings_by_site_and_only_when_changed() -> Result<()> {
        let p = p();
        let page = u("https://news.example.co.uk/a");
        assert_eq!(p.site(&page)?, SiteSettings::default());
        let mut s = SiteSettings {
            shields: false,
            ..SiteSettings::default()
        };
        s.permissions.insert("geolocation".into(), Decision::Deny);
        p.set_site(&page, &s)?;
        assert_eq!(
            p.site(&u("https://www.example.co.uk/"))?,
            s,
            "same site, other host"
        );
        assert_eq!(p.changed_sites()?.len(), 1);
        p.set_site(&page, &SiteSettings::default())?;
        assert!(p.changed_sites()?.is_empty());
        Ok(())
    }

    #[test]
    fn extension_storage_by_extension_and_area() -> Result<()> {
        let p = p();
        let set = |pairs: &[(&str, &str)]| -> BTreeMap<String, String> {
            pairs
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect()
        };
        p.ext_storage_set(
            "dark-reader",
            "local",
            &set(&[("enabled", "true"), ("theme", "{\"mode\":1}")]),
        )?;
        p.ext_storage_set("dark-reader", "sync", &set(&[("enabled", "false")]))?;
        p.ext_storage_set("other", "local", &set(&[("enabled", "\"x\"")]))?;
        let all = p.ext_storage_get("dark-reader", "local", None)?;
        assert_eq!(all.len(), 2);
        assert_eq!(all["theme"], "{\"mode\":1}");
        let one = p.ext_storage_get("dark-reader", "local", Some(&["enabled".to_string()]))?;
        assert_eq!(one.len(), 1);
        assert_eq!(
            p.ext_storage_get("dark-reader", "sync", None)?["enabled"],
            "false"
        );
        p.ext_storage_set("dark-reader", "local", &set(&[("enabled", "false")]))?;
        assert_eq!(
            p.ext_storage_get("dark-reader", "local", None)?["enabled"],
            "false"
        );
        p.ext_storage_remove("dark-reader", "local", Some(&["theme".to_string()]))?;
        assert_eq!(p.ext_storage_get("dark-reader", "local", None)?.len(), 1);
        let big = "x".repeat(STORAGE_QUOTA_BYTES);
        assert!(p
            .ext_storage_set("dark-reader", "local", &set(&[("big", big.as_str())]))
            .is_err());
        p.ext_storage_forget("dark-reader")?;
        assert!(p.ext_storage_get("dark-reader", "sync", None)?.is_empty());
        assert_eq!(
            p.ext_storage_get("other", "local", None)?.len(),
            1,
            "other extensions keep theirs"
        );
        Ok(())
    }

    #[test]
    fn session_round_trip_and_on_disk() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let session = Session {
            windows: vec![WindowState {
                tabs: vec![
                    Tab {
                        url: "https://servo.org/".into(),
                        title: Some("Servo".into()),
                        pinned: true,
                        space: String::new(),
                    },
                    Tab {
                        url: "https://example.com/".into(),
                        ..Tab::default()
                    },
                ],
                active: 1,
            }],
        };
        {
            let p = Profile::open(dir.path())?;
            assert_eq!(p.load_session()?, None);
            p.save_session(&session, 9)?;
            p.visit(&u("https://servo.org/"), Some("Servo"), 9)?;
        }
        let p = Profile::open(dir.path())?;
        assert_eq!(p.load_session()?, Some(session));
        assert_eq!(p.suggest("servo", 1)?.len(), 1);
        Ok(())
    }
}
