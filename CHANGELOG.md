<!--
SPDX-License-Identifier: GPL-3.0-or-later
SPDX-FileCopyrightText: 2026 EON contributors
-->

# Changelog

## v0.0.0 — 26 November 2026

The first version (plan madde 50): filter lists and their update cycle, blocking and
element hiding over `adblock`, link cleaning, HTTPS-only, the address bar and search
engines, the profile (history with full-text search, bookmarks, session, site settings),
the user-agent policy. No engine, no window, no network client.

Found while testing: `adblock` answers generic class and id rules (`##.cookie-banner`) only
for the classes and ids a page has, as uBlock Origin does. `Shield::generic_css` and the
`COLLECT_CLASSES_AND_IDS` script are that second step.
