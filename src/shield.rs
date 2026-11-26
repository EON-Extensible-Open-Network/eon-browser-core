// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 EON contributors

//! Blocking ads, trackers and cookie notices, over Brave's `adblock` engine.
//!
//! The application calls [`Shield::check`] for **every request the engine makes** (in
//! Servo, from `WebViewDelegate::load_web_resource`) and [`Shield::cosmetic`] once per page
//! for the elements to hide. Inside the browser there is no Manifest V3 ceiling: every rule
//! of every list is in force.
//!
//! Whether blocking applies to a site at all is the person's per-site switch
//! ([`crate::profile::SiteSettings::shields`]); the application checks it first.

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use adblock::lists::{FilterFormat, FilterSet, ParseOptions};
use adblock::request::Request as AbRequest;
use adblock::Engine;

use crate::lists::{self, Format, Lists};
use crate::Result;

/// What kind of resource a request loads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A top-level page.
    Document,
    /// A frame inside a page.
    Subdocument,
    Script,
    Stylesheet,
    Image,
    Font,
    Media,
    /// `fetch` and `XMLHttpRequest`.
    Xhr,
    WebSocket,
    /// `navigator.sendBeacon` and `<a ping>`.
    Ping,
    Other,
}

impl Kind {
    fn adblock_name(self) -> &'static str {
        match self {
            Kind::Document => "document",
            Kind::Subdocument => "subdocument",
            Kind::Script => "script",
            Kind::Stylesheet => "stylesheet",
            Kind::Image => "image",
            Kind::Font => "font",
            Kind::Media => "media",
            Kind::Xhr => "xmlhttprequest",
            Kind::WebSocket => "websocket",
            Kind::Ping => "ping",
            Kind::Other => "other",
        }
    }

    /// The kind implied by an HTTP `Sec-Fetch-Dest` / Fetch "destination" value, which is
    /// what an engine knows about a request.
    pub fn from_destination(dest: &str) -> Kind {
        match dest {
            "document" => Kind::Document,
            "iframe" | "frame" | "embed" | "object" => Kind::Subdocument,
            "script" | "worker" | "sharedworker" | "serviceworker" | "audioworklet"
            | "paintworklet" => Kind::Script,
            "style" => Kind::Stylesheet,
            "image" => Kind::Image,
            "font" => Kind::Font,
            "audio" | "video" | "track" => Kind::Media,
            "empty" => Kind::Xhr,
            "report" => Kind::Ping,
            _ => Kind::Other,
        }
    }
}

/// One request, as the engine is about to make it.
#[derive(Debug, Clone, Copy)]
pub struct Request<'a> {
    /// What is requested.
    pub url: &'a str,
    /// The page that asked for it (the top-level document), for first-/third-party.
    pub source: &'a str,
    pub kind: Kind,
}

/// The decision for one request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Allow,
    /// Do not load it. `rule` is the filter that matched, when the engine kept it.
    Block {
        rule: Option<String>,
    },
    /// Load this harmless stand-in instead (a `data:` URL), so the page does not break
    /// waiting for what was blocked.
    Redirect {
        data_url: String,
    },
}

/// What to hide on a page and what to run in it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Cosmetic {
    /// A stylesheet: one `display: none !important` rule per selector, so a selector the
    /// engine does not understand invalidates only itself.
    pub css: String,
    /// Scriptlets to run at document start. Empty until uBlock Origin's resources are
    /// loaded (plan: `v0.0.2`).
    pub script: String,
    /// The page turned off generic hiding (`$generichide`).
    pub generichide: bool,
    /// Classes and ids this page excepts from generic rules, for [`Shield::generic_css`].
    pub exceptions: Vec<String>,
}

/// A compiled set of rules.
pub struct Shield {
    engine: Engine,
    rules: usize,
}

impl std::fmt::Debug for Shield {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Shield")
            .field("rules", &self.rules)
            .finish_non_exhaustive()
    }
}

impl Shield {
    /// Compile every enabled list that has a stored copy, and the custom rules.
    pub fn build(lists: &Lists) -> Result<Shield> {
        let (texts, custom) = lists::enabled_texts(lists)?;
        let mut set = FilterSet::new(false);
        let mut rules = 0;
        for (info, text) in texts {
            let format = match info.format {
                Format::Standard => FilterFormat::Standard,
                Format::Hosts => FilterFormat::Hosts,
            };
            set.add_filter_list(
                text,
                ParseOptions {
                    format,
                    ..ParseOptions::default()
                },
            );
            rules += 1;
        }
        if !custom.trim().is_empty() {
            set.add_filter_list(custom, ParseOptions::default());
            rules += 1;
        }
        Ok(Shield {
            engine: Engine::new_with_filter_set(set),
            rules,
        })
    }

