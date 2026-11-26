// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 EON contributors

//! Promises made in plan madde 50, checked against the build itself.

use std::process::Command;

/// madde 50c: the core never touches the network. Its whole dependency tree (normal
/// dependencies, every platform) carries no HTTP client, TLS stack or socket library.
#[test]
fn the_core_has_no_network_client() {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let out = Command::new(cargo)
        .args([
            "tree",
            "-e",
            "normal",
            "--prefix",
            "none",
            "--target",
            "all",
            "--offline",
        ])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output();
    let out = match out {
        Ok(o) if o.status.success() => o,
        Ok(o) => {
            // Without a lock or offline index the tree cannot be listed; that is a test
            // environment problem, not a pass.
            unreachable!("cargo tree failed: {}", String::from_utf8_lossy(&o.stderr))
        }
        Err(e) => unreachable!("cargo tree could not run: {e}"),
    };
    let tree = String::from_utf8_lossy(&out.stdout);
    let forbidden = [
        "reqwest",
        "hyper",
        "ureq",
        "curl",
        "isahc",
        "attohttpc",
        "h2",
        "h3",
        "quinn",
        "rustls",
        "native-tls",
        "openssl",
        "tokio",
        "mio",
        "socket2",
    ];
    for line in tree.lines() {
        let name = line.split_whitespace().next().unwrap_or("");
        assert!(
            !forbidden.contains(&name),
            "network-capable dependency in the core: {line}"
        );
    }
    assert!(tree.contains("adblock"), "the tree was really listed");
}

/// madde 50, the user agent: nothing in it names the engine or the product.
#[test]
fn the_user_agent_does_not_single_anyone_out() {
    use eon_browser_core::fingerprint::{user_agent, Platform};
    let ua = user_agent(Platform::current());
    assert!(ua.starts_with("Mozilla/5.0 ("));
    assert!(!ua.contains("Servo") && !ua.contains("EON"));
}
