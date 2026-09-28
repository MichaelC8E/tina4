// Copyright (c) 2026 Code Infinity
// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Native Tina4 AI-skills installer.
//!
//! This replaces the old shell-out. The client used to download
//! `install-skills.sh` / `install-skills.ps1` and run it — on Windows as
//! `powershell -Command "iex ([System.IO.File]::ReadAllText('<downloaded .ps1>'))"`.
//! A freshly-updated, low-reputation `tina4.exe` spawning PowerShell to `iex`
//! a just-downloaded script is exactly the shape Windows Defender / Attack
//! Surface Reduction blocks: a real `tina4 update` came back with
//! `os error 225` (ERROR_VIRUS_INFECTED) and installed no skills. The binary is
//! EV-signed and fine; the spawn was the problem.
//!
//! So the whole installer now runs in-process. It fetches each skill file with
//! the CLI's own download primitive (`crate::download_file_classified`),
//! verifies every file against the published `skills.sha256` manifest with
//! `ring`'s SHA-256, and writes the files into the target directories itself.
//! No `powershell`, no `sh`, no `cmd`, no `iex`, no `install-skills.*` — this
//! module spawns no process of its own (the download primitive it calls prefers
//! `curl`/`curl.exe`, the same one `tina4 update` already uses to fetch its own
//! binary, which Defender does not object to).
//!
//! The behaviour is a faithful port of both scripts: same sources and fallback
//! order, same `TINA4_SKILLS_*` environment variables, same target directories,
//! the same checksum gate, and the same atomic publish. The one deliberate
//! difference is the dead-host optimisation: `download_file_classified` reports
//! an HTTP error without distinguishing 4xx from 5xx, so a host is written off
//! for the rest of the run only on a transport failure (no answer / DNS /
//! connect / TLS), while any HTTP error moves to the next source for that one
//! file. The set of files that land, and where, is identical.

use std::fs;
use std::path::{Path, PathBuf};

use crate::console::{icon_ok, icon_play, icon_warn};
use colored::Colorize;

/// The skills ref installed when `TINA4_SKILLS_REF` is unset. Pinned to a
/// released tag, not a moving branch, so an install is reproducible. This is the
/// same default the two installer scripts carry; bump it (and `skills.sha256`)
/// in the release that changes the skills.
const DEFAULT_REF: &str = "3.13.138";

/// The three fetch tiers, in priority order. tina4.com FIRST so the common path
/// never depends on GitHub raw (which 503s during GitHub incidents); jsDelivr and
/// raw are automatic fallbacks. Each tier has its OWN path shape — see
/// [`skill_urls`] — so a tier is not just a swapped root prefix.
const DEFAULT_TINA4_ROOT: &str = "https://tina4.com/skills";
const DEFAULT_JSDELIVR_ROOT: &str = "https://cdn.jsdelivr.net/gh/tina4stack";
const DEFAULT_RAW_ROOT: &str = "https://raw.githubusercontent.com/tina4stack";

/// Attempts per source are `retry_count + 1`; `retry_delay` seconds between them.
/// The same defaults the scripts use.
const DEFAULT_RETRY_COUNT: u32 = 3;
const DEFAULT_RETRY_DELAY_SECS: u64 = 2;

/// A ceiling on the whole fetch walk, mirroring `fetch_skills_installer`'s
/// budget: the download primitive runs `curl` without a timeout, so a host that
/// accepts a connection and then says nothing can hang, and the retries would
/// multiply that wait. Once this much time has gone, no further attempt starts.
const FETCH_BUDGET_SECS: u64 = 60;

/// Every reference file a per-language developer skill ships. Every file under
/// `references/`, not most of them — `ai-coder-rule-path.svg` was once omitted,
/// so a SUCCESSFUL install still produced an incomplete skill.
const DEV_REFS: &[&str] = &[
    "auth-and-services.md",
    "data-and-orm.md",
    "deployment.md",
    "routes-and-api.md",
    "templates-and-frontend.md",
    "realtime.md",
    "web-push.md",
    "ai-coder-rule-path.svg",
];

