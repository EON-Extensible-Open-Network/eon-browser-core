// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 EON contributors

//! WebExtensions match patterns: `<all_urls>`, `*://*.example.com/*`,
//! `https://example.org/path*`. Chrome's and Firefox's rules, which agree on these.

use url::Url;

/// A parsed match pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pattern {
    /// `<all_urls>`: every http, https, ws, wss, ftp, file and data address.
    All,
    Parts {
        /// `*` (http or https), or one scheme.
        scheme: String,
        /// `*` (any host), `*.example.com` (it and its subdomains) or a host.
        host: String,
        /// The path with `*` wildcards, without the query.
        path: String,
    },
}

impl Pattern {
    /// Parse a pattern; `None` if it is not valid.
    pub fn parse(s: &str) -> Option<Pattern> {
        if s == "<all_urls>" {
            return Some(Pattern::All);
        }
        let (scheme, rest) = s.split_once("://")?;
        if !matches!(
            scheme,
            "*" | "http" | "https" | "ws" | "wss" | "ftp" | "file" | "data"
        ) {
            return None;
        }
        let i = rest.find('/')?;
        let (host, path) = (&rest[..i], &rest[i..]);
        if scheme != "file" && host.is_empty() {
            return None;
        }
        // A wildcard host is `*` alone or `*.` before a name without another `*`.
        let wildcard_ok = host == "*"
            || host
                .strip_prefix("*.")
                .is_some_and(|rest| !rest.contains('*'));
        if host.contains('*') && !wildcard_ok {
            return None;
        }
        Some(Pattern::Parts {
            scheme: scheme.to_ascii_lowercase(),
            host: host.to_ascii_lowercase(),
            path: path.to_string(),
        })
    }

    pub fn matches(&self, url: &Url) -> bool {
        let scheme = url.scheme();
        match self {
            Pattern::All => matches!(
                scheme,
                "http" | "https" | "ws" | "wss" | "ftp" | "file" | "data"
            ),
            Pattern::Parts {
                scheme: s,
                host,
                path,
            } => {
                let scheme_ok = if s == "*" {
                    matches!(scheme, "http" | "https" | "ws" | "wss")
                } else {
                    s == scheme
                };
                if !scheme_ok {
                    return false;
                }
                let h = url.host_str().unwrap_or("").to_ascii_lowercase();
                let host_ok = if host == "*" {
                    true
                } else if let Some(base) = host.strip_prefix("*.") {
                    h == base || h.ends_with(&format!(".{base}"))
                } else {
                    &h == host
                };
                host_ok && glob(path, url.path())
            }
        }
    }

    /// Whether this pattern reaches every site.
    pub fn is_every_site(&self) -> bool {
        match self {
            Pattern::All => true,
            Pattern::Parts { host, .. } => host == "*",
        }
    }
}

/// Does `text` match `pattern`, where `*` stands for any run of characters?
fn glob(pattern: &str, text: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == text;
    }
    let mut rest = text;
    for (i, part) in parts.iter().enumerate() {
        if i == 0 {
            match rest.strip_prefix(part) {
                Some(r) => rest = r,
                None => return false,
            }
        } else if i == parts.len() - 1 {
            return rest.ends_with(part);
        } else {
            match rest.find(part) {
                Some(at) => rest = &rest[at + part.len()..],
                None => return false,
            }
        }
    }
    true
}

/// Whether any of `patterns` matches `url`, and none of `excluded` does.
pub fn any_matches(patterns: &[Pattern], excluded: &[Pattern], url: &Url) -> bool {
    patterns.iter().any(|p| p.matches(url)) && !excluded.iter().any(|p| p.matches(url))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> Url {
        Url::parse(s).unwrap_or_else(|e| unreachable!("{s}: {e}"))
    }

    fn p(s: &str) -> Pattern {
        Pattern::parse(s).unwrap_or_else(|| unreachable!("{s}"))
    }

    #[test]
    fn chrome_examples() {
        assert!(p("<all_urls>").matches(&u("https://example.com/x")));
        assert!(p("*://*/*").matches(&u("http://a.b/c?d")));
        assert!(!p("*://*/*").matches(&u("file:///C:/x")));
        let mail = p("*://mail.google.com/*");
        assert!(mail.matches(&u("https://mail.google.com/mail/u/0")));
        assert!(!mail.matches(&u("https://google.com/")));
        let sub = p("https://*.example.org/*");
        assert!(sub.matches(&u("https://example.org/")));
        assert!(sub.matches(&u("https://a.b.example.org/x")));
        assert!(!sub.matches(&u("https://badexample.org/")));
        assert!(!sub.matches(&u("http://example.org/")));
        let path = p("https://example.com/foo*bar");
        assert!(path.matches(&u("https://example.com/foo123bar")));
        assert!(!path.matches(&u("https://example.com/foo123baz")));
        assert!(p("*://*/*").is_every_site() && !mail.is_every_site());
    }

    #[test]
    fn invalid_patterns() {
        for s in [
            "example.com",
            "https://*foo.com/*",
            "https://a.*.com/*",
            "chrome://x/*",
            "https://example.com",
        ] {
            assert!(Pattern::parse(s).is_none(), "{s}");
        }
    }
}