    /// [`Shield::build`], reusing a compiled copy in `cache_dir` when nothing it was built
    /// from has changed. Compiling the default lists takes a moment; loading the cache is
    /// close to instant, which is part of starting fast.
    pub fn build_cached(lists: &Lists, cache_dir: &Path) -> Result<Shield> {
        fs::create_dir_all(cache_dir)?;
        let key = lists.digest()?;
        let path = cache_dir.join(format!("shield-{}.bin", &key[..16]));
        if let Ok(bytes) = fs::read(&path) {
            let mut engine = Engine::default();
            if engine.deserialize(&bytes).is_ok() {
                let (texts, custom) = lists::enabled_texts(lists)?;
                let rules = texts.len() + usize::from(!custom.trim().is_empty());
                return Ok(Shield { engine, rules });
            }
        }
        let shield = Shield::build(lists)?;
        // Remove older compiled copies; one is enough.
        if let Ok(entries) = fs::read_dir(cache_dir) {
            for e in entries.flatten() {
                let name = e.file_name();
                let name = name.to_string_lossy();
                if name.starts_with("shield-") && name.ends_with(".bin") {
                    let _ = fs::remove_file(e.path());
                }
            }
        }
        fs::write(&path, shield.engine.serialize())?;
        Ok(shield)
    }

    /// A shield from rules written out, for tests and for checking a rule before saving it.
    pub fn from_rules(rules: &str) -> Shield {
        Shield {
            engine: Engine::new_with_list_text(rules),
            rules: 1,
        }
    }

    /// How many lists (and the custom rules, if any) went into this shield.
    pub fn sources(&self) -> usize {
        self.rules
    }

    /// Decide one request.
    pub fn check(&self, req: &Request<'_>) -> Verdict {
        // Top-level pages the person navigated to are theirs to open; the blocking lists
        // are for what a page pulls in. (A known-bad site is a different question, for
        // `ubo-badware`, answered by a warning page in the application: `check_document`.)
        if req.kind == Kind::Document {
            return Verdict::Allow;
        }
        self.decide(req)
    }

    /// For a top-level page: whether a list marks the whole site as harmful (`$document`
    /// rules, as in uBlock Origin's badware list). The application shows a warning page
    /// with a way through, rather than silently refusing.
    pub fn check_document(&self, url: &str) -> Verdict {
        self.decide(&Request {
            url,
            source: url,
            kind: Kind::Document,
        })
    }

    fn decide(&self, req: &Request<'_>) -> Verdict {
        let Ok(r) = AbRequest::new(req.url, req.source, req.kind.adblock_name(), "get") else {
            // A request adblock cannot parse (no host: `data:`, `blob:`) is never a
            // tracker on the network.
            return Verdict::Allow;
        };
        let result = self.engine.check_network_request(&r);
        if result.should_block() {
            if let Some(body) = result.redirect {
                return Verdict::Redirect { data_url: body };
            }
        }
        if result.should_block() {
            return Verdict::Block {
                rule: result.filter.map(|f| format!("{f:?}")),
            };
        }
        Verdict::Allow
    }

    /// What to hide on the page at `url`.
    pub fn cosmetic(&self, url: &str) -> Cosmetic {
        let res = self.engine.url_cosmetic_resources(url);
        let mut selectors: Vec<&String> = res.hide_selectors.iter().collect();
        selectors.sort();
        let mut css = String::new();
        for s in selectors {
            // A selector carrying `{` or `}` would break out of its rule.
            if s.contains('{') || s.contains('}') {
                continue;
            }
            let _ = writeln!(css, "{s} {{ display: none !important; }}");
        }
        Cosmetic {
            css,
            script: res.injected_script,
            generichide: res.generichide,
            exceptions: res.exceptions.into_iter().collect(),
        }
    }

    /// The second step of hiding, after the page has loaded: generic rules written as a bare
    /// class or id (`##.cookie-banner`) apply to every site, so the engine answers them only
    /// for the classes and ids a page actually uses — the way uBlock Origin does it.
    ///
    /// The application collects them from the page ([`COLLECT_CLASSES_AND_IDS`]) and injects
    /// the returned stylesheet. Not called when [`Cosmetic::generichide`] is set.
    pub fn generic_css(&self, classes: &[String], ids: &[String], cosmetic: &Cosmetic) -> String {
        let selectors = self.engine.hidden_class_id_selectors(
            classes,
            ids,
            &cosmetic.exceptions.iter().cloned().collect(),
        );
        let mut selectors: Vec<String> = selectors
            .into_iter()
            .filter(|s| !s.contains('{') && !s.contains('}'))
            .collect();
        selectors.sort();
        let mut css = String::new();
        for s in selectors {
            let _ = writeln!(css, "{s} {{ display: none !important; }}");
        }
        css
    }
}

