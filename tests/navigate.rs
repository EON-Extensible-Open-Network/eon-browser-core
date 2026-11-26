// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 EON contributors

//! A navigation goes through cleaning, then HTTPS, in that order.

use eon_browser_core::upgrade::{Exemption, Upgrade};
use eon_browser_core::{navigate, Navigation};
use url::Url;

fn u(s: &str) -> Url {
    Url::parse(s).unwrap_or_else(|e| unreachable!("{s}: {e}"))
}

#[test]
fn a_tracked_wrapped_plain_http_link_ends_clean_and_secure() {
    let n: Navigation = navigate(
        &u("https://www.google.com/url?q=http%3A%2F%2Fexample.com%2Fread%3Fid%3D3%26utm_source%3Dg&sa=U"),
        false,
    );
    assert_eq!(n.url.as_str(), "https://example.com/read?id=3");
    assert!(n.changed);
    let cleaned = n.cleaned.unwrap_or_else(|| unreachable!());
    assert_eq!(cleaned.unwrapped_from.as_deref(), Some("www.google.com"));
    assert_eq!(cleaned.removed, vec!["utm_source"]);
    assert!(matches!(n.upgrade, Upgrade::Upgraded(_)));
}

#[test]
fn nothing_to_do() {
    let n = navigate(&u("https://servo.org/"), false);
    assert!(!n.changed);
    assert_eq!(n.cleaned, None);
    assert_eq!(n.upgrade, Upgrade::Already);
}

#[test]
fn the_persons_http_exception_is_kept() {
    let n = navigate(&u("http://old-site.example/?fbclid=1"), true);
    assert_eq!(n.url.as_str(), "http://old-site.example/");
    assert_eq!(n.upgrade, Upgrade::Exempt(Exemption::PersonAllowed));
}
