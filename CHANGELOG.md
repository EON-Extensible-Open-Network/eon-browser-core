<!--
SPDX-License-Identifier: GPL-3.0-or-later
SPDX-FileCopyrightText: 2026 EON contributors
-->

# Changelog

## Unreleased (v0.0.1)

Other browsers' extensions, stage 1 (plan madde 50h): `extension::package` unpacks Chrome's
CRX3, Firefox's XPI and plain zips without writing outside their folder;
`extension::manifest` reads MV2 and MV3 manifests and says in sentences what an
extension asks for; `extension::pattern` matches WebExtensions match patterns;
`extension::dnr` translates `declarativeNetRequest` rules into Adblock Plus lines that run
in the shield, and `Lists::set_extension_rules` keeps them beside the lists.

Found while testing: a DNR `urlFilter` like `/banner/` is a regular expression to Adblock
Plus, and `adblock` reads a lone negated type (`$~document`) as matching nothing; both are
translated so the DNR meaning holds (`/banner/*`, an explicit list of types).

## v0.0.0 — 26 November 2026

The first version (plan madde 50): filter lists and their update cycle, blocking and
element hiding over `adblock`, link cleaning, HTTPS-only, the address bar and search
engines, the profile (history with full-text search, bookmarks, session, site settings),
the user-agent policy. No engine, no window, no network client.

Found while testing: `adblock` answers generic class and id rules (`##.cookie-banner`) only
for the classes and ids a page has, as uBlock Origin does. `Shield::generic_css` and the
`COLLECT_CLASSES_AND_IDS` script are that second step.
