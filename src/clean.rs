// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 EON contributors

//! Link cleaning: tracking parameters stripped, redirect wrappers skipped.
//!
//! Firefox does this only in Strict mode and private windows; EON Browser does it always
//! (madde 50e). Two rules keep it honest:
//!
//! - **Only parameters whose single purpose is tracking.** A parameter that might also
//!   carry meaning on some site (`ref`, `source`, `id`) is left alone, because a broken
//!   page is worse than one leaked click.
//! - **Affiliate tags are neither added nor removed.** Inserting them is what Brave was
//!   rightly criticised for in 2020; removing them takes income from the person who
//!   shared the link. They are not tracking the reader across sites.

use url::Url;

/// What [`clean`] changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cleaned {
    /// The address after cleaning.
    pub url: Url,
    /// The parameter names removed, in the order they appeared.
    pub removed: Vec<String>,
    /// The redirect wrapper skipped, if any (its host).
    pub unwrapped_from: Option<String>,
}

/// Parameters removed everywhere. Exact names, compared case-insensitively.
///
/// Each one's owner, so a reader can check it: Google Ads (`gclid`, `gclsrc`, `dclid`,
/// `gbraid`, `wbraid`, `gad_source`, `gad_campaignid`, `srsltid`), Google Analytics's
/// cross-domain linker (`_gl`), Meta (`fbclid`, `igshid`), Microsoft Advertising
/// (`msclkid`), X (`twclid`), TikTok (`ttclid`), LinkedIn (`li_fat_id`), Yandex (`yclid`,
/// `ysclid`, `_openstat`), Mailchimp (`mc_cid`, `mc_eid`), HubSpot (`_hsenc`, `_hsmi`,
/// `__hssc`, `__hstc`, `__hsfp`, `hsctatracking`), Marketo (`mkt_tok`), Omeda
/// (`oly_anon_id`, `oly_enc_id`), Vero (`vero_id`, `vero_conv`), Wicked Reports
/// (`wickedid`), Pinterest (`epik`), Rakuten (`rb_clickid`).
const ANYWHERE: &[&str] = &[
    "gclid",
    "gclsrc",
    "dclid",
    "gbraid",
    "wbraid",
    "gad_source",
    "gad_campaignid",
    "srsltid",
    "_gl",
    "fbclid",
    "igshid",
    "msclkid",
    "twclid",
    "ttclid",
    "li_fat_id",
    "yclid",
    "ysclid",
    "_openstat",
    "mc_cid",
    "mc_eid",
    "_hsenc",
    "_hsmi",
    "__hssc",
    "__hstc",
    "__hsfp",
    "hsctatracking",
    "mkt_tok",
    "oly_anon_id",
    "oly_enc_id",
    "vero_id",
    "vero_conv",
    "wickedid",
    "epik",
    "rb_clickid",
];

/// Prefixes removed everywhere: Urchin Tracking Module (`utm_source`, `utm_medium`, …).
const PREFIXES: &[&str] = &["utm_"];

/// Parameters removed only on certain sites, where they are known to be share trackers.
/// (site, parameter): `si` identifies who shared a Spotify or YouTube link; `igsh` the
/// Instagram sharer; `feature=share` is harmless but `pp` is YouTube's share payload.
const ON_SITE: &[(&str, &str)] = &[
    ("spotify.com", "si"),
    ("youtube.com", "si"),
    ("youtu.be", "si"),
    ("youtube.com", "pp"),
    ("instagram.com", "igsh"),
    ("x.com", "s"),
    ("x.com", "t"),
    ("twitter.com", "s"),
    ("twitter.com", "t"),
];

/// Redirect wrappers: (host, path, parameter holding the destination).
/// The wrapper exists to record the click; the destination is already in the address.
const WRAPPERS: &[(&str, &str, &[&str])] = &[
    ("www.google.com", "/url", &["q", "url"]),
    ("google.com", "/url", &["q", "url"]),
    ("www.google.com.tr", "/url", &["q", "url"]),
    ("l.facebook.com", "/l.php", &["u"]),
    ("lm.facebook.com", "/l.php", &["u"]),
    ("l.messenger.com", "/l.php", &["u"]),
    ("l.instagram.com", "/", &["u"]),
    ("l.threads.net", "/", &["u"]),
    ("out.reddit.com", "", &["url"]),
    ("www.youtube.com", "/redirect", &["q"]),
    ("steamcommunity.com", "/linkfilter/", &["u", "url"]),
    ("www.linkedin.com", "/redir/redirect", &["url"]),
    ("away.vk.com", "/away.php", &["to"]),
    ("slack-redir.net", "/link", &["url"]),
    ("t.umblr.com", "/redirect", &["z"]),
    ("href.li", "/", &[]),
];

/// Clean `url`, or `None` if nothing in it is tracking.
pub fn clean(url: &Url) -> Option<Cleaned> {
    let mut current = url.clone();
    let mut unwrapped_from = None;
    // A wrapper may wrap another (Google → Facebook → site); follow a few, no more.
    for _ in 0..3 {
        match unwrap(&current) {
            Some(inner) => {
                if unwrapped_from.is_none() {
                    unwrapped_from = current.host_str().map(str::to_string);
                }
                current = inner;
            }
            None => break,
        }
    }
    let removed = strip(&mut current);
    if removed.is_empty() && unwrapped_from.is_none() {
        return None;
    }
    Some(Cleaned {
        url: current,
        removed,
        unwrapped_from,
    })
}

