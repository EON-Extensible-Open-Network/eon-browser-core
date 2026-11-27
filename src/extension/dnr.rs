// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 EON contributors

//! `declarativeNetRequest` rules (the blocking format of Manifest V3 extensions: uBlock
//! Origin Lite, AdGuard, Ghostery…) translated into Adblock Plus lines, so they run in the
//! same engine as EasyList — with no Manifest V3 ceiling on how many.
//!
//! The two formats are close by design: `urlFilter` *is* Adblock Plus pattern syntax
//! (`||`, `|`, `^`, `*`). What does not translate is reported, never guessed.

use serde::Deserialize;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Rule {
    pub id: i64,
    pub priority: Option<i64>,
    pub action: RuleAction,
    pub condition: Condition,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RuleAction {
    #[serde(rename = "type")]
    pub kind: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Condition {
    pub url_filter: Option<String>,
    pub regex_filter: Option<String>,
    pub is_url_filter_case_sensitive: Option<bool>,
    pub initiator_domains: Vec<String>,
    pub excluded_initiator_domains: Vec<String>,
    /// The older names of the two above.
    pub domains: Vec<String>,
    pub excluded_domains: Vec<String>,
    pub request_domains: Vec<String>,
    pub excluded_request_domains: Vec<String>,
    pub resource_types: Vec<String>,
    pub excluded_resource_types: Vec<String>,
    /// `firstParty` or `thirdParty`.
    pub domain_type: Option<String>,
}

/// The result of translating a rule file.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Translation {
    /// Adblock Plus lines, ready for the engine.
    pub lines: Vec<String>,
    /// Rules not translated, with the reason: `(rule id, reason)`.
    pub skipped: Vec<(i64, String)>,
    /// Rules that need no line here because the browser already does it (upgradeScheme:
    /// EON Browser is HTTPS-only).
    pub already_done: usize,
}

/// Translate a rule file (a JSON array of rules).
pub fn translate(json: &str) -> Result<Translation, String> {
    let rules: Vec<Rule> = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let mut t = Translation::default();
    for rule in &rules {
        match translate_rule(rule) {
            Ok(Some(lines)) => t.lines.extend(lines),
            Ok(None) => t.already_done += 1,
            Err(reason) => t.skipped.push((rule.id, reason)),
        }
    }
    Ok(t)
}

fn translate_rule(rule: &Rule) -> Result<Option<Vec<String>>, String> {
    let prefix = match rule.action.kind.as_str() {
        "block" => "",
        "allow" | "allowAllRequests" => "@@",
        "upgradeScheme" => return Ok(None),
        other => return Err(format!("action `{other}` is not translated yet")),
    };
    let c = &rule.condition;
    let mut options: Vec<String> = Vec::new();

    // Resource types, always as a positive list. DNR's default is "every type except the
    // main frame", while an untyped Adblock Plus line also blocks top-level pages; and
    // `adblock` reads a lone negated type (`$~document`) as matching nothing. Listing the
    // types keeps DNR's meaning exactly.
    let mut types: Vec<&'static str> = if c.resource_types.is_empty() {
        if rule.action.kind == "allowAllRequests" {
            vec!["document", "subdocument"]
        } else {
            ALL_BUT_DOCUMENT.to_vec()
        }
    } else {
        c.resource_types
            .iter()
            .map(|t| resource_type(t))
            .collect::<Result<_, _>>()?
    };
    for t in &c.excluded_resource_types {
        let ex = resource_type(t)?;
        types.retain(|x| *x != ex);
    }
    types.dedup();
    if types.is_empty() {
        return Err("every resource type is excluded".into());
    }
    options.push(types.join(","));

    let mut domains: Vec<String> = c
        .initiator_domains
        .iter()
        .chain(&c.domains)
        .map(|d| d.to_ascii_lowercase())
        .collect();
    domains.extend(
        c.excluded_initiator_domains
            .iter()
            .chain(&c.excluded_domains)
            .map(|d| format!("~{}", d.to_ascii_lowercase())),
    );
    if !domains.is_empty() {
        options.push(format!("domain={}", domains.join("|")));
    }
    match c.domain_type.as_deref() {
        Some("thirdParty") => options.push("third-party".into()),
        Some("firstParty") => options.push("~third-party".into()),
        Some(other) => return Err(format!("domainType `{other}`")),
        None => {}
    }
    if c.is_url_filter_case_sensitive == Some(true) {
        options.push("match-case".into());
    }
    if !c.excluded_request_domains.is_empty() {
        return Err("excludedRequestDomains has no Adblock Plus equivalent on one line".into());
    }

    // The pattern part: urlFilter, a regular expression, or one line per request domain.
    let patterns: Vec<String> = match (&c.url_filter, &c.regex_filter, c.request_domains.is_empty())
    {
        (Some(f), None, true) => vec![plain(f)],
        (None, Some(r), true) => vec![format!("/{}/", escape_slashes(r))],
        (None, None, false) => c
            .request_domains
            .iter()
            .map(|d| format!("||{}^", d.to_ascii_lowercase()))
            .collect(),
        (Some(f), None, false) if f.starts_with('/') || f.starts_with('*') => c
            .request_domains
            .iter()
            .map(|d| format!("||{}^{}", d.to_ascii_lowercase(), f.trim_start_matches('*')))
            .collect(),
        (None, None, true) => vec!["*".into()],
        _ => return Err("this combination of urlFilter, regexFilter and requestDomains".into()),
    };
    let opts = if options.is_empty() {
        String::new()
    } else {
        format!("${}", options.join(","))
    };
    Ok(Some(
        patterns
            .into_iter()
            .map(|p| format!("{prefix}{p}{opts}"))
            .collect(),
    ))
}

