# Changelog

All notable changes to this project are documented here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Operator dashboard at `/dashboard` — payments list with status filtering and
  cursor pagination, payment detail, webhook delivery history with one-click
  redelivery, and a live health indicator. Built as dependency-free HTML/CSS/JS
  compiled into the binary, so there is no build step and no separate deploy.
- Deployment stack under `deploy/` — Docker Compose (app + Caddy for automatic
  TLS), an Oracle Cloud bootstrap script, and a systemd unit — plus a
  production runbook (`DEPLOYMENT.md`) covering first deploy, secrets, backups,
  upgrades, rollback, alerting signals, and scaling limits. The gateway is not
  published on a host port; Caddy is the only route in.
- `.dockerignore`, cutting the Docker build context from ~7 GB to a few hundred
  kilobytes. Without it every image build shipped the whole `target/` directory.
- Repository furniture: issue and pull request templates, Dependabot
  configuration, `.editorconfig`, `.gitattributes`, and a pinned
  `rust-toolchain.toml`.
- `ALLOWED_WEBHOOK_SCHEMES` documented in `.env.example`.

### Changed

- Minimum supported Rust version is now **1.88**, declared consistently in
  `Cargo.toml`, the CI matrix, the Dockerfile, and `rust-toolchain.toml`. The
  previously declared 1.75 was unreachable — `time` requires 1.88 and `url`'s
  `icu_*` chain requires 1.86.
- `main.rs` startup wiring collapsed into `spawn_task`/`join_task` helpers,
  removing four near-identical spawn blocks and a macro that existed only to
  work around the same repetition. Behaviour unchanged.
- README rewritten against the actual API surface.

### Fixed

- **The build.** `main` did not compile. An unclosed block in
  `rate_limit_middleware` plus a reversion to the pre-`moka` `Mutex` API, a
  duplicated struct field and an unterminated character literal in `config.rs`,
  and a dropped `elapsed_secs` helper whose three call sites remained.
- `Cargo.lock` disagreed with `Cargo.toml`, so every `--locked` CI step failed.
  Resolved by removing the unused `url` dependency — the code uses
  `reqwest::Url`, a re-export.
- HTTPS is again enforced for `webhook_url` on the public network. The rule had
  been replaced by the configurable scheme allow-list in a commit that never
  compiled, leaving its test failing; both gates now apply, so a permissive
  `ALLOWED_WEBHOOK_SCHEMES` cannot downgrade mainnet delivery to plaintext.
- Supply-chain CI, red on every push and weekly cron: bumped `event-listener`
  to the patched 5.4.2 (RUSTSEC-2026-0221) and allowed the `ISC` and
  `CDLA-Permissive-2.0` licences the rustls stack brings in. Dropped the now-
  unused `OpenSSL` licence allowance so it cannot return unnoticed.
- The Docker healthcheck invoked `curl`, which the runtime image did not
  install — containers reported unhealthy while serving traffic normally.

## [0.1.0] - 2026-07-29

Initial development release: payment intents, Horizon SSE and polling
listeners, payment verification, signed webhooks with retry and redrive,
multi-merchant API keys, intent expiry, SSRF protection, rate limiting, and
Prometheus metrics.

[Unreleased]: https://github.com/StellarGateLabs/StellarGate/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/StellarGateLabs/StellarGate/releases/tag/v0.1.0