/// A script the application evaluates in a loaded page: it returns
/// `{ "classes": [...], "ids": [...] }` for [`Shield::generic_css`]. It reads names only,
/// and caps them, so a page with a hundred thousand nodes does not stall.
pub const COLLECT_CLASSES_AND_IDS: &str = r"(() => {
  const classes = new Set(), ids = new Set(), cap = 5000;
  for (const el of document.querySelectorAll('[class],[id]')) {
    if (el.id && ids.size < cap) ids.add(el.id);
    if (el.classList) for (const c of el.classList) { if (classes.size < cap) classes.add(c); }
    if (classes.size >= cap && ids.size >= cap) break;
  }
  return { classes: [...classes], ids: [...ids] };
})()";

#[cfg(test)]
mod tests {
    use super::*;

    const RULES: &str = "\
||tracker.example^
||ads.example^$third-party
/banner/*$image
@@||tracker.example/allowed.js
example.com##.ad-slot
##.cookie-banner
||badsite.example^$document
";

    fn req<'a>(url: &'a str, source: &'a str, kind: Kind) -> Request<'a> {
        Request { url, source, kind }
    }

    #[test]
    fn network_rules() {
        let s = Shield::from_rules(RULES);
        let page = "https://news.example.org/story";
        assert!(matches!(
            s.check(&req("https://tracker.example/t.js", page, Kind::Script)),
            Verdict::Block { .. }
        ));
        assert_eq!(
            s.check(&req(
                "https://tracker.example/allowed.js",
                page,
                Kind::Script
            )),
            Verdict::Allow
        );
        assert!(matches!(
            s.check(&req("https://ads.example/x.png", page, Kind::Image)),
            Verdict::Block { .. }
        ));
        // `$third-party`: on its own site it is first-party and allowed.
        assert_eq!(
            s.check(&req(
                "https://ads.example/x.png",
                "https://ads.example/",
                Kind::Image
            )),
            Verdict::Allow
        );
        assert!(matches!(
            s.check(&req("https://cdn.other/banner/1.jpg", page, Kind::Image)),
            Verdict::Block { .. }
        ));
        assert_eq!(
            s.check(&req("https://cdn.other/banner/1.jpg", page, Kind::Script)),
            Verdict::Allow
        );
        assert_eq!(
            s.check(&req("https://news.example.org/app.js", page, Kind::Script)),
            Verdict::Allow
        );
        assert_eq!(
            s.check(&req("data:image/png;base64,AA", page, Kind::Image)),
            Verdict::Allow
        );
    }

    #[test]
    fn documents_are_warned_not_silently_blocked() {
        let s = Shield::from_rules(RULES);
        assert_eq!(
            s.check(&req(
                "https://badsite.example/",
                "https://badsite.example/",
                Kind::Document
            )),
            Verdict::Allow
        );
        assert!(matches!(
            s.check_document("https://badsite.example/"),
            Verdict::Block { .. }
        ));
        assert_eq!(s.check_document("https://example.org/"), Verdict::Allow);
    }

    #[test]
    fn cosmetic_rules() {
        let s = Shield::from_rules(RULES);
        let c = s.cosmetic("https://example.com/page");
        assert!(
            c.css.contains(".ad-slot { display: none !important; }"),
            "{}",
            c.css
        );
        let other = s.cosmetic("https://other.org/");
        assert!(!other.css.contains(".ad-slot"));

        // Generic class rules come in the second step, for classes the page has.
        let classes = vec!["cookie-banner".to_string(), "article".to_string()];
        let css = s.generic_css(&classes, &[], &other);
        assert_eq!(css, ".cookie-banner { display: none !important; }\n");
        assert!(s
            .generic_css(&["article".to_string()], &[], &other)
            .is_empty());
    }

    #[test]
    fn destinations_map_to_kinds() {
        assert_eq!(Kind::from_destination("iframe"), Kind::Subdocument);
        assert_eq!(Kind::from_destination("empty"), Kind::Xhr);
        assert_eq!(Kind::from_destination("video"), Kind::Media);
        assert_eq!(Kind::from_destination("nonsense"), Kind::Other);
    }
}