/// Skill directories from an older layout that must be removed from a target
/// before the current skills are written.
const LEGACY_SKILLS: &[&str] = &["tina4-developer"];

/// One skill to fetch: which framework repo hosts it (only used to build the
/// jsDelivr and raw URLs) and which `references/` files it ships alongside
/// `SKILL.md`.
struct SkillSpec {
    repo: &'static str,
    skill: &'static str,
    refs: &'static [&'static str],
}

/// The eight skills, in the scripts' order. Per-language developer skills come
/// from their own framework repo; `tina4-js` and `tina4-maintainer` are shared
/// and served from `tina4-python`.
const INSTALLS: &[SkillSpec] = &[
    SkillSpec { repo: "tina4-python", skill: "tina4-developer-python", refs: DEV_REFS },
    SkillSpec { repo: "tina4-php", skill: "tina4-developer-php", refs: DEV_REFS },
    SkillSpec { repo: "tina4-ruby", skill: "tina4-developer-ruby", refs: DEV_REFS },
    SkillSpec { repo: "tina4-nodejs", skill: "tina4-developer-nodejs", refs: DEV_REFS },
    SkillSpec {
        repo: "tina4-python",
        skill: "tina4-js",
        refs: &["html-and-components.md", "signals-and-reactivity.md", "persistence.md", "rtc.md"],
    },
    SkillSpec {
        repo: "tina4-python",
        skill: "tina4-maintainer",
        refs: &["cli-and-deployment.md", "frond-and-frontend.md", "routing-and-orm.md", "subsystems.md"],
    },
    SkillSpec { repo: "tina4-python", skill: "tina4-architect", refs: &[] },
    SkillSpec { repo: "tina4-python", skill: "tina4-design", refs: &[] },
];

/// The resolved configuration for one install run, read from the environment
/// once so every file is fetched from the same tiers at the same ref.
struct Config {
    reference: String,
    tina4_root: String,
    jsdelivr_root: String,
    raw_root: String,
    retry_count: u32,
    retry_delay: std::time::Duration,
}

impl Config {
    fn from_env() -> Self {
        let env = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty());
        let num = |name: &str, default: u64| -> u64 {
            env(name).and_then(|v| v.parse().ok()).unwrap_or(default)
        };
        Config {
            reference: env("TINA4_SKILLS_REF").unwrap_or_else(|| DEFAULT_REF.to_string()),
            tina4_root: env("TINA4_SKILLS_TINA4_ROOT").unwrap_or_else(|| DEFAULT_TINA4_ROOT.to_string()),
            jsdelivr_root: env("TINA4_SKILLS_JSDELIVR_ROOT")
                .unwrap_or_else(|| DEFAULT_JSDELIVR_ROOT.to_string()),
            raw_root: env("TINA4_SKILLS_RAW_ROOT").unwrap_or_else(|| DEFAULT_RAW_ROOT.to_string()),
            retry_count: num("TINA4_SKILLS_RETRY_COUNT", DEFAULT_RETRY_COUNT as u64) as u32,
            retry_delay: std::time::Duration::from_secs(num(
                "TINA4_SKILLS_RETRY_DELAY",
                DEFAULT_RETRY_DELAY_SECS,
            )),
        }
    }

    /// Candidate URLs for one skill file, in priority order. Whitespace-safe:
    /// URLs never contain a space.
    fn skill_urls(&self, repo: &str, skill: &str, relative: &str) -> [String; 3] {
        [
            format!("{}/{}/{}/{}", self.tina4_root, self.reference, skill, relative),
            format!(
                "{}/{}@{}/.claude/skills/{}/{}",
                self.jsdelivr_root, repo, self.reference, skill, relative
            ),
            format!(
                "{}/{}/{}/.claude/skills/{}/{}",
                self.raw_root, repo, self.reference, skill, relative
            ),
        ]
    }

    /// Candidate URLs for the `skills.sha256` manifest, which lives in the
    /// `tina4` repo at this ref.
    fn manifest_urls(&self) -> [String; 3] {
        [
            format!("{}/{}/skills.sha256", self.tina4_root, self.reference),
            format!("{}/tina4@{}/skills.sha256", self.jsdelivr_root, self.reference),
            format!("{}/tina4/{}/skills.sha256", self.raw_root, self.reference),
        ]
    }
}

