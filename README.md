<!--
SPDX-License-Identifier: GPL-3.0-or-later
SPDX-FileCopyrightText: 2026 EON contributors
-->

# eon-browser-core

The core of **EON Browser** (plan madde 50): the policy that protects a person while they
browse, written once and independent of the engine. A library with no window, no engine
and **no network access of its own**; EON Browser's application links it and runs Servo.

```rust
let mut lists = Lists::load(&dir.join("lists"))?;
lists.seed("easylist", SNAPSHOT, built_at)?;            // shipped with the release: no request
lists.update(|url| app_fetch(url), now);                 // only after the person allowed it
let shield = Shield::build_cached(&lists, &cache)?;      // ~100 ms to compile, ~20 ms from cache

shield.check(&Request { url, source: page, kind: Kind::Script });   // ~5 µs, every request
shield.cosmetic(page);                                   // what to hide on this page

let nav = navigate(&typed_url, site.http_allowed);       // unwrap redirects, strip trackers, HTTPS
omnibox::classify("rust servo", &search::catalogue(), Some("duckduckgo"));
profile.visit(&url, Some(title), now)?;  profile.suggest("wik", 8)?;
fingerprint::user_agent(Platform::current());            // Firefox ESR's, Servo's token removed
```

## What it does

| Module | |
|---|---|
| `lists` | The catalogue (EasyList, EasyPrivacy, uBlock Origin's lists, EasyList Cookie; Fanboy's Annoyance and Peter Lowe's offered, off), each with its address, homepage and licence. Seeding from the release, updating through the application's fetch, refusing error pages and truncated downloads, verifying the stored copy. |
| `shield` | Network blocking and element hiding over Brave's `adblock` engine; no Manifest V3 ceiling. Top-level pages are never silently blocked: `check_document` lets the application warn instead. |
| `clean` | Tracking parameters stripped (`utm_*`, `gclid`, `fbclid`, `msclkid`, … each owner documented) and redirect wrappers skipped (`google.com/url`, `l.facebook.com`, …). Affiliate tags are neither added nor removed. |
| `upgrade` | HTTPS-only, with the honest exemptions: this computer, this network, `.onion`, and the person's own per-site choice. |
| `omnibox`, `search` | What a typed line means; a search is never sent to DNS and an address never to a search engine. Search engines alphabetical, none default until the person chooses, suggestions off. |
| `profile` | History with full-text search (diacritics folded: "buyuksehir" finds "Büyükşehir"), bookmarks, the session, per-site settings. One SQLite file; an in-memory profile for private windows. Credentials in addresses are never stored. |
| `fingerprint` | The user agent and `Accept-Language` of Firefox ESR on the same system; Global Privacy Control; no Do Not Track. |
| `site` | Which site an address belongs to, from the public suffix list. |

## Tests

`cargo test` — 40 tests, no network. `tests/invariants.rs` checks that the dependency tree
holds no HTTP client, TLS stack or async runtime.

`cargo test --release --test real_lists -- --ignored --nocapture` downloads the default
lists with `curl` (standing in for the application), compiles them and checks real
trackers (Google Analytics, Tag Manager, AdSense, DoubleClick, Meta Pixel, Hotjar, Bing)
are blocked while CDNs, fonts and Wikimedia are not. Measured on 26 November 2026:
compile 107 ms, load from cache 18 ms, **5.2 µs per request**.

## Licence

`GPL-3.0-or-later` with the EON Module ABI Exception (`LICENSE-EXCEPTION.md`). The `adblock`
dependency is MPL-2.0, combined as a Larger Work; see `deny.toml`.
