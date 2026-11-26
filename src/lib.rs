// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 EON contributors

//! The core of **EON Browser** (plan madde 50): the policy that protects a person while they
//! browse, written once and independent of the engine.
//!
//! - [`lists`] — the filter lists a person can turn on, where each comes from and its
//!   licence; updating them through a fetch function the *application* supplies.
//! - [`shield`] — blocking ads, trackers and cookie banners, over Brave's `adblock` engine.
//! - [`clean`] — tracking parameters stripped and redirect wrappers unwrapped.
//! - [`upgrade`] — HTTPS-only, with the honest exemptions.
//! - [`omnibox`] and [`search`] — what a typed line means, and the search engines.
//! - [`profile`] — history with full-text search, bookmarks, the session, per-site settings.
//! - [`fingerprint`] — the user agent and the page-start script.
//! - [`site`] — which site an address belongs to.
//!
//! The core has no engine, no window and **no network access of its own**; the application
//! fetches and the core decides. It never collects anything about the person (madde 36).

pub mod clean;
mod error;
pub mod fingerprint;
pub mod lists;
pub mod omnibox;
pub mod profile;
pub mod search;
pub mod shield;
pub mod site;
pub mod upgrade;

pub use error::{Error, Result};

/// Everything a navigation goes through before the engine loads it: redirect wrappers
/// unwrapped, tracking parameters stripped, then upgraded to HTTPS.
///
/// `http_allowed` is the person's own per-site exception to HTTPS-only, from
/// [`profile::SiteSettings::http_allowed`].
pub fn navigate(url: &url::Url, http_allowed: bool) -> Navigation {
    let cleaned = clean::clean(url);
    let after_clean = cleaned.as_ref().map_or(url, |c| &c.url);
    let upgraded = if http_allowed {
        upgrade::Upgrade::Exempt(upgrade::Exemption::PersonAllowed)
    } else {
        upgrade::https_only(after_clean)
    };
    let final_url = match &upgraded {
        upgrade::Upgrade::Upgraded(u) => u.clone(),
        _ => after_clean.clone(),
    };
    Navigation {
        changed: &final_url != url,
        url: final_url,
        cleaned,
        upgrade: upgraded,
    }
}

/// What [`navigate`] did, so the interface can say it ("2 tracking parameters removed").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Navigation {
    /// The address to load.
    pub url: url::Url,
    /// Whether it differs from the address asked for.
    pub changed: bool,
    /// The cleaning step, if it changed anything.
    pub cleaned: Option<clean::Cleaned>,
    /// The HTTPS step.
    pub upgrade: upgrade::Upgrade,
}
