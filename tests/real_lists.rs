// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 EON contributors

//! The default lists, really downloaded, really compiled, checked against real trackers.
//!
//! `cargo test --test real_lists -- --ignored --nocapture`
//!
//! The test fetches with `curl` (present on every CI runner and on Windows 10+): the
//! *core* still has no network client; this is the test standing in for the application.
//! Copies are kept in `target/browser-test-lists` and reused for a day.

use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use eon_browser_core::lists::{catalogue, Lists};
use eon_browser_core::shield::{Kind, Request, Shield, Verdict};

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// The fetch the application would supply. A CI runner's network has bad seconds; three
/// tries, so the test measures the lists and not the runner.
fn curl(url: &str) -> Result<Vec<u8>, String> {
    let mut last = String::new();
    for _ in 0..3 {
        let out = Command::new("curl")
            .args(["-sSfL", "--retry", "2", "--max-time", "60", "--compressed", url])
            .output()
            .map_err(|e| e.to_string())?;
        if out.status.success() && !out.stdout.is_empty() {
            return Ok(out.stdout);
        }
        last = format!("{}: {}", out.status, String::from_utf8_lossy(&out.stderr).trim());
        std::thread::sleep(Duration::from_secs(3));
    }
    Err(last)
}

#[test]
#[allow(clippy::too_many_lines)] // one scenario, read top to bottom
#[ignore = "downloads the real lists (~5 MB)"]
fn real_lists_block_real_trackers() -> eon_browser_core::Result<()> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/browser-test-lists");
    let mut lists = Lists::load(&dir)?;
    lists.set_updates_allowed(true)?;
    let report = lists.update(curl, now());
    println!("update: {report:?}");
    assert!(
        report.failed.is_empty(),
        "every default list downloads and passes the checks"
    );
    for l in catalogue().iter().filter(|l| l.default_on) {
        assert!(lists.text(l.id)?.is_some(), "{} stored", l.id);
    }

    let t = Instant::now();
    let shield = Shield::build(&lists)?;
    let build = t.elapsed();
    let t = Instant::now();
    let cached = Shield::build_cached(&lists, &dir.join("cache"))?;
    let first_cache = t.elapsed();
    let t = Instant::now();
    let _again = Shield::build_cached(&lists, &dir.join("cache"))?;
    let from_cache = t.elapsed();
    println!("compile {build:?}; compile+write {first_cache:?}; load from cache {from_cache:?}");

    let page = "https://www.example-news.com/article";
    let blocked = [
        (
            "https://www.google-analytics.com/analytics.js",
            Kind::Script,
        ),
        (
            "https://www.googletagmanager.com/gtag/js?id=G-XXXX",
            Kind::Script,
        ),
        (
            "https://pagead2.googlesyndication.com/pagead/js/adsbygoogle.js",
            Kind::Script,
        ),
        (
            "https://securepubads.g.doubleclick.net/tag/js/gpt.js",
            Kind::Script,
        ),
        (
            "https://connect.facebook.net/en_US/fbevents.js",
            Kind::Script,
        ),
        (
            "https://static.hotjar.com/c/hotjar-123.js?sv=6",
            Kind::Script,
        ),
        ("https://bat.bing.com/bat.js", Kind::Script),
        ("https://www.facebook.com/tr?id=1&ev=PageView", Kind::Image),
    ];
    let allowed = [
        (
            "https://cdn.jsdelivr.net/npm/react@18.3.1/umd/react.production.min.js",
            Kind::Script,
        ),
        ("https://www.example-news.com/assets/app.js", Kind::Script),
        ("https://fonts.gstatic.com/s/inter/v13/x.woff2", Kind::Font),
        (
            "https://upload.wikimedia.org/wikipedia/commons/a/a1/x.jpg",
            Kind::Image,
        ),
        (
            "https://www.youtube.com/embed/aqz-KE-bpKQ",
            Kind::Subdocument,
        ),
    ];
    for s in [&shield, &cached] {
        for (url, kind) in blocked {
            let v = s.check(&Request {
                url,
                source: page,
                kind,
            });
            assert!(!matches!(v, Verdict::Allow), "should block {url}");
        }
        for (url, kind) in allowed {
            let v = s.check(&Request {
                url,
                source: page,
                kind,
            });
            assert_eq!(v, Verdict::Allow, "should allow {url}");
        }
    }

    // Speed: a busy page makes ~200 requests; the check must be invisible next to them.
    let urls: Vec<String> = (0..20_000)
        .map(|i| {
            format!(
                "https://cdn{}.example-cdn.net/assets/{i}/bundle.{}.js?v={i}",
                i % 50,
                i % 7
            )
        })
        .collect();
    let t = Instant::now();
    let mut n = 0;
    for u in &urls {
        if !matches!(
            shield.check(&Request {
                url: u,
                source: page,
                kind: Kind::Script
            }),
            Verdict::Allow
        ) {
            n += 1;
        }
    }
    let per = t.elapsed() / urls.len() as u32;
    println!("{} checks, {n} blocked, {per:?} per check", urls.len());
    assert!(per < Duration::from_micros(200), "a check costs {per:?}");

    let cosmetic = shield.cosmetic("https://www.example-news.com/article");
    println!(
        "site-specific hiding rules for a news page: {} bytes",
        cosmetic.css.len()
    );
    Ok(())
}
