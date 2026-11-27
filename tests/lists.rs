// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 EON contributors

//! The filter-list cycle as the application drives it: a snapshot from the release, the
//! person's consent, fetching through the application's function, refusing what is not a
//! list, and the compiled shield cached between starts.

use std::cell::RefCell;
use std::fmt::Write as _;

use eon_browser_core::lists::{Lists, Origin};
use eon_browser_core::shield::{Kind, Request, Shield, Verdict};
use eon_browser_core::Result;

fn list(title: &str, rules: &[&str]) -> Vec<u8> {
    let mut s = format!("[Adblock Plus 2.0]\n! Title: {title}\n! Expires: 2 days\n");
    for r in rules {
        s.push_str(r);
        s.push('\n');
    }
    // Pad to the minimum a real list has.
    for i in 0..12 {
        let _ = writeln!(s, "||filler{i}.invalid^");
    }
    s.into_bytes()
}

const DAY: u64 = 24 * 3600;

#[test]
fn seed_consent_update_and_refusal() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let mut lists = Lists::load(dir.path())?;

    // The release's snapshot: blocking works with no request at all.
    assert!(lists.seed("easylist", &list("EasyList", &["||ads.example^"]), 1_000)?);
    assert!(
        !lists.seed("easylist", &list("EasyList", &["||older.example^"]), 900)?,
        "an older snapshot never replaces a newer copy"
    );
    let shield = Shield::build(&lists)?;
    let page = "https://news.example.org/";
    assert!(matches!(
        shield.check(&Request {
            url: "https://ads.example/a.js",
            source: page,
            kind: Kind::Script
        }),
        Verdict::Block { .. }
    ));

    // Not asked yet: update fetches nothing.
    let calls = RefCell::new(Vec::<String>::new());
    let fetch = |url: &str| -> std::result::Result<Vec<u8>, String> {
        calls.borrow_mut().push(url.to_string());
        if url.contains("easyprivacy") {
            Ok(b"<!DOCTYPE html><html><body>Sign in to the Wi-Fi</body></html>".to_vec())
        } else if url.contains("cookiemonster") {
            Err("connection reset".into())
        } else if url.contains("ubo-unbreak") || url.contains("unbreak") {
            Ok(b"! Title: truncated\n||a^\n".to_vec())
        } else {
            Ok(list("fresh", &["||tracker.example^"]))
        }
    };
    let r = lists.update(fetch, 1_000 + 3 * DAY);
    assert!(r.not_allowed);
    assert!(
        calls.borrow().is_empty(),
        "no request before the person allowed it"
    );

    // Allowed: every due list is fetched; bad downloads are refused, the rest stored.
    lists.set_updates_allowed(true)?;
    let r = lists.update(fetch, 1_000 + 3 * DAY);
    assert!(!r.not_allowed);
    assert!(r.updated.contains(&"easylist".to_string()), "{r:?}");
    let failed: Vec<&str> = r.failed.iter().map(|(id, _)| id.as_str()).collect();
    assert!(
        failed.contains(&"easyprivacy"),
        "an HTML page is not a list: {r:?}"
    );
    assert!(
        failed.contains(&"easylist-cookie"),
        "a network error is reported: {r:?}"
    );
    assert!(
        failed.contains(&"ubo-unbreak"),
        "a truncated list is refused: {r:?}"
    );
    assert!(
        lists.text("easyprivacy")?.is_none(),
        "nothing stored for a refused list"
    );
    // Lists that are off are never fetched.
    assert!(!calls
        .borrow()
        .iter()
        .any(|u| u.contains("annoyance") || u.contains("yoyo.org")));

    let status = lists.status();
    let easylist = status
        .iter()
        .find(|s| s.info.id == "easylist")
        .and_then(|s| s.stored.clone());
    let easylist =
        easylist.ok_or_else(|| eon_browser_core::Error::UnknownList("easylist".into()))?;
    assert_eq!(easylist.origin, Origin::Fetched);
    assert_eq!(
        easylist.expires_hours, 48,
        "the list's own `! Expires: 2 days`"
    );

    // Not due again until it expires.
    calls.borrow_mut().clear();
    let r = lists.update(fetch, 1_000 + 3 * DAY + DAY);
    assert!(r.fresh.contains(&"easylist".to_string()));
    assert!(!calls.borrow().iter().any(|u| u.contains("easylist.txt")));

    // The new copy is what the shield uses now.
    let shield = Shield::build(&lists)?;
    assert!(matches!(
        shield.check(&Request {
            url: "https://tracker.example/t.js",
            source: page,
            kind: Kind::Script
        }),
        Verdict::Block { .. }
    ));
    assert_eq!(
        shield.check(&Request {
            url: "https://ads.example/a.js",
            source: page,
            kind: Kind::Script
        }),
        Verdict::Allow
    );
    Ok(())
}