/// The three destinations a target maps to, under `home`.
fn destinations_for_target(target: &str, home: &Path) -> Option<Vec<PathBuf>> {
    let claude = || home.join(".claude").join("skills");
    let codex = || home.join(".agents").join("skills");
    let cursor = || home.join(".cursor").join("skills");
    match target {
        "claude" => Some(vec![claude()]),
        "codex" => Some(vec![codex()]),
        "cursor" => Some(vec![cursor()]),
        "all" => Some(vec![claude(), codex(), cursor()]),
        _ => None,
    }
}

/// `$TINA4_SKILLS_HOME`, else the user's home directory.
fn skill_home() -> Option<PathBuf> {
    if let Some(explicit) = std::env::var_os("TINA4_SKILLS_HOME").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(explicit));
    }
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// Install the skills for `target` (`claude` / `codex` / `cursor` / `all`).
/// Returns `true` on a fully verified, published install; on any failure it
/// prints the reason and a "run later" hint and returns `false`, leaving every
/// existing skill directory untouched (nothing is published unless every file
/// downloaded and verified).
pub fn install(target: &str) -> bool {
    println!("  {} Installing tina4 AI skills for {}...", icon_play().green(), target);
    match install_inner(target) {
        Ok(()) => true,
        Err(why) => {
            println!("  {} {}", icon_warn().yellow(), why);
            println!(
                "  {} Skills install skipped — run later: {}",
                icon_warn().yellow(),
                "tina4 ai".cyan()
            );
            false
        }
    }
}

fn install_inner(target: &str) -> Result<(), String> {
    let home = skill_home()
        .ok_or_else(|| "Could not find your home directory (HOME / USERPROFILE unset).".to_string())?;
    let destinations = destinations_for_target(target, &home).ok_or_else(|| {
        "Choose one of: tina4 skills claude, tina4 skills codex, tina4 skills cursor, or tina4 skills all."
            .to_string()
    })?;

    let config = Config::from_env();
    println!("  Target: {}  (ref: {})", target, config.reference);

    let stage = Stage::new()
        .map_err(|e| format!("Could not create a temporary directory for the skills — {e}"))?;

    // Fetch every skill file into the staging area.
    let mut dead_hosts: Vec<String> = Vec::new();
    let started = std::time::Instant::now();
    for spec in INSTALLS {
        let skill_dir = stage.path.join(spec.skill);
        fs::create_dir_all(skill_dir.join("references"))
            .map_err(|e| format!("Could not create the staging directory for {} — {e}", spec.skill))?;

        let skill_md = skill_dir.join("SKILL.md");
        download_file(
            &config,
            &skill_md,
            &config.skill_urls(spec.repo, spec.skill, "SKILL.md"),
            &mut dead_hosts,
            started,
        )?;
        for reference in spec.refs {
            let relative = format!("references/{reference}");
            download_file(
                &config,
                &skill_dir.join("references").join(reference),
                &config.skill_urls(spec.repo, spec.skill, &relative),
                &mut dead_hosts,
                started,
            )?;
        }
        println!("  {} {}  ({})", "+".green(), spec.skill, spec.repo);
    }

    // Verify every staged file against the published manifest BEFORE anything is
    // installed, so a tampered or truncated download can never reach a skills
    // directory.
    verify_checksums(&config, &stage.path, &mut dead_hosts, started)?;

    // Publish into every destination.
    for destination in &destinations {
        publish(&stage.path, destination, &config.reference)
            .map_err(|e| format!("Could not install the skills into {} — {e}", destination.display()))?;
        println!("  {} installed for {}", icon_ok().green(), destination.display());
    }

    println!(
        "  {} Done - eight skills installed for {} (ref {}). Restart your coding tool to pick them up.",
        icon_ok().green(),
        target,
        config.reference
    );
    Ok(())
}

