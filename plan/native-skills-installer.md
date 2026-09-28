# Task: Native (pure-Rust) AI-skills installer — kill the PowerShell/iex spawn

Outcome: `tina4 update` / `tina4 skills <target>` install the Tina4 AI skills with
NO `powershell`/`sh`/`cmd`/`iex`/external `install-skills.*` execution. Same files,
same target dirs, same fetch sources/fallbacks, same checksum gate, same
`TINA4_SKILLS_*` env semantics. Fixes the Windows Defender/ASR block
(os error 225, ERROR_VIRUS_INFECTED) that killed the skills refresh after a
`tina4 update`.

Branch context: `tina4stack/tina4` (Rust CLI), default branch `main`. PR-only (ADR-0073).

## Scope
- [x] Characterize: read install-skills.sh + install-skills.ps1 + setup.rs skills path + download_file_classified
- [x] Confirm download primitive (`crate::download_file_classified`, curl-based) and SHA-256 option (`ring`, already in tree via rustls)
- [x] New `src/skills.rs`: native fetch → checksum-verify (ring SHA-256) → atomic publish
- [x] setup.rs `install_skills_target` delegates to `skills::install`; remove dead shell-out code + obsolete tests
- [x] Keep `windows_powershell`/`choose_powershell` (used by uv/claude/download paths — NOT skills)
- [x] Comment on the uv/Chocolatey/Claude `irm|iex` installers noting the same latent Defender risk (follow-up)
- [x] Unit tests in skills.rs: target→dirs, exact URL shapes, multi-host fallback, SHA-256 KAT, no `Command::new` in module
- [x] Integration test (real binary, file:// fixtures, no mocks): files land natively AND succeeds with `sh` absent from PATH (fails on old shell-out)
- [x] Rewrite tests/skills_refresh_reports_why_it_failed.rs for the native failure reporting
- [x] cargo test / clippy -Dwarnings / cargo-deny green on the lab
- [ ] PR into main; verify CI green (check, skills-installer, generator-parity x5, dco, cla)

## Behavior to preserve (from both scripts — identical)
- ref: env TINA4_SKILLS_REF else default `3.13.138`
- target: claude→~/.claude/skills, codex→~/.agents/skills, cursor→~/.cursor/skills, all→three; home = TINA4_SKILLS_HOME else HOME
- roots (env-overridable): tina4 https://tina4.com/skills ; jsdelivr https://cdn.jsdelivr.net/gh/tina4stack ; raw https://raw.githubusercontent.com/tina4stack
- per-file URL shapes: tina4 `{root}/{ref}/{skill}/{rel}` ; jsdelivr `{root}/{repo}@{ref}/.claude/skills/{skill}/{rel}` ; raw `{root}/{repo}/{ref}/.claude/skills/{skill}/{rel}`
- 8 installs (repo/skill/refs), DEV_REFS (8 files incl ai-coder-rule-path.svg), LEGACY_SKILLS=tina4-developer
- checksum: download skills.sha256 (tina4/jsdelivr/raw), verify every staged file, abort-all on mismatch/missing/empty
- publish: mkdir dest, remove legacy, atomic replace via `.{skill}.tina4-new`, write `.tina4-skills-ref`
- retries: 3 attempts, 2s delay (env TINA4_SKILLS_RETRY_COUNT/_DELAY)

## Decisions
- SHA-256 via `ring::digest` (already locked at 0.17.14 via rustls → zero new crates; vetted crypto, not hand-rolled). NOT the existing `sha256_of_file` (spawns `Get-FileHash` via powershell on Windows = the exact spawn we remove).
- Dead-host memory keyed on curl transport failure (`DownloadOutcome::Local`); HTTP error (`Http`, covers 4xx+5xx) → try next source. Same files land; only outage-perf differs slightly from the scripts (documented). Rust binary is not gated by skills_installer_http.py.

## Tests (written/updated, real — no mocks)
- [x] skills.rs: destinations_for_target, skill_urls exact strings, roots span >1 host, sha256_hex KAT (NIST vectors), module has no `Command::new(`
- [x] tests/skills_native_install.rs: real binary + file:// fixtures → files land + manifest verified + ref marker; and succeeds with `sh`/`powershell` OFF PATH (regression proof vs old shell-out)
- [x] tests/skills_refresh_reports_why_it_failed.rs: native failure (unreachable roots) reports why + skip line, no shell spawned

## Bugs
- [ ] (log here)

## Commits
- (hash  description)

## Status: PR open — CI verifying
