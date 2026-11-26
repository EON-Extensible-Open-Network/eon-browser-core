<!--
SPDX-License-Identifier: GPL-3.0-or-later
SPDX-FileCopyrightText: 2026 EON contributors
-->

# Security

Report privately through GitHub's **Security → Report a vulnerability** on this
repository; the organisation-wide policy is
[`.github/SECURITY.md`](https://github.com/EON-Extensible-Open-Network/.github/blob/main/SECURITY.md).

The core decides what a browser loads, so its mistakes are privacy or security mistakes.
The rules it keeps, each tested:

- **No network of its own.** Lists are fetched by the application; the core checks them.
  An HTML page, a captive portal's login page, a truncated or oversized download is refused
  and the previous copy kept. A stored copy whose SHA-256 no longer matches is not used.
- **Redirect wrappers are followed only into `http` and `https`.** `javascript:`, `file:`
  and relative targets are never produced by unwrapping.
- **Typed `javascript:` and `data:` addresses are searched, never run** (the "paste this in
  your address bar" scam).
- **Credentials in addresses are never stored** in history or bookmarks; non-web addresses
  are not recorded.
- **Element-hiding selectors that could escape their CSS rule** (`{`, `}`) are dropped.
- **Top-level pages are never silently blocked**; a list's `$document` rule becomes a warning
  the application shows.
