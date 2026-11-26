// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 EON contributors

//! The filter lists: which exist, which are on, where each comes from, its licence, and
//! when it was last fetched.
//!
//! **The core never fetches.** [`Lists::update`] is handed a function that fetches a URL;
//! the application supplies it (madde 50c). Lists also arrive with the release through
//! [`Lists::seed`], so blocking works on first start without a single request; fetching
//! fresh copies from their publishers happens only after the person allowed it
//! ([`Lists::set_updates_allowed`]).
//!
//! What is downloaded is checked before it replaces anything: an error page, a captive
//! portal's login page or a truncated file is refused and the previous copy kept.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{Error, Result};

/// What a list is for, so the interface can group them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Purpose {
    Ads,
    Privacy,
    CookieNotices,
    Annoyances,
    Security,
    /// Exceptions that fix pages other lists break.
    Unbreak,
}

/// How a list's lines are written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Format {
    /// Adblock Plus / uBlock Origin syntax.
    Standard,
    /// `0.0.0.0 host` lines.
    Hosts,
}

/// One list in the catalogue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ListInfo {
    pub id: &'static str,
    pub title: &'static str,
    pub url: &'static str,
    pub homepage: &'static str,
    /// As the publisher states it.
    pub licence: &'static str,
    pub purpose: Purpose,
    pub format: Format,
    /// On for a new profile (madde 50e: Firefox's Strict, not its Standard).
    pub default_on: bool,
}

/// The lists EON Browser knows. Every address was checked to answer on 26 November 2026.
///
/// Not included: `turk-adlist` (hosts, CC BY-SA 3.0), last updated December 2023.
pub fn catalogue() -> &'static [ListInfo] {
    const UBO_LICENCE: &str = "GPL-3.0 (uBlock Origin's uAssets)";
    const EASYLIST_LICENCE: &str = "GPL-3.0-or-later or CC BY-SA 3.0";
    static LISTS: &[ListInfo] = &[
        ListInfo {
            id: "easylist",
            title: "EasyList",
            url: "https://easylist.to/easylist/easylist.txt",
            homepage: "https://easylist.to/",
            licence: EASYLIST_LICENCE,
            purpose: Purpose::Ads,
            format: Format::Standard,
            default_on: true,
        },
        ListInfo {
            id: "easyprivacy",
            title: "EasyPrivacy",
            url: "https://easylist.to/easylist/easyprivacy.txt",
            homepage: "https://easylist.to/",
            licence: EASYLIST_LICENCE,
            purpose: Purpose::Privacy,
            format: Format::Standard,
            default_on: true,
        },
        ListInfo {
            id: "ubo-filters",
            title: "uBlock filters",
            url: "https://ublockorigin.github.io/uAssets/filters/filters.txt",
            homepage: "https://github.com/uBlockOrigin/uAssets",
            licence: UBO_LICENCE,
            purpose: Purpose::Ads,
            format: Format::Standard,
            default_on: true,
        },
        ListInfo {
            id: "ubo-privacy",
            title: "uBlock filters – Privacy",
            url: "https://ublockorigin.github.io/uAssets/filters/privacy.txt",
            homepage: "https://github.com/uBlockOrigin/uAssets",
            licence: UBO_LICENCE,
            purpose: Purpose::Privacy,
            format: Format::Standard,
            default_on: true,
        },
        ListInfo {
            id: "ubo-badware",
            title: "uBlock filters – Badware risks",
            url: "https://ublockorigin.github.io/uAssets/filters/badware.txt",
            homepage: "https://github.com/uBlockOrigin/uAssets",
            licence: UBO_LICENCE,
            purpose: Purpose::Security,
            format: Format::Standard,
            default_on: true,
        },
        ListInfo {
            id: "ubo-unbreak",
            title: "uBlock filters – Unbreak",
            url: "https://ublockorigin.github.io/uAssets/filters/unbreak.txt",
            homepage: "https://github.com/uBlockOrigin/uAssets",
            licence: UBO_LICENCE,
            purpose: Purpose::Unbreak,
            format: Format::Standard,
            default_on: true,
        },
        ListInfo {
            id: "easylist-cookie",
            title: "EasyList Cookie List",
            url: "https://secure.fanboy.co.nz/fanboy-cookiemonster.txt",
            homepage: "https://easylist.to/",
            licence: "CC BY 3.0",
            purpose: Purpose::CookieNotices,
            format: Format::Standard,
            default_on: true,
        },
        ListInfo {
            id: "fanboy-annoyance",
            title: "Fanboy's Annoyance List",
            url: "https://secure.fanboy.co.nz/fanboy-annoyance.txt",
            homepage: "https://easylist.to/",
            licence: "CC BY 3.0",
            purpose: Purpose::Annoyances,
            format: Format::Standard,
            default_on: false,
        },
        ListInfo {
            id: "peter-lowe",
            title: "Peter Lowe's Ad and tracking server list",
            url: "https://pgl.yoyo.org/adservers/serverlist.php?hostformat=adblock&showintro=1&mimetype=plaintext",
            homepage: "https://pgl.yoyo.org/adservers/",
            licence: "McRae GPL v4",
            purpose: Purpose::Ads,
            format: Format::Standard,
            default_on: false,
        },
    ];
    LISTS
}