/// A `urlFilter` as an Adblock Plus pattern. They share the syntax, with one trap: a
/// pattern that starts and ends with `/` is a regular expression to Adblock Plus but a
/// plain substring to DNR, so `/banner/` becomes `/banner/*` (the same match, as a pattern).
fn plain(f: &str) -> String {
    if f.len() > 1 && f.starts_with('/') && f.ends_with('/') {
        format!("{f}*")
    } else {
        f.to_string()
    }
}

/// A DNR `regexFilter` inside Adblock Plus's `/…/`: every unescaped `/` escaped.
fn escape_slashes(r: &str) -> String {
    let mut out = String::with_capacity(r.len() + 8);
    let mut escaped = false;
    for c in r.chars() {
        if c == '/' && !escaped {
            out.push('\\');
        }
        escaped = c == '\\' && !escaped;
        out.push(c);
    }
    out
}

/// Every type a sub-resource can have: DNR's default set (all but `main_frame`).
const ALL_BUT_DOCUMENT: [&str; 11] = [
    "subdocument",
    "stylesheet",
    "script",
    "image",
    "font",
    "object",
    "xmlhttprequest",
    "ping",
    "media",
    "websocket",
    "other",
];

fn resource_type(t: &str) -> Result<&'static str, String> {
    Ok(match t {
        "main_frame" => "document",
        "sub_frame" => "subdocument",
        "stylesheet" => "stylesheet",
        "script" => "script",
        "image" => "image",
        "font" => "font",
        "object" => "object",
        "xmlhttprequest" => "xmlhttprequest",
        "ping" => "ping",
        "media" => "media",
        "websocket" => "websocket",
        "csp_report" | "webtransport" | "webbundle" | "other" => "other",
        other => return Err(format!("resource type `{other}`")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shield::{Kind, Request, Shield, Verdict};

    const RULES: &str = r#"[
      { "id": 1, "priority": 1, "action": { "type": "block" },
        "condition": { "urlFilter": "||ads.example^", "resourceTypes": ["script", "image"] } },
      { "id": 2, "action": { "type": "allow" },
        "condition": { "urlFilter": "||ads.example/ok.js", "resourceTypes": ["script"] } },
      { "id": 3, "action": { "type": "block" },
        "condition": { "requestDomains": ["tracker.example", "pixel.example"], "domainType": "thirdParty" } },
      { "id": 4, "action": { "type": "block" },
        "condition": { "urlFilter": "/banner/", "initiatorDomains": ["news.example"], "excludedResourceTypes": ["main_frame"] } },
      { "id": 5, "action": { "type": "upgradeScheme" }, "condition": { "requestDomains": ["x.example"] } },
      { "id": 6, "action": { "type": "redirect", "redirect": { "url": "https://x" } },
        "condition": { "urlFilter": "||y.example^" } },
      { "id": 7, "action": { "type": "block" }, "condition": { "regexFilter": "^https://cdn\\.example/ad[0-9]+\\.js" } }
    ]"#;

    #[test]
    fn rules_become_adblock_lines() {
        const ALL: &str =
            "subdocument,stylesheet,script,image,font,object,xmlhttprequest,ping,media,websocket,other";
        let t = translate(RULES).unwrap_or_default();
        assert_eq!(
            t.lines,
            vec![
                "||ads.example^$script,image",
                "@@||ads.example/ok.js$script",
                format!("||tracker.example^${ALL},third-party").as_str(),
                format!("||pixel.example^${ALL},third-party").as_str(),
                format!("/banner/*${ALL},domain=news.example").as_str(),
                format!("/^https:\\/\\/cdn\\.example\\/ad[0-9]+\\.js/${ALL}").as_str(),
            ]
        );
        assert_eq!(
            t.already_done, 1,
            "upgradeScheme: the browser is HTTPS-only"
        );
        assert_eq!(t.skipped.len(), 1);
        assert_eq!(t.skipped[0].0, 6);
    }

    #[test]
    fn translated_rules_block_in_the_engine() {
        let t = translate(RULES).unwrap_or_default();
        let shield = Shield::from_rules(&t.lines.join("\n"));
        let page = "https://news.example/story";
        let check = |url: &str, kind| {
            shield.check(&Request {
                url,
                source: page,
                kind,
            })
        };
        assert!(matches!(
            check("https://ads.example/a.js", Kind::Script),
            Verdict::Block { .. }
        ));
        assert_eq!(
            check("https://ads.example/ok.js", Kind::Script),
            Verdict::Allow
        );
        assert_eq!(
            check("https://ads.example/a.css", Kind::Stylesheet),
            Verdict::Allow
        );
        assert!(matches!(
            check("https://pixel.example/p.gif", Kind::Image),
            Verdict::Block { .. }
        ));
        assert!(matches!(
            check("https://cdn.other/banner/1.png", Kind::Image),
            Verdict::Block { .. }
        ));
        assert!(matches!(
            check("https://cdn.example/ad12.js", Kind::Script),
            Verdict::Block { .. }
        ));
        assert_eq!(
            check("https://cdn.example/app.js", Kind::Script),
            Verdict::Allow
        );
    }

    #[test]
    fn not_json_is_an_error() {
        assert!(translate("{").is_err());
    }
}
