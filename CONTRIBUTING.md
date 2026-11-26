<!--
SPDX-License-Identifier: GPL-3.0-or-later
SPDX-FileCopyrightText: 2026 EON contributors
-->

# Contributing to EON Browser core

The organisation-wide guide,
[`.github/CONTRIBUTING.md`](https://github.com/EON-Extensible-Open-Network/.github/blob/main/CONTRIBUTING.md),
applies: DCO sign-off (`git commit -s`), Conventional Commits (`feat(shield): ...`,
`fix(clean): ...`). By contributing you license your work under `GPL-3.0-or-later` with the
EON Module ABI Exception.

- **No engine, no window, no network.** The core is the policy; Servo and the interface live
  in `eon-browser-app`. CI and `tests/invariants.rs` check it.
- **A tracking parameter is added with its owner** in the comment above `ANYWHERE`, and a
  test. Only parameters whose single purpose is tracking; affiliate tags are not touched.
- **A new list comes with its address checked, its licence as the publisher states it**, and
  `default_on` justified in `eon-docs/browser/plan.md`.
- **Nothing that phones home, no AI feature, no affiliate rewriting** (madde 36, 50d).