/// The destination inside a redirect wrapper, if `url` is one and the destination is a web
/// address. Anything else (`javascript:`, a relative path) is not followed.
fn unwrap(url: &Url) -> Option<Url> {
    let host = url.host_str()?;
    let (_, _, params) = WRAPPERS
        .iter()
        .find(|(h, p, _)| h.eq_ignore_ascii_case(host) && (p.is_empty() || url.path() == *p))?;
    let target = if params.is_empty() {
        // href.li/?https://example.com: the whole query is the destination.
        url.query().map(str::to_string)
    } else {
        url.query_pairs()
            .find(|(k, _)| params.iter().any(|p| k == p))
            .map(|(_, v)| v.into_owned())
    }?;
    let inner = Url::parse(&target).ok()?;
    matches!(inner.scheme(), "http" | "https").then_some(inner)
}

/// Remove tracking parameters in place; returns their names.
fn strip(url: &mut Url) -> Vec<String> {
    let Some(query) = url.query() else {
        return Vec::new();
    };
    if query.is_empty() {
        return Vec::new();
    }
    let site = crate::site::of(url).unwrap_or_default();
    // Work on the raw `&`-separated parts, so what is kept is kept byte for byte (sites
    // differ in how they encode `+`, `%20` and empty values; re-encoding breaks some).
    let mut kept: Vec<&str> = Vec::new();
    let mut removed = Vec::new();
    for part in query.split('&') {
        let raw_name = part.split('=').next().unwrap_or(part);
        let name = percent_decode(raw_name).to_ascii_lowercase();
        if is_tracking(&name, &site) {
            removed.push(name);
        } else {
            kept.push(part);
        }
    }
    if removed.is_empty() {
        return removed;
    }
    let rest = kept.join("&");
    url.set_query(if rest.is_empty() { None } else { Some(&rest) });
    removed
}

fn is_tracking(name: &str, site: &str) -> bool {
    ANYWHERE.contains(&name)
        || PREFIXES.iter().any(|p| name.starts_with(p))
        || ON_SITE.iter().any(|(s, p)| *p == name && site == *s)
}

/// Percent-decoding a parameter name, leniently (an invalid escape is kept as written).
fn percent_decode(s: &str) -> String {
    url::form_urlencoded::parse(format!("{s}=").as_bytes())
        .next()
        .map_or_else(|| s.to_string(), |(k, _)| k.into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> Url {
        Url::parse(s).unwrap_or_else(|e| unreachable!("{s}: {e}"))
    }

    fn cleaned(s: &str) -> String {
        clean(&u(s)).map_or_else(|| s.to_string(), |c| c.url.to_string())
    }

    #[test]
    fn strips_the_common_trackers() {
        assert_eq!(
            cleaned("https://example.com/a?utm_source=x&id=7&utm_medium=email&fbclid=abc"),
            "https://example.com/a?id=7"
        );
        assert_eq!(
            cleaned("https://shop.example/p?gclid=1&gad_source=1&srsltid=AfmB"),
            "https://shop.example/p"
        );
        assert_eq!(
            cleaned("https://example.com/?MSCLKID=1"),
            "https://example.com/"
        );
    }

    #[test]
    fn keeps_what_is_not_tracking() {
        for s in [
            "https://example.com/search?q=utm&ref=home&source=nav",
            "https://www.amazon.com.tr/dp/B0?tag=someone-21",
            "https://example.com/?si=1",
            "https://example.com/",
            "https://example.com/path#utm_source=fragment-is-not-sent",
        ] {
            assert!(clean(&u(s)).is_none(), "{s}");
        }
    }

    #[test]
    fn site_specific_share_trackers() {
        assert_eq!(
            cleaned("https://open.spotify.com/track/1?si=abc123"),
            "https://open.spotify.com/track/1"
        );
        assert_eq!(
            cleaned("https://youtu.be/dQw4?si=XyZ&t=42"),
            "https://youtu.be/dQw4?t=42"
        );
        assert_eq!(
            cleaned("https://www.instagram.com/p/C1/?igsh=MWQ"),
            "https://www.instagram.com/p/C1/"
        );
    }

    #[test]
    fn keeps_the_rest_of_the_query_byte_for_byte() {
        assert_eq!(
            cleaned("https://e.com/?a=1+2&utm_campaign=z&b=%20x&c"),
            "https://e.com/?a=1+2&b=%20x&c"
        );
    }

    #[test]
    fn unwraps_redirect_wrappers() {
        let c = clean(&u(
            "https://www.google.com/url?sa=t&url=https%3A%2F%2Fwww.rust-lang.org%2Flearn%3Futm_source%3Dgoogle&ved=2a",
        ));
        let c = c.unwrap_or_else(|| unreachable!());
        assert_eq!(c.url.as_str(), "https://www.rust-lang.org/learn");
        assert_eq!(c.unwrapped_from.as_deref(), Some("www.google.com"));
        assert_eq!(c.removed, vec!["utm_source"]);

        assert_eq!(
            cleaned(
                "https://l.facebook.com/l.php?u=https%3A%2F%2Fexample.org%2F%3Ffbclid%3DIw&h=AT0"
            ),
            "https://example.org/"
        );
        assert_eq!(
            cleaned("https://www.youtube.com/redirect?event=video_description&q=https%3A%2F%2Fservo.org"),
            "https://servo.org/"
        );
        assert_eq!(
            cleaned("https://href.li/?https://example.net/x"),
            "https://example.net/x"
        );
    }

    #[test]
    fn never_unwraps_into_something_that_is_not_the_web() {
        for s in [
            "https://www.google.com/url?q=javascript:alert(1)",
            "https://l.facebook.com/l.php?u=file:///etc/passwd",
            "https://www.google.com/url?q=/relative",
            "https://www.google.com/search?q=https://example.com",
        ] {
            assert!(clean(&u(s)).is_none(), "{s}");
        }
    }
}
