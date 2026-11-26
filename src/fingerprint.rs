// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 EON contributors

//! What the browser tells sites about itself (madde 50f).
//!
//! A new browser has no crowd to hide in, so it borrows one: the **user agent** is exactly
//! what Firefox ESR sends on the same operating system, and Servo's own `Servo/x.y` token
//! (which would make every EON Browser user one of a few thousand) is removed. The
//! `Accept-Language` header is the one Firefox sends for the same locale. **Global Privacy
//! Control** is sent everywhere; **Do Not Track is not** (sites ignore it and it adds a bit
//! to the fingerprint).
//!
//! Language and time zone are left alone on purpose, as Firefox's 2026 protections do:
//! hiding them breaks sites people rely on and buys little.

/// The Firefox ESR line whose user agent is borrowed. It moves when Firefox's ESR moves
/// (Servo itself presents Firefox 153 in its compatibility token).
pub const FIREFOX_ESR: u32 = 153;

/// The operating system, as a user agent states it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Windows,
    MacOs,
    Linux,
}

impl Platform {
    /// The platform this program was built for.
    pub const fn current() -> Platform {
        if cfg!(target_os = "windows") {
            Platform::Windows
        } else if cfg!(target_os = "macos") {
            Platform::MacOs
        } else {
            Platform::Linux
        }
    }
}

/// The user agent string sent to every site.
///
/// The CPU is stated the way Firefox's fingerprinting resistance states it, so ARM machines
/// look like the much larger x86-64 crowd: `Win64; x64`, `Intel Mac OS X 10.15` (frozen by
/// every browser since 2020) and `Linux x86_64`.
pub fn user_agent(platform: Platform) -> String {
    let os = match platform {
        Platform::Windows => "Windows NT 10.0; Win64; x64",
        Platform::MacOs => "Macintosh; Intel Mac OS X 10.15",
        Platform::Linux => "X11; Linux x86_64",
    };
    format!("Mozilla/5.0 ({os}; rv:{FIREFOX_ESR}.0) Gecko/20100101 Firefox/{FIREFOX_ESR}.0")
}

/// The `Accept-Language` header Firefox sends for `locale` (`"tr-TR"`, `"en-US"`, `"de"`):
/// the locale, its language, then English, with Firefox's own weights.
pub fn accept_language(locale: &str) -> String {
    let locale = locale.trim().replace('_', "-");
    let lang = locale
        .split('-')
        .next()
        .unwrap_or("en")
        .to_ascii_lowercase();
    let mut tags: Vec<String> = Vec::new();
    for t in [locale.clone(), lang, "en-US".into(), "en".into()] {
        if !t.is_empty() && !tags.iter().any(|x| x.eq_ignore_ascii_case(&t)) {
            tags.push(t);
        }
    }
    let n = tags.len();
    tags.iter()
        .enumerate()
        .map(|(i, t)| {
            if i == 0 {
                t.clone()
            } else {
                // Firefox: q = 1 - i/n, rounded to one decimal.
                let q = ((1.0 - i as f64 / n as f64) * 10.0).round() / 10.0;
                format!("{t};q={q}")
            }
        })
        .collect::<Vec<_>>()
        .join(",")
}

/// Headers added to every request.
pub fn headers() -> &'static [(&'static str, &'static str)] {
    &[("Sec-GPC", "1")]
}

/// The script run at the start of every document, in every frame: `navigator.
/// globalPrivacyControl`, so a page's script sees the same signal its server got.
pub fn page_script() -> &'static str {
    r"(() => {
  try {
    if (!('globalPrivacyControl' in Navigator.prototype)) {
      Object.defineProperty(Navigator.prototype, 'globalPrivacyControl', {
        get() { return true; }, configurable: true, enumerable: true,
      });
    }
  } catch (_) {}
})();"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_agents_borrow_firefox_esr_and_drop_servo() {
        assert_eq!(
            user_agent(Platform::Windows),
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:153.0) Gecko/20100101 Firefox/153.0"
        );
        assert_eq!(
            user_agent(Platform::MacOs),
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10.15; rv:153.0) Gecko/20100101 Firefox/153.0"
        );
        for p in [Platform::Windows, Platform::MacOs, Platform::Linux] {
            let ua = user_agent(p);
            assert!(!ua.contains("Servo") && !ua.contains("EON"), "{ua}");
        }
    }

    #[test]
    fn accept_language_matches_firefox() {
        assert_eq!(accept_language("en-US"), "en-US,en;q=0.5");
        assert_eq!(
            accept_language("tr-TR"),
            "tr-TR,tr;q=0.8,en-US;q=0.5,en;q=0.3"
        );
        assert_eq!(accept_language("de"), "de,en-US;q=0.7,en;q=0.3");
        assert_eq!(accept_language("en"), "en,en-US;q=0.5");
    }

    #[test]
    fn gpc_yes_dnt_no() {
        assert!(headers().contains(&("Sec-GPC", "1")));
        assert!(!headers().iter().any(|(h, _)| h.eq_ignore_ascii_case("DNT")));
        assert!(page_script().contains("globalPrivacyControl"));
    }
}
