// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 EON contributors

//! Which *site* an address belongs to: the registrable domain ("eTLD+1") from the public
//! suffix list. `news.bbc.co.uk` and `www.bbc.co.uk` are the site `bbc.co.uk`; settings a
//! person makes for one apply to both, and a tracker on `bbc.co.uk` is first-party there.

use std::net::IpAddr;

use url::{Host, Url};

/// The site of `url`, or `None` for addresses without a host (`file:`, `data:`, `about:`).
///
/// IP addresses and single-label hosts (`localhost`, `router`) are their own site.
pub fn of(url: &Url) -> Option<String> {
    match url.host()? {
        Host::Ipv4(ip) => Some(ip.to_string()),
        Host::Ipv6(ip) => Some(format!("[{ip}]")),
        Host::Domain(d) => Some(of_host(d)),
    }
}

/// The site of a bare host name.
pub fn of_host(host: &str) -> String {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    match psl::domain(host.as_bytes()) {
        Some(d) => String::from_utf8_lossy(d.as_bytes()).into_owned(),
        None => host,
    }
}

/// Whether `host` ends in a suffix on the public suffix list that is really in use
/// (`.com`, `.com.tr`, `.dev`), not one the list only infers by its wildcard rule.
pub fn has_known_suffix(host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if !host.contains('.') {
        return false;
    }
    psl::suffix(host.as_bytes()).is_some_and(|s| s.is_known())
}

/// Addresses that never leave this computer or this network: loopback, private ranges,
/// link-local, `localhost` and the names reserved for local networks.
pub fn is_local(url: &Url) -> bool {
    match url.host() {
        Some(Host::Ipv4(ip)) => is_local_ip(IpAddr::V4(ip)),
        Some(Host::Ipv6(ip)) => is_local_ip(IpAddr::V6(ip)),
        Some(Host::Domain(d)) => is_local_name(d),
        None => false,
    }
}

fn is_local_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback() || v4.is_private() || v4.is_link_local() || v4.is_unspecified()
                // 100.64.0.0/10, carrier-grade NAT and tailnets
                || (v4.octets()[0] == 100 && (v4.octets()[1] & 0xC0) == 64)
        }
        IpAddr::V6(v6) => {
            let seg = v6.segments()[0];
            v6.is_loopback()
                || v6.is_unspecified()
                || (seg & 0xfe00) == 0xfc00 // unique local
                || (seg & 0xffc0) == 0xfe80 // link-local
        }
    }
}

fn is_local_name(host: &str) -> bool {
    let h = host.trim_end_matches('.').to_ascii_lowercase();
    if !h.contains('.') {
        // `localhost`, `router`, `nas`: single labels resolve only on the local network.
        return true;
    }
    [
        ".localhost",
        ".local",
        ".lan",
        ".home.arpa",
        ".internal",
        ".test",
    ]
    .iter()
    .any(|s| h.ends_with(s))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> Url {
        Url::parse(s).unwrap_or_else(|e| unreachable!("{s}: {e}"))
    }

    #[test]
    fn registrable_domain() {
        assert_eq!(
            of(&u("https://news.bbc.co.uk/x")).as_deref(),
            Some("bbc.co.uk")
        );
        assert_eq!(
            of(&u("https://www.example.com")).as_deref(),
            Some("example.com")
        );
        assert_eq!(
            of(&u("https://a.b.github.io")).as_deref(),
            Some("b.github.io")
        );
        assert_eq!(of(&u("https://www.gov.tr")).as_deref(), Some("www.gov.tr"));
        assert_eq!(
            of(&u("http://192.168.1.1/")).as_deref(),
            Some("192.168.1.1")
        );
        assert_eq!(
            of(&u("http://localhost:8080/")).as_deref(),
            Some("localhost")
        );
        assert_eq!(of(&u("about:blank")), None);
    }

    #[test]
    fn known_suffixes() {
        assert!(has_known_suffix("wikipedia.org"));
        assert!(has_known_suffix("sahibinden.com.tr"));
        assert!(!has_known_suffix("notes.txt"));
        assert!(!has_known_suffix("localhost"));
    }

    #[test]
    fn local_addresses() {
        for s in [
            "http://localhost/",
            "http://127.0.0.1:3000/",
            "http://10.0.0.5/",
            "http://192.168.1.1/",
            "http://100.101.2.3/",
            "http://[::1]/",
            "http://[fd00::1]/",
            "http://printer.local/",
            "http://router/",
            "http://box.home.arpa/",
        ] {
            assert!(is_local(&u(s)), "{s}");
        }
        for s in [
            "http://example.com/",
            "http://8.8.8.8/",
            "http://[2606:4700::1111]/",
        ] {
            assert!(!is_local(&u(s)), "{s}");
        }
    }
}