/// Look a list up in the catalogue.
pub fn info(id: &str) -> Option<&'static ListInfo> {
    catalogue().iter().find(|l| l.id == id)
}

/// Where the stored copy of a list came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Origin {
    /// The snapshot shipped with the release.
    Seed,
    /// Fetched from its publisher.
    Fetched,
}

/// What is known about the stored copy of a list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stored {
    /// Unix seconds when it was stored.
    pub at: u64,
    pub sha256: String,
    pub bytes: u64,
    /// The list's own `! Expires:` interval, in hours (default 96, clamped to 1..=336).
    pub expires_hours: u32,
    pub origin: Origin,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct State {
    /// `None` until the person has been asked.
    updates_allowed: Option<bool>,
    /// Lists the person turned on or off, overriding `default_on`.
    choices: BTreeMap<String, bool>,
    stored: BTreeMap<String, Stored>,
}

/// One line of the lists panel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ListStatus {
    pub info: &'static ListInfo,
    pub enabled: bool,
    pub stored: Option<Stored>,
}

/// What an update did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct UpdateReport {
    /// Lists whose content changed.
    pub updated: Vec<String>,
    /// Fetched, identical to what was stored.
    pub unchanged: Vec<String>,
    /// Not due yet.
    pub fresh: Vec<String>,
    /// `(list, reason)`; the previous copy is kept for each.
    pub failed: Vec<(String, String)>,
    /// Nothing was fetched because the person has not allowed updates.
    pub not_allowed: bool,
}

const STATE_FILE: &str = "lists.json";
const CUSTOM_FILE: &str = "custom.txt";
const MAX_LIST_BYTES: usize = 32 * 1024 * 1024;
const MIN_RULES: usize = 10;

/// The person's filter lists, stored in one folder of the profile.
#[derive(Debug)]
pub struct Lists {
    dir: PathBuf,
    state: State,
}

impl Lists {
    /// Open (or create) the lists folder.
    pub fn load(dir: &Path) -> Result<Self> {
        fs::create_dir_all(dir)?;
        let path = dir.join(STATE_FILE);
        let state = if path.exists() {
            serde_json::from_slice(&fs::read(&path)?)?
        } else {
            State::default()
        };
        Ok(Lists {
            dir: dir.to_path_buf(),
            state,
        })
    }

    /// Whether the person allowed fetching lists from their publishers; `None` means the
    /// interface has not asked yet.
    pub fn updates_allowed(&self) -> Option<bool> {
        self.state.updates_allowed
    }

    pub fn set_updates_allowed(&mut self, allowed: bool) -> Result<()> {
        self.state.updates_allowed = Some(allowed);
        self.save()
    }

