// Copyright (c) 2026 Code Infinity
// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The skills refresh has to say why it failed — now for the NATIVE installer.
//!
//! A report once arrived showing `Installing tina4 AI skills for all...`, then
//! the skip line, and nothing in between — no cause. The native installer keeps
//! that contract: when it cannot fetch the skill files, it prints the reason and
//! the "run later" hint, and returns rather than hanging or clobbering anything.
//!
//! This drives the REAL binary at roots that resolve to nothing (a `file://`
//! path that does not exist), so every download fails locally with no network.
//! It also runs with a PATH that has `curl` and NO shell, proving the failure
//! path — like the success path — spawns no `sh` / `powershell` of its own.

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

fn private_dir(label: &str) -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!("tina4-skills-why-{}-{label}-{stamp}", std::process::id()));
    fs::create_dir(&dir).expect("could not create a private temp directory");
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).expect("could not narrow it");
    dir
}

fn which_curl() -> PathBuf {
    for dir in std::env::var("PATH").unwrap_or_default().split(':') {
        let candidate = Path::new(dir).join("curl");
        if candidate.is_file() {
            return candidate;
        }
    }
    panic!("no curl on PATH; the CLI's download primitive needs it");
}

/// A PATH holding a real `curl` and nothing else — no `sh`, no `powershell`.
fn curl_only_path() -> PathBuf {
    let bin = private_dir("curl-only");
    fs::copy(which_curl(), bin.join("curl")).expect("could not stage curl");
    fs::set_permissions(bin.join("curl"), fs::Permissions::from_mode(0o755)).unwrap();
    bin
}

#[test]
fn a_refresh_that_cannot_download_says_why_and_spawns_no_shell() {
    let home = private_dir("home");
    let bin = curl_only_path();
    // A root that resolves to nothing, so every source fails fast and locally.
    let dead_root = format!("file://{}", private_dir("empty").join("does-not-exist").display());

    let out = Command::new(env!("CARGO_BIN_EXE_tina4"))
        .args(["skills", "all"])
        .env("PATH", &bin)
        .env("HOME", &home)
        .env("TINA4_SKILLS_HOME", &home)
        .env("TINA4_SKILLS_REF", "0.0.0-test")
        .env("TINA4_SKILLS_TINA4_ROOT", &dead_root)
        .env("TINA4_SKILLS_JSDELIVR_ROOT", &dead_root)
        .env("TINA4_SKILLS_RAW_ROOT", &dead_root)
        .env("TINA4_SKILLS_RETRY_DELAY", "0")
        .env("TINA4_SKILLS_RETRY_COUNT", "1")
        .output()
        .expect("could not run the tina4 binary");

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let combined = format!("{stdout}{stderr}");

    // It named a cause — not a bare skip with nothing above it.
    assert!(
        combined.contains("every download source failed"),
        "the refresh failed without naming the cause:\n{combined}"
    );
    // It printed the skip hint.
    assert!(
        stdout.contains("Skills install skipped"),
        "the refresh did not report the skip:\n{stdout}"
    );
    // Running with no shell on PATH proves the native path did not reach for one:
    // a spawn of an absent `sh`/`powershell` would surface as a different error.
    assert!(
        !combined.contains("could not be started") && !combined.contains("No such file"),
        "the installer tried to spawn a program that was not on PATH:\n{combined}"
    );
    // Nothing was published.
    assert!(!home.join(".claude").join("skills").join("tina4-maintainer").exists());

    let _ = fs::remove_dir_all(&home);
    let _ = fs::remove_dir_all(&bin);
}
