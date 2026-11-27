// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 EON contributors

//! `manifest.json`, as Chrome (MV2 and MV3) and Firefox write it, and what it asks for in
//! words a person can judge before installing (plan madde 50h).

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::Value;

use super::pattern::Pattern;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Manifest {
    pub manifest_version: u8,
    pub name: String,
    pub version: String,
    pub description: String,
    pub default_locale: Option<String>,
    /// API permissions, and in MV2 also host patterns.
    pub permissions: Vec<Value>,
    /// MV3 host patterns.
    pub host_permissions: Vec<String>,
    pub optional_permissions: Vec<Value>,
    pub content_scripts: Vec<ContentScript>,
    pub background: Option<Background>,
    /// MV3 `action`; MV2 `browser_action` and `page_action` are read into it.
    pub action: Option<Action>,
    pub browser_action: Option<Action>,
    pub page_action: Option<Action>,
    pub options_page: Option<String>,
    pub options_ui: Option<OptionsUi>,
    pub declarative_net_request: Option<Dnr>,
    pub icons: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ContentScript {
    pub matches: Vec<String>,
    pub exclude_matches: Vec<String>,
    pub js: Vec<String>,
    pub css: Vec<String>,
    /// `document_start`, `document_end` or `document_idle` (the default).
    pub run_at: Option<String>,
    pub all_frames: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Background {
    pub service_worker: Option<String>,
    pub scripts: Vec<String>,
    pub page: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Action {
    pub default_popup: Option<String>,
    pub default_title: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct OptionsUi {
    pub page: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Dnr {
    pub rule_resources: Vec<RuleResource>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct RuleResource {
    pub id: String,
    pub enabled: bool,
    pub path: String,
}

impl Manifest {
    pub fn parse(json: &str) -> Result<Manifest, String> {
        // Chrome tolerates a BOM and comments are not allowed; a BOM is all we strip.
        let json = json.trim_start_matches('\u{feff}');
        let mut m: Manifest = serde_json::from_str(json).map_err(|e| e.to_string())?;
        if !matches!(m.manifest_version, 2 | 3) {
            return Err(format!(
                "manifest_version {} is not 2 or 3",
                m.manifest_version
            ));
        }
        if m.name.is_empty() || m.version.is_empty() {
            return Err("a manifest needs a name and a version".into());
        }
        if m.action.is_none() {
            m.action = m.browser_action.take().or_else(|| m.page_action.take());
        }
        Ok(m)
    }

    /// Replace `__MSG_key__` in the name and description from `_locales/<lang>/messages.json`.
    pub fn localize(&mut self, messages: &str) {
        let Ok(map) = serde_json::from_str::<BTreeMap<String, Value>>(messages) else {
            return;
        };
        let lookup = |s: &str| -> Option<String> {
            let key = s.strip_prefix("__MSG_")?.strip_suffix("__")?;
            map.iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(key))
                .and_then(|(_, v)| v.get("message")?.as_str().map(str::to_string))
        };
        if let Some(n) = lookup(&self.name) {
            self.name = n;
        }
        if let Some(d) = lookup(&self.description) {
            self.description = d;
        }
    }

    /// API permissions (strings that are not host patterns).
    pub fn api_permissions(&self) -> Vec<String> {
        self.permissions
            .iter()
            .filter_map(Value::as_str)
            .filter(|p| Pattern::parse(p).is_none())
            .map(str::to_string)
            .collect()
    }

    /// Host patterns, from `host_permissions`, MV2 `permissions` and content scripts.
    pub fn host_patterns(&self) -> Vec<Pattern> {
        let mut out: Vec<Pattern> = self
            .host_permissions
            .iter()
            .chain(
                self.permissions
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect::<Vec<_>>()
                    .iter(),
            )
            .filter_map(|p| Pattern::parse(p))
            .collect();
        for cs in &self.content_scripts {
            out.extend(cs.matches.iter().filter_map(|p| Pattern::parse(p)));
        }
        out
    }

    /// What the extension may do, in sentences, most consequential first. This is shown
    /// before anything is installed.
    pub fn permission_summary(&self) -> Vec<String> {
        let mut out = Vec::new();
        let hosts = self.host_patterns();
        if hosts.iter().any(Pattern::is_every_site) {
            out.push("Read and change everything on every website you visit".to_string());
        } else {
            let mut names: Vec<String> = hosts
                .iter()
                .filter_map(|p| match p {
                    Pattern::Parts { host, .. } => Some(host.trim_start_matches("*.").to_string()),
                    Pattern::All => None,
                })
                .collect();
            names.sort();
            names.dedup();
            if !names.is_empty() {
                out.push(format!(
                    "Read and change what you see on {}",
                    names.join(", ")
                ));
            }
        }
        let api = self.api_permissions();
        let has = |p: &str| api.iter().any(|a| a == p);
        if has("webRequest") || has("webRequestBlocking") {
            out.push("See every request the browser makes, and block or change it".into());
        }
        if has("declarativeNetRequest") || has("declarativeNetRequestWithHostAccess") {
            out.push("Block or change content on pages, by its own rules".into());
        }
        if has("history") {
            out.push("Read and change your browsing history".into());
        }
        if has("tabs") {
            out.push("See the addresses and titles of your open tabs".into());
        }
        if has("cookies") {
            out.push("Read and change cookies on the sites it can reach".into());
        }
        if has("downloads") {
            out.push("Manage your downloads".into());
        }
        if has("clipboardRead") {
            out.push("Read what you copy".into());
        }
        if has("nativeMessaging") {
            out.push("Talk to programs installed on your computer".into());
        }
        if has("management") {
            out.push("Manage your other extensions".into());
        }
        if has("storage") || has("unlimitedStorage") {
            out.push("Store its own data on this computer".into());
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MV3: &str = r#"{
      "manifest_version": 3, "name": "__MSG_extName__", "version": "1.2.0",
      "description": "__MSG_extDesc__", "default_locale": "en",
      "permissions": ["storage", "declarativeNetRequest", "tabs"],
      "host_permissions": ["*://*.youtube.com/*"],
      "background": { "service_worker": "bg.js" },
      "action": { "default_popup": "popup.html" },
      "content_scripts": [{ "matches": ["https://www.youtube.com/*"], "js": ["c.js"], "css": ["c.css"], "run_at": "document_start" }],
      "declarative_net_request": { "rule_resources": [{ "id": "r1", "enabled": true, "path": "rules.json" }] }
    }"#;

    #[test]
    fn mv3_and_its_permissions_in_words() {
        let mut m = Manifest::parse(MV3).unwrap_or_default();
        m.localize(
            r#"{"extName":{"message":"Tube Helper"},"extDesc":{"message":"Makes it nicer"}}"#,
        );
        assert_eq!(m.name, "Tube Helper");
        assert_eq!(m.description, "Makes it nicer");
        assert_eq!(
            m.action.as_ref().and_then(|a| a.default_popup.as_deref()),
            Some("popup.html")
        );
        let words = m.permission_summary();
        assert_eq!(
            words[0],
            "Read and change what you see on www.youtube.com, youtube.com"
        );
        assert!(words
            .iter()
            .any(|w| w.starts_with("Block or change content")));
        assert!(words.iter().any(|w| w.starts_with("See the addresses")));
    }

    #[test]
    fn mv2_hosts_in_permissions_and_browser_action() {
        let m = Manifest::parse(
            r#"{"manifest_version":2,"name":"Old","version":"1","permissions":["<all_urls>","webRequest","webRequestBlocking"],"browser_action":{"default_title":"Old"}}"#,
        )
        .unwrap_or_default();
        assert_eq!(
            m.api_permissions(),
            vec!["webRequest", "webRequestBlocking"]
        );
        let words = m.permission_summary();
        assert_eq!(
            words[0],
            "Read and change everything on every website you visit"
        );
        assert!(m.action.is_some());
    }

    #[test]
    fn refuses_what_is_not_a_manifest() {
        assert!(Manifest::parse(r#"{"manifest_version":4,"name":"x","version":"1"}"#).is_err());
        assert!(Manifest::parse(r#"{"manifest_version":3,"version":"1"}"#).is_err());
        assert!(Manifest::parse("not json").is_err());
    }
}