#[test]
fn turning_a_list_off_and_custom_rules() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let mut lists = Lists::load(dir.path())?;
    lists.seed("easylist", &list("EasyList", &["||ads.example^"]), 1)?;
    lists.set_custom_rules("||mine.example^\n")?;
    let page = "https://site.org/";
    let blocked = |s: &Shield, url: &str| {
        matches!(
            s.check(&Request {
                url,
                source: page,
                kind: Kind::Image
            }),
            Verdict::Block { .. }
        )
    };
    let s = Shield::build(&lists)?;
    assert!(blocked(&s, "https://ads.example/x.png") && blocked(&s, "https://mine.example/x.png"));

    lists.set_enabled("easylist", false)?;
    let s = Shield::build(&lists)?;
    assert!(!blocked(&s, "https://ads.example/x.png"));
    assert!(blocked(&s, "https://mine.example/x.png"));

    assert!(lists.set_enabled("no-such-list", true).is_err());

    // State survives reopening.
    let again = Lists::load(dir.path())?;
    assert!(!again.is_enabled("easylist"));
    assert!(again.is_enabled("easyprivacy"));
    assert!(
        !again.is_enabled("fanboy-annoyance"),
        "off unless the person turns it on"
    );
    Ok(())
}

#[test]
fn a_copy_changed_on_disk_is_not_trusted() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let mut lists = Lists::load(dir.path())?;
    lists.seed("easylist", &list("EasyList", &["||ads.example^"]), 1)?;
    std::fs::write(dir.path().join("easylist.txt"), "@@||*^\n")?;
    assert!(lists.text("easylist")?.is_none());
    Ok(())
}

#[test]
fn compiled_shield_is_cached_and_invalidated() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let cache = dir.path().join("cache");
    let mut lists = Lists::load(&dir.path().join("lists"))?;
    lists.seed("easylist", &list("EasyList", &["||ads.example^"]), 1)?;
    let page = "https://site.org/";
    let check = |s: &Shield, url: &str| {
        s.check(&Request {
            url,
            source: page,
            kind: Kind::Script,
        })
    };

    let first = Shield::build_cached(&lists, &cache)?;
    let second = Shield::build_cached(&lists, &cache)?; // from the file
    for s in [&first, &second] {
        assert!(matches!(
            check(s, "https://ads.example/a.js"),
            Verdict::Block { .. }
        ));
    }
    assert_eq!(std::fs::read_dir(&cache)?.count(), 1);

    lists.set_custom_rules("||new.example^\n")?;
    let third = Shield::build_cached(&lists, &cache)?;
    assert!(matches!(
        check(&third, "https://new.example/a.js"),
        Verdict::Block { .. }
    ));
    assert_eq!(
        std::fs::read_dir(&cache)?.count(),
        1,
        "the stale copy is removed"
    );
    Ok(())
}

#[test]
fn an_extensions_declarative_rules_block_alongside_the_lists() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let mut lists = Lists::load(dir.path())?;
    lists.seed("easylist", &list("EasyList", &["||ads.example^"]), 1)?;
    let rules = r#"[{"id":1,"action":{"type":"block"},"condition":{"urlFilter":"||ext-blocked.example^","resourceTypes":["script"]}}]"#;
    let t = eon_browser_core::extension::dnr::translate(rules)
        .map_err(eon_browser_core::Error::BadUrl)?;
    lists.set_extension_rules("ublock-lite@example", &t.lines)?;
    let page = "https://site.org/";
    let s = Shield::build(&lists)?;
    let blocked = |s: &Shield, url: &str| {
        matches!(
            s.check(&Request {
                url,
                source: page,
                kind: Kind::Script
            }),
            Verdict::Block { .. }
        )
    };
    assert!(blocked(&s, "https://ext-blocked.example/x.js"));
    assert!(blocked(&s, "https://ads.example/x.js"));
    let before = lists.digest()?;
    lists.remove_extension_rules("ublock-lite@example")?;
    assert_ne!(before, lists.digest()?, "a cached shield is rebuilt");
    assert!(!blocked(
        &Shield::build(&lists)?,
        "https://ext-blocked.example/x.js"
    ));
    Ok(())
}
