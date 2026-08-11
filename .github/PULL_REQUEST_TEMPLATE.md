<!-- Keep this short. The goal is that a reviewer can tell what changed and
     why without reading the diff first. -->

## What and why

<!-- What does this change, and what problem does it solve? Link the issue. -->

Closes #

## How it was verified

<!-- What did you actually run or observe? "Tests pass" is weaker than
     "added a test that fails without the fix". -->

- [ ] `cargo test` passes
- [ ] `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings` pass
- [ ] New behaviour is covered by a test that fails without this change

## Risk

<!-- Delete any that do not apply. -->

- [ ] Adds or changes a **database migration** (forward-only — see DEPLOYMENT.md)
- [ ] Changes a **public API response shape** or error `code`
- [ ] Touches **payment verification, settlement, or amount handling**
- [ ] Touches **authentication, the SSRF guard, or webhook signing**
- [ ] Adds or updates a **dependency** (`cargo deny check all` passes)
- [ ] Requires a **config/env change** to deploy (documented in README + `.env.example`)

## Notes for the reviewer

<!-- Anything you are unsure about, or deliberately left out of scope. -->