/// A private staging directory that removes itself when dropped, so a failed or
/// panicking run never leaves a half-downloaded tree behind.
struct Stage {
    path: PathBuf,
}

impl Stage {
    /// `create_dir` fails if the path already exists, so this can never adopt a
    /// directory another user planted in a world-writable temp location; the
    /// name carries the pid and a nanosecond timestamp so a recycled pid cannot
    /// collide. On Unix it is narrowed to 0700.
    fn new() -> std::io::Result<Self> {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path =
            std::env::temp_dir().join(format!("tina4-skills-{}-{}", std::process::id(), stamp));
        fs::create_dir(&path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?;
        }
        Ok(Stage { path })
    }
}

impl Drop for Stage {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// What one fetch of one URL amounted to.
enum FetchResult {
    /// Got the file.
    Got,
    /// The server answered with an HTTP error (4xx/5xx). Says nothing about the
    /// next source, so move on to it for this file.
    NotHere,
    /// No HTTP answer at all (DNS, connect, TLS, timeout, transport). Equally
    /// true for every remaining file, so the host is worth writing off.
    HostDead,
}

/// `scheme://authority` for a URL, so two tiers pointing at the same host share
/// its fate. Falls back to the whole URL rather than an empty key, which would
/// collide with every other empty key.
fn host_of(url: &str) -> String {
    if let Some(scheme_end) = url.find("://") {
        let after = &url[scheme_end + 3..];
        let authority_end = after.find('/').unwrap_or(after.len());
        return url[..scheme_end + 3 + authority_end].to_string();
    }
    url.to_string()
}

/// One full retry walk against one URL. Retries only transport failures (the
/// download primitive cannot distinguish a 4xx from a 5xx, so an HTTP error is
/// treated as "not on this host" and handed straight to the next source).
fn fetch_one(config: &Config, url: &str, dest: &Path) -> FetchResult {
    let attempts = config.retry_count + 1;
    for attempt in 1..=attempts {
        match crate::download_file_classified(url, dest) {
            crate::DownloadOutcome::Ok => return FetchResult::Got,
            crate::DownloadOutcome::Http => {
                let _ = fs::remove_file(dest);
                return FetchResult::NotHere;
            }
            crate::DownloadOutcome::Local(_) => {
                let _ = fs::remove_file(dest);
                if attempt < attempts {
                    std::thread::sleep(config.retry_delay);
                }
            }
        }
    }
    FetchResult::HostDead
}

/// Fetch `dest` from `urls` in priority order. Two passes: the first skips hosts
/// already written off this run; the second tries exactly those skipped hosts,
/// so a source is never lost — a host is only skipped while another is still
/// worth trying.
fn download_file(
    config: &Config,
    dest: &Path,
    urls: &[String],
    dead_hosts: &mut Vec<String>,
    started: std::time::Instant,
) -> Result<(), String> {
    let budget = std::time::Duration::from_secs(FETCH_BUDGET_SECS);
    let mut skipped: Vec<&String> = Vec::new();
    for url in urls {
        let host = host_of(url);
        if dead_hosts.contains(&host) {
            skipped.push(url);
            continue;
        }
        if started.elapsed() >= budget {
            return Err(format!(
                "gave up fetching {} after {}s",
                dest.display(),
                started.elapsed().as_secs()
            ));
        }
        match fetch_one(config, url, dest) {
            FetchResult::Got => return Ok(()),
            FetchResult::HostDead => {
                if !dead_hosts.contains(&host) {
                    dead_hosts.push(host.clone());
                }
                eprintln!(
                    "  {} {host} is not answering; using the next source for the rest of this run",
                    "!".yellow()
                );
            }
            FetchResult::NotHere => {
                eprintln!("  {} not served by {host}, trying next source", "!".yellow());
            }
        }
    }
    for url in skipped {
        if let FetchResult::Got = fetch_one(config, url, dest) {
            return Ok(());
        }
    }
    Err(format!("every download source failed for {}", dest.display()))
}

/// Download the manifest and verify every staged file against it. A mismatch, a
/// missing named file, or an empty manifest aborts with nothing installed.
fn verify_checksums(
    config: &Config,
    stage: &Path,
    dead_hosts: &mut Vec<String>,
    started: std::time::Instant,
) -> Result<(), String> {
    let manifest_path = stage.join(".skills.sha256");
    download_file(config, &manifest_path, &config.manifest_urls(), dead_hosts, started)?;
    let manifest = fs::read_to_string(&manifest_path)
        .map_err(|e| format!("Could not read the skills checksum manifest — {e}"))?;
    let _ = fs::remove_file(&manifest_path);

    let mut verified = 0usize;
    for line in manifest.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let (expected, relative) = parse_manifest_line(line)
            .ok_or_else(|| format!("Malformed line in the skills checksum manifest: {line}"))?;
        // The manifest paths are forward-slashed and relative to the stage; join
        // them component by component so this is correct on Windows too and can
        // never escape the stage.
        let mut file = stage.to_path_buf();
        for component in relative.split('/') {
            if component.is_empty() || component == "." || component == ".." {
                return Err(format!("Unsafe path in the skills checksum manifest: {relative}"));
            }
            file.push(component);
        }
        let bytes = fs::read(&file).map_err(|_| {
            format!("A skill file named in the manifest was not downloaded: {relative}")
        })?;
        let actual = sha256_hex(&bytes);
        if !actual.eq_ignore_ascii_case(expected) {
            return Err(format!(
                "A skill file failed checksum verification (tampering or a stale manifest) — nothing installed: {relative}"
            ));
        }
        verified += 1;
    }
    if verified == 0 {
        return Err("The skills checksum manifest is empty — refusing to install.".to_string());
    }
    println!(
        "  {} verified {verified} skill files against skills.sha256 (ref {})",
        icon_ok().green(),
        config.reference
    );
    Ok(())
}

