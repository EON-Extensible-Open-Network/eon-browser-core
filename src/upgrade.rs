// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 EON contributors

//! HTTPS-only (madde 50e). Every `http://` address is loaded as `https://`; if the secure
//! version fails, the application shows a page that lets the person continue on HTTP *for
//! that site*, which is recorded in [`crate::profile::SiteSettings::http_allowed`].
//!
//! Exempt, because HTTPS cannot be expected there: addresses on this computer or this
//! network (a router's page, a development server) and `.onion` (Tor already encrypts).

use url::Url;

/// What [`https_only`] decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Upgrade {
    /// Already HTTPS, or not a web address at all.
    Already,
    /// Load this instead.
    Upgraded(Url),
    /// Left on HTTP, for this reason.
    Exempt(Exemption),
}

/// Why an `http://` address was not upgraded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exemption {
    /// On this computer or this network.
    Local,
    /// A Tor onion service.
    Onion,
    /// The person chose to continue on HTTP for this site.
    PersonAllowed,
}

/// Decide the HTTPS upgrade for `url`.
pub fn https_only(url: &Url) -> Upgrade {
    if url.scheme() != "http" {
        return Upgrade::Already;
    }
    if crate::site::is_local(url) {
        return Upgrade::Exempt(Exemption::Local);
    }
    if url
        .host_str()
        .is_some_and(|h| h.trim_end_matches('.').rsplit('.').next() == Some("onion"))
    {
        return Upgrade::Exempt(Exemption::Onion);
    }
    let mut secure = url.clone();
    // Port 80 is HTTP's; on HTTPS it would be wrong. Any other explicit port is kept, as
    // Firefox's HTTPS-Only mode does.
    let port = url.port();
    if secure.set_scheme("https").is_err() {
        return Upgrade::Already;
    }
    if port == Some(80) && secure.set_port(None).is_err() {
        return Upgrade::Already;
    }
    Upgrade::Upgraded(secure)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> Url {
        Url::parse(s).unwrap_or_else(|e| unreachable!("{s}: {e}"))
    }

    #[test]
    fn upgrades_the_web() {
        assert_eq!(
            https_only(&u("http://example.com/a?b#c")),
            Upgrade::Upgraded(u("https://example.com/a?b#c"))
        );
        assert_eq!(
            https_only(&u("http://example.com:80/")),
            Upgrade::Upgraded(u("https://example.com/"))
        );
        assert_eq!(
            https_only(&u("http://example.com:8080/")),
            Upgrade::Upgraded(u("https://example.com:8080/"))
        );
        assert_eq!(
            https_only(&u("http://8.8.8.8/")),
            Upgrade::Upgraded(u("https://8.8.8.8/"))
        );
    }

    #[test]
    fn leaves_what_cannot_be_upgraded() {
        assert_eq!(https_only(&u("https://example.com/")), Upgrade::Already);
        assert_eq!(https_only(&u("file:///C:/x.html")), Upgrade::Already);
        assert_eq!(
            https_only(&u("http://192.168.1.1/")),
            Upgrade::Exempt(Exemption::Local)
        );
        assert_eq!(
            https_only(&u("http://localhost:5173/")),
            Upgrade::Exempt(Exemption::Local)
        );
        assert_eq!(
            https_only(&u(
                "http://duckduckgogg42xjoc72x3sjasowoarfbgcmvfimaftt6twagswzczad.onion/"
            )),
            Upgrade::Exempt(Exemption::Onion)
        );
    }
}
