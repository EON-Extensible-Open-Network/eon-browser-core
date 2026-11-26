// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 EON contributors

//! What a line typed in the address bar means: an address, a search, or one of the
//! browser's own pages.
//!
//! The rule that matters for privacy: **a search must never be mistaken for an address,
//! and an address must never be sent to a search engine.** "notes.txt" is not a site;
//! "wikipedia.org" is; "my bank password" is a search, never a DNS lookup.

use url::Url;

use crate::search::{self, SearchEngine};

/// What a typed line means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// Open this address.
    Url(Url),
    /// Search for `query` with the engine whose id is `engine` (the person's default, or
    /// the one named by an `@keyword`).
    Search { engine: String, query: String },
    /// One of the browser's own pages: `eon:settings`, `eon:history`…
    Internal(String),
    /// A search, but no engine has been chosen yet: the interface asks.
    NeedsEngine { query: String },
}

/// Classify `input`. `default_engine` is the id of the person's chosen engine, if any.
pub fn classify(
    input: &str,
    engines: &[SearchEngine],
    default_engine: Option<&str>,
) -> Option<Target> {
    let text = input.trim();
    if text.is_empty() {
        return None;
    }

    // `@keyword query` — one search with another engine.
    if let Some(rest) = text.strip_prefix('@') {
        let (kw, query) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
        if let Some(e) = search::by_keyword(engines, &format!("@{kw}")) {
            return Some(Target::Search {
                engine: e.id.clone(),
                query: query.trim().to_string(),
            });
        }
    }

    // `?anything` — always a search, even if it looks like an address.
    if let Some(q) = text.strip_prefix('?') {
        return Some(search_for(q.trim(), default_engine));
    }

    if let Some(page) = internal(text) {
        return Some(Target::Internal(page));
    }

    if let Some(url) = as_address(text) {
        return Some(Target::Url(url));
    }

    Some(search_for(text, default_engine))
}

fn search_for(query: &str, default_engine: Option<&str>) -> Target {
    match default_engine {
        Some(e) => Target::Search {
            engine: e.to_string(),
            query: query.to_string(),
        },
        None => Target::NeedsEngine {
            query: query.to_string(),
        },
    }
}

fn internal(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    let page = lower
        .strip_prefix("eon:")
        .or_else(|| lower.strip_prefix("about:"))?;
    let page = page.trim_start_matches('/');
    (!page.is_empty() && page.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
        .then(|| page.to_string())
}

/// `text` as an address, or `None` if it should be searched.
fn as_address(text: &str) -> Option<Url> {
    if text.chars().any(char::is_whitespace) {
        return None;
    }
    // An explicit scheme the browser opens.
    if let Some((scheme, _)) = text.split_once(':') {
        let scheme = scheme.to_ascii_lowercase();
        if matches!(scheme.as_str(), "http" | "https" | "file") && text.contains(':') {
            if let Ok(u) = Url::parse(text) {
                if u.has_host() || scheme == "file" {
                    return Some(u);
                }
            }
        }
        // `javascript:` and `data:` typed by hand are how "paste this in your address bar"
        // scams work; they are searched, never run.
        if matches!(scheme.as_str(), "javascript" | "data" | "vbscript") {
            return None;
        }
    }

    // Without a scheme: decide from the host.
    let candidate = Url::parse(&format!("http://{text}")).ok()?;
    let host = candidate.host_str()?;
    let local = crate::site::is_local(&candidate);
    let ip = matches!(
        candidate.host(),
        Some(url::Host::Ipv4(_) | url::Host::Ipv6(_))
    );
    let has_port_or_path =
        candidate.port().is_some() || text.contains('/') || candidate.query().is_some();

    let is_address = if ip {
        true
    } else if host.contains('.') {
        // A real public suffix (`.com`, `.com.tr`, `.dev`) or a local-network name.
        crate::site::has_known_suffix(host)
            || (local && host.rsplit('.').next() != Some("test"))
            || host.rsplit('.').next() == Some("onion")
    } else {
        // A single word is a search, unless it is clearly a local machine with a port or
        // path (`localhost:3000`, `router/admin`) or `localhost` itself.
        host == "localhost" || (local && has_port_or_path)
    };
    if !is_address {
        return None;
    }
    // HTTPS first; local addresses and IPs stay on HTTP, where they live.
    if local || ip {
        Some(candidate)
    } else {
        Url::parse(&format!("https://{text}")).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn go(s: &str) -> Option<Target> {
        classify(s, &search::catalogue(), Some("duckduckgo"))
    }

    fn url(s: &str) -> Option<String> {
        match go(s) {
            Some(Target::Url(u)) => Some(u.to_string()),
            _ => None,
        }
    }

    fn is_search(s: &str) -> bool {
        matches!(go(s), Some(Target::Search { .. }))
    }

    #[test]
    fn addresses() {
        assert_eq!(
            url("wikipedia.org").as_deref(),
            Some("https://wikipedia.org/")
        );
        assert_eq!(
            url("www.sahibinden.com.tr/ilan").as_deref(),
            Some("https://www.sahibinden.com.tr/ilan")
        );
        assert_eq!(url("https://example.com/a b").as_deref(), None);
        assert_eq!(
            url("http://neverssl.com").as_deref(),
            Some("http://neverssl.com/")
        );
        assert_eq!(
            url("localhost:5173").as_deref(),
            Some("http://localhost:5173/")
        );
        assert_eq!(url("localhost").as_deref(), Some("http://localhost/"));
        assert_eq!(url("192.168.1.1").as_deref(), Some("http://192.168.1.1/"));
        assert_eq!(url("[::1]:8080").as_deref(), Some("http://[::1]:8080/"));
        assert_eq!(
            url("printer.local").as_deref(),
            Some("http://printer.local/")
        );
        assert_eq!(
            url("servo.org:443/x").as_deref(),
            Some("https://servo.org/x")
        );
        assert_eq!(
            url("file:///C:/notes.html").as_deref(),
            Some("file:///C:/notes.html")
        );
    }

    #[test]
    fn searches() {
        for s in [
            "rust servo",
            "notes.txt",
            "my bank password",
            "çay demleme",
            "servo",
            "what is 2+2",
            "javascript:alert(document.cookie)",
            "data:text/html,<script>x</script>",
            "C++",
        ] {
            assert!(is_search(s), "{s} should be a search, got {:?}", go(s));
        }
    }

    #[test]
    fn keywords_and_forced_search() {
        assert_eq!(
            go("@w Ankara"),
            Some(Target::Search {
                engine: "wikipedia".into(),
                query: "Ankara".into()
            })
        );
        assert_eq!(
            go("?wikipedia.org"),
            Some(Target::Search {
                engine: "duckduckgo".into(),
                query: "wikipedia.org".into()
            })
        );
        // An unknown keyword is just text.
        assert!(is_search("@nobody hello"));
    }

    #[test]
    fn internal_pages() {
        assert_eq!(
            go("eon:settings"),
            Some(Target::Internal("settings".into()))
        );
        assert_eq!(go("about:blank"), Some(Target::Internal("blank".into())));
    }

    #[test]
    fn no_engine_chosen_yet_means_ask() {
        assert_eq!(
            classify("rust", &search::catalogue(), None),
            Some(Target::NeedsEngine {
                query: "rust".into()
            })
        );
        assert_eq!(classify("   ", &search::catalogue(), None), None);
    }
}