    pub fn is_enabled(&self, id: &str) -> bool {
        self.state
            .choices
            .get(id)
            .copied()
            .unwrap_or_else(|| info(id).is_some_and(|l| l.default_on))
    }

    pub fn set_enabled(&mut self, id: &str, on: bool) -> Result<()> {
        if info(id).is_none() {
            return Err(Error::UnknownList(id.to_string()));
        }
        self.state.choices.insert(id.to_string(), on);
        self.save()
    }

    /// Every list in the catalogue, with whether it is on and what is stored.
    pub fn status(&self) -> Vec<ListStatus> {
        catalogue()
            .iter()
            .map(|info| ListStatus {
                info,
                enabled: self.is_enabled(info.id),
                stored: self.state.stored.get(info.id).cloned(),
            })
            .collect()
    }

    /// Store the snapshot shipped with the release, unless a newer copy is already there.
    /// `built_at` is when the snapshot was taken (Unix seconds).
    pub fn seed(&mut self, id: &str, text: &[u8], built_at: u64) -> Result<bool> {
        if let Some(s) = self.state.stored.get(id) {
            if s.at >= built_at {
                return Ok(false);
            }
        }
        self.store(id, text, built_at, Origin::Seed)?;
        Ok(true)
    }

    /// Enabled lists that have no copy or whose copy has expired at `now`.
    pub fn due(&self, now: u64) -> Vec<&'static ListInfo> {
        catalogue()
            .iter()
            .filter(|l| self.is_enabled(l.id))
            .filter(|l| match self.state.stored.get(l.id) {
                None => true,
                Some(s) => now >= s.at + u64::from(s.expires_hours) * 3600,
            })
            .collect()
    }

    /// Fetch every due list through `fetch` and store what passes the checks.
    ///
    /// Does nothing (and says so) unless the person allowed updates. One list failing does
    /// not stop the others.
    pub fn update<F>(&mut self, mut fetch: F, now: u64) -> UpdateReport
    where
        F: FnMut(&str) -> std::result::Result<Vec<u8>, String>,
    {
        let mut report = UpdateReport::default();
        if self.state.updates_allowed != Some(true) {
            report.not_allowed = true;
            return report;
        }
        let due: Vec<&'static ListInfo> = self.due(now);
        for l in catalogue().iter().filter(|l| self.is_enabled(l.id)) {
            if !due.iter().any(|d| d.id == l.id) {
                report.fresh.push(l.id.to_string());
            }
        }
        for l in due {
            let bytes = match fetch(l.url) {
                Ok(b) => b,
                Err(reason) => {
                    report.failed.push((l.id.to_string(), reason));
                    continue;
                }
            };
            let before = self.state.stored.get(l.id).map(|s| s.sha256.clone());
            match self.store(l.id, &bytes, now, Origin::Fetched) {
                Ok(()) => {
                    let after = self.state.stored.get(l.id).map(|s| s.sha256.clone());
                    if before == after {
                        report.unchanged.push(l.id.to_string());
                    } else {
                        report.updated.push(l.id.to_string());
                    }
                }
                Err(e) => report.failed.push((l.id.to_string(), e.to_string())),
            }
        }
        report
    }

    /// Check `bytes` and store them as list `id`. On any failure nothing changes.
    pub fn store(&mut self, id: &str, bytes: &[u8], now: u64, origin: Origin) -> Result<()> {
        let info = info(id).ok_or_else(|| Error::UnknownList(id.to_string()))?;
        let text = check(info, bytes)?;
        let meta = adblock::lists::read_list_metadata(text);
        let expires_hours = match meta.expires {
            Some(adblock::lists::ExpiresInterval::Hours(h)) => u32::from(h),
            Some(adblock::lists::ExpiresInterval::Days(d)) => u32::from(d) * 24,
            None => 96,
        }
        .clamp(1, 14 * 24);
        write_atomic(&self.dir.join(format!("{id}.txt")), bytes)?;
        self.state.stored.insert(
            id.to_string(),
            Stored {
                at: now,
                sha256: hex(&Sha256::digest(bytes)),
                bytes: bytes.len() as u64,
                expires_hours,
                origin,
            },
        );
        self.save()
    }

    /// The stored text of list `id`, if there is a copy.
    pub fn text(&self, id: &str) -> Result<Option<String>> {
        let path = self.dir.join(format!("{id}.txt"));
        if !self.state.stored.contains_key(id) || !path.exists() {
            return Ok(None);
        }
        let bytes = fs::read(&path)?;
        // Verify the copy on disk is the one that passed the checks.
        let expected = self.state.stored.get(id).map(|s| s.sha256.as_str());
        if expected != Some(hex(&Sha256::digest(&bytes)).as_str()) {
            return Ok(None);
        }
        Ok(String::from_utf8(bytes).ok())
    }

    /// The person's own rules, in the same syntax.
    pub fn custom_rules(&self) -> Result<String> {
        let path = self.dir.join(CUSTOM_FILE);
        if path.exists() {
            Ok(fs::read_to_string(path)?)
        } else {
            Ok(String::new())
        }
    }

    pub fn set_custom_rules(&mut self, text: &str) -> Result<()> {
        write_atomic(&self.dir.join(CUSTOM_FILE), text.as_bytes())
    }

    /// A digest of everything a compiled shield is built from: which lists are on, the
    /// exact copy of each, and the custom rules. A cached engine is valid only while this
    /// is unchanged.
    pub fn digest(&self) -> Result<String> {
        let mut h = Sha256::new();
        h.update(b"eon-browser-shield-v1\n");
        h.update(env!("CARGO_PKG_VERSION").as_bytes());
        for l in catalogue() {
            if self.is_enabled(l.id) {
                if let Some(s) = self.state.stored.get(l.id) {
                    h.update(l.id.as_bytes());
                    h.update(s.sha256.as_bytes());
                }
            }
        }
        h.update(self.custom_rules()?.as_bytes());
        Ok(hex(&h.finalize()))
    }

    fn save(&self) -> Result<()> {
        write_atomic(
            &self.dir.join(STATE_FILE),
            &serde_json::to_vec_pretty(&self.state)?,
        )
    }
}

