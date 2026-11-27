// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 EON contributors

//! Other browsers' extensions on EON's own core (plan madde 50h, `browser/extensions.md`).
//!
//! Chrome, Edge, Opera and Firefox share one extension format, WebExtensions. This module
//! is everything about it that needs no engine: reading packages and manifests, saying in
//! words what an extension asks for, matching its patterns, and translating its
//! `declarativeNetRequest` rules into the shield's own engine. The application runs the
//! scripts in Servo.

pub mod dnr;
pub mod manifest;
pub mod package;
pub mod pattern;

pub use manifest::Manifest;
pub use pattern::Pattern;
