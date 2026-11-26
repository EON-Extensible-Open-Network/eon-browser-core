// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 EON contributors

//! Search engines. **None is the default until the person chooses** (madde 50e): there is
//! no deal behind the list, and its order is alphabetical. Suggestions while typing are off
//! unless the person turns them on, because every keystroke would go to the engine.

use serde::{Deserialize, Serialize};
use url::Url;

/// A search engine a person can choose.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchEngine {
    /// Stable identifier, stored in settings.
    pub id: String,
    /// The name shown.
    pub name: String,
    /// Typed before a query to use this engine once: `@ddg rust servo`.
    pub keyword: String,
    /// The results address, with `{q}` where the query goes.
    pub search: String,
    /// The suggestions address (OpenSearch JSON), used only if the person turned
    /// suggestions on.
    pub suggest: Option<String>,
    /// One sentence a person can choose by: who runs it and whose index it uses.
    pub about: String,
}

impl SearchEngine {
    /// The results address for `query`.
    pub fn results_url(&self, query: &str) -> Option<Url> {
        fill(&self.search, query)
    }

    /// The suggestions address for `query`, if this engine has one.
    pub fn suggest_url(&self, query: &str) -> Option<Url> {
        self.suggest.as_deref().and_then(|t| fill(t, query))
    }
}

fn fill(template: &str, query: &str) -> Option<Url> {
    let q: String = url::form_urlencoded::byte_serialize(query.as_bytes()).collect();
    Url::parse(&template.replace("{q}", &q)).ok()
}

fn engine(
    id: &str,
    name: &str,
    keyword: &str,
    search: &str,
    suggest: Option<&str>,
    about: &str,
) -> SearchEngine {
    SearchEngine {
        id: id.into(),
        name: name.into(),
        keyword: keyword.into(),
        search: search.into(),
        suggest: suggest.map(Into::into),
        about: about.into(),
    }
}

/// The engines offered at first start, alphabetically.
pub fn catalogue() -> Vec<SearchEngine> {
    vec![
        engine(
            "bing",
            "Bing",
            "@bing",
            "https://www.bing.com/search?q={q}",
            Some("https://www.bing.com/osjson.aspx?query={q}"),
            "Microsoft's own index.",
        ),
        engine(
            "brave",
            "Brave Search",
            "@brave",
            "https://search.brave.com/search?q={q}",
            Some("https://search.brave.com/api/suggest?q={q}"),
            "Brave's own independent index; no profile of the person.",
        ),
        engine(
            "duckduckgo",
            "DuckDuckGo",
            "@ddg",
            "https://duckduckgo.com/?q={q}",
            Some("https://duckduckgo.com/ac/?q={q}&type=list"),
            "Results mostly from Bing, without tracking.",
        ),
        engine(
            "ecosia",
            "Ecosia",
            "@ecosia",
            "https://www.ecosia.org/search?q={q}",
            Some("https://ac.ecosia.org/autocomplete?q={q}&type=list"),
            "Results from Bing and Google; profits fund tree planting.",
        ),
        engine(
            "google",
            "Google",
            "@google",
            "https://www.google.com/search?q={q}",
            Some("https://www.google.com/complete/search?client=firefox&q={q}"),
            "Google's own index; Google keeps a profile when signed in.",
        ),
        engine(
            "mojeek",
            "Mojeek",
            "@mojeek",
            "https://www.mojeek.com/search?q={q}",
            None,
            "Its own independent index, built in the UK; no tracking.",
        ),
        engine(
            "qwant",
            "Qwant",
            "@qwant",
            "https://www.qwant.com/?q={q}",
            Some("https://api.qwant.com/v3/suggest?q={q}&client=opensearch"),
            "A European engine with its own index and Bing results.",
        ),
        engine(
            "startpage",
            "Startpage",
            "@sp",
            "https://www.startpage.com/sp/search?query={q}",
            Some("https://www.startpage.com/osuggestions?q={q}"),
            "Google's results, fetched on the person's behalf.",
        ),
        engine(
            "wikipedia",
            "Wikipedia",
            "@w",
            "https://www.wikipedia.org/search-redirect.php?search={q}",
            None,
            "Searches the encyclopedia directly.",
        ),
    ]
}

/// Find an engine by id.
pub fn by_id<'a>(engines: &'a [SearchEngine], id: &str) -> Option<&'a SearchEngine> {
    engines.iter().find(|e| e.id == id)
}

/// Find an engine by its `@keyword`.
pub fn by_keyword<'a>(engines: &'a [SearchEngine], keyword: &str) -> Option<&'a SearchEngine> {
    engines
        .iter()
        .find(|e| e.keyword.eq_ignore_ascii_case(keyword))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogue_is_alphabetical_and_complete() {
        let c = catalogue();
        let names: Vec<_> = c.iter().map(|e| e.name.to_lowercase()).collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted, "no engine is placed first by a deal");
        for e in &c {
            let u = e
                .results_url("x")
                .unwrap_or_else(|| unreachable!("{}", e.id));
            assert_eq!(u.scheme(), "https", "{}", e.id);
            assert!(e.keyword.starts_with('@'));
        }
    }

    #[test]
    fn queries_are_encoded() {
        let c = catalogue();
        let ddg = by_id(&c, "duckduckgo").unwrap_or_else(|| unreachable!());
        assert_eq!(
            ddg.results_url("çay & simit?")
                .map(|u| u.to_string())
                .as_deref(),
            Some("https://duckduckgo.com/?q=%C3%A7ay+%26+simit%3F")
        );
        assert!(by_keyword(&c, "@W").is_some());
    }
}