/// Parse a `sha256sum`-format line: 64 hex chars, whitespace, then the path.
/// Returns `(expected_hex, relative_path)`.
fn parse_manifest_line(line: &str) -> Option<(&str, &str)> {
    let line = line.trim_end();
    let (hash, rest) = line.split_once(|c: char| c.is_ascii_whitespace())?;
    if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let relative = rest.trim_start();
    // sha256sum marks a binary read with a leading '*'; the manifest is text, but
    // tolerate it so a binary-mode manifest still verifies.
    let relative = relative.strip_prefix('*').unwrap_or(relative);
    if relative.is_empty() {
        return None;
    }
    Some((hash, relative))
}

/// Lowercase hex SHA-256 of `bytes`, via `ring` (already in the dependency tree
/// through rustls). Not the CLI's `sha256_of_file`, which shells out to
/// `Get-FileHash` through PowerShell on Windows — the exact spawn this module
/// exists to remove.
fn sha256_hex(bytes: &[u8]) -> String {
    let digest = ring::digest::digest(&ring::digest::SHA256, bytes);
    let mut hex = String::with_capacity(64);
    for byte in digest.as_ref() {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

/// Publish every staged skill into `destination`, replacing each one atomically
/// and removing any legacy skill directories first.
fn publish(stage: &Path, destination: &Path, reference: &str) -> std::io::Result<()> {
    fs::create_dir_all(destination)?;

    for legacy in LEGACY_SKILLS {
        let legacy_path = destination.join(legacy);
        if legacy_path.exists() {
            fs::remove_dir_all(&legacy_path)?;
            println!("  {} removed legacy {legacy}", "-".yellow());
        }
    }

    for entry in fs::read_dir(stage)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let name = entry.file_name();
        let replacement = destination.join(format!(".{}.tina4-new", name.to_string_lossy()));
        let installed = destination.join(&name);
        // Build the new copy beside the target, then swing it into place with a
        // rename so a reader never sees a half-written skill.
        if replacement.exists() {
            fs::remove_dir_all(&replacement)?;
        }
        copy_dir_all(&entry.path(), &replacement)?;
        if installed.exists() {
            fs::remove_dir_all(&installed)?;
        }
        fs::rename(&replacement, &installed)?;
    }

    // Trimmed on read (see doctor::read_installed_skills_ref), so the trailing
    // newline is immaterial; kept to match install-skills.sh's marker.
    fs::write(destination.join(".tina4-skills-ref"), format!("{reference}\n"))?;
    Ok(())
}

/// Recursively copy a directory tree. The stdlib has no equivalent, and the
/// trees here are a handful of small files, so a plain recursion is the lean
/// choice over a new dependency.
fn copy_dir_all(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_all(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_target_maps_to_its_directories() {
        let home = Path::new("/home/dev");
        assert_eq!(
            destinations_for_target("claude", home),
            Some(vec![home.join(".claude").join("skills")])
        );
        assert_eq!(
            destinations_for_target("codex", home),
            Some(vec![home.join(".agents").join("skills")])
        );
        assert_eq!(
            destinations_for_target("cursor", home),
            Some(vec![home.join(".cursor").join("skills")])
        );
        assert_eq!(
            destinations_for_target("all", home),
            Some(vec![
                home.join(".claude").join("skills"),
                home.join(".agents").join("skills"),
                home.join(".cursor").join("skills"),
            ])
        );
        assert_eq!(destinations_for_target("nonsense", home), None);
    }

    fn default_config() -> Config {
        Config {
            reference: DEFAULT_REF.to_string(),
            tina4_root: DEFAULT_TINA4_ROOT.to_string(),
            jsdelivr_root: DEFAULT_JSDELIVR_ROOT.to_string(),
            raw_root: DEFAULT_RAW_ROOT.to_string(),
            retry_count: DEFAULT_RETRY_COUNT,
            retry_delay: std::time::Duration::from_secs(0),
        }
    }

    /// The exact URL shapes each tier serves. These must match install-skills.sh
    /// (`skill_urls`) and install-skills.ps1 (`Get-Tina4SkillUrls`) byte for byte,
    /// or a native install fetches from a different place than the scripts.
    #[test]
    fn skill_urls_match_the_scripts_for_each_tier() {
        let config = default_config();
        let urls = config.skill_urls("tina4-php", "tina4-developer-php", "references/realtime.md");
        assert_eq!(
            urls[0],
            "https://tina4.com/skills/3.13.138/tina4-developer-php/references/realtime.md"
        );
        assert_eq!(
            urls[1],
            "https://cdn.jsdelivr.net/gh/tina4stack/tina4-php@3.13.138/.claude/skills/tina4-developer-php/references/realtime.md"
        );
        assert_eq!(
            urls[2],
            "https://raw.githubusercontent.com/tina4stack/tina4-php/3.13.138/.claude/skills/tina4-developer-php/references/realtime.md"
        );

        let skill_md = config.skill_urls("tina4-python", "tina4-architect", "SKILL.md");
        assert_eq!(skill_md[0], "https://tina4.com/skills/3.13.138/tina4-architect/SKILL.md");
    }

    #[test]
    fn manifest_urls_point_at_the_tina4_repo() {
        let config = default_config();
        let urls = config.manifest_urls();
        assert_eq!(urls[0], "https://tina4.com/skills/3.13.138/skills.sha256");
        assert_eq!(urls[1], "https://cdn.jsdelivr.net/gh/tina4stack/tina4@3.13.138/skills.sha256");
        assert_eq!(urls[2], "https://raw.githubusercontent.com/tina4stack/tina4/3.13.138/skills.sha256");
    }

    /// The installer must have a second host to fall back to. A single source is
    /// what a 503 from raw.githubusercontent.com once turned into a failed
    /// `tina4 update`; collapsing the tiers onto one host brings that back.
    #[test]
    fn the_fetch_tiers_span_more_than_one_host() {
        let config = default_config();
        let urls = config.skill_urls("tina4-python", "tina4-js", "SKILL.md");
        let hosts: std::collections::BTreeSet<String> =
            urls.iter().map(|u| host_of(u)).collect();
        assert!(hosts.len() > 1, "every tier resolves to one host: {urls:?}");
    }

    #[test]
    fn host_of_extracts_scheme_and_authority() {
        assert_eq!(host_of("https://tina4.com/skills/3.13.138/x"), "https://tina4.com");
        assert_eq!(
            host_of("https://cdn.jsdelivr.net/gh/tina4stack/a@1/b"),
            "https://cdn.jsdelivr.net"
        );
        assert_eq!(host_of("file:///tmp/fixtures/x"), "file://");
        assert_eq!(host_of("not-a-url"), "not-a-url");
    }

    /// SHA-256 known-answer tests from FIPS 180-4 / RFC 6234, so the verifier is
    /// proven correct against published vectors, not against itself.
    #[test]
    fn sha256_matches_published_vectors() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha256_hex(b"The quick brown fox jumps over the lazy dog"),
            "d7a8fbb307d7809469ca9abcb0082e4f8d5651e46d3cdb762d02d0bf37c9e592"
        );
    }

    /// The manifest is `sha256sum` format: 64 hex, whitespace, path.
    #[test]
    fn manifest_lines_parse_and_reject_junk() {
        let hash = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        assert_eq!(
            parse_manifest_line(&format!("{hash}  tina4-js/SKILL.md")),
            Some((hash, "tina4-js/SKILL.md"))
        );
        // A binary-mode marker is tolerated.
        assert_eq!(
            parse_manifest_line(&format!("{hash} *tina4-js/SKILL.md")),
            Some((hash, "tina4-js/SKILL.md"))
        );
        assert_eq!(parse_manifest_line("deadbeef  short-hash.md"), None);
        assert_eq!(parse_manifest_line("no-hash-here"), None);
        assert_eq!(parse_manifest_line(&format!("{hash}   ")), None);
    }

    /// The whole point of this module: the skills install path spawns no process
    /// of its own. `Command::new` anywhere in it would reintroduce exactly the
    /// PowerShell/`sh` spawn the Windows Defender block was about. (The shared
    /// download primitive it calls lives in main.rs; that is `curl`, which
    /// `tina4 update` already uses to fetch its own binary.)
    #[test]
    fn the_skills_module_spawns_no_process() {
        // Scan only the production code — everything before the test module, so
        // this test's own needle strings ("powershell", "iex") are not counted —
        // and strip line comments (doc + `//`) so the prose explaining what this
        // module AVOIDS does not trip the check either.
        let source = include_str!("skills.rs");
        let production = source.split("\n#[cfg(test)]").next().unwrap_or(source);
        let code: String = production
            .lines()
            .map(|line| line.split("//").next().unwrap_or(""))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            !code.contains("Command::new"),
            "the native skills installer must spawn no process"
        );
        assert!(
            !code.contains("powershell") && !code.contains("iex"),
            "the native skills installer must not reach for PowerShell or iex"
        );
    }
}