/// Reject what is not a filter list. Returns the text.
fn check<'a>(info: &ListInfo, bytes: &'a [u8]) -> Result<&'a str> {
    let refuse = |reason: &str| Error::NotAList {
        list: info.id.to_string(),
        reason: reason.to_string(),
    };
    if bytes.len() > MAX_LIST_BYTES {
        return Err(refuse("larger than 32 MB"));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| refuse("not UTF-8 text"))?;
    let start = text.trim_start_matches('\u{feff}').trim_start();
    if start.starts_with('<') {
        return Err(refuse(
            "an HTML page (an error page or a network login page?)",
        ));
    }
    let rules = text
        .lines()
        .map(str::trim)
        .filter(|l| {
            !l.is_empty() && !l.starts_with('!') && !l.starts_with('#') && !l.starts_with('[')
        })
        .count();
    let rules = match info.format {
        Format::Standard => rules,
        Format::Hosts => text
            .lines()
            .filter(|l| {
                let mut parts = l.split_whitespace();
                matches!(parts.next(), Some("0.0.0.0" | "127.0.0.1" | "::"))
                    && parts.next().is_some()
            })
            .count(),
    };
    if rules < MIN_RULES {
        return Err(refuse("fewer than 10 rules (truncated?)"));
    }
    Ok(text)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

/// The lists that are on and have a stored copy, with their text, plus the custom rules.
pub(crate) fn enabled_texts(lists: &Lists) -> Result<(Vec<(&'static ListInfo, String)>, String)> {
    let mut out = Vec::new();
    for l in catalogue() {
        if lists.is_enabled(l.id) {
            if let Some(t) = lists.text(l.id)? {
                out.push((l, t));
            }
        }
    }
    Ok((out, lists.custom_rules()?))
}
