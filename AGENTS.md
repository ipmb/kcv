# Notes for agents working on kcv

## Requirements

macOS, a stable Rust toolchain supporting edition 2024, and the Xcode command
line tools. `kcv` links against the system Security framework, so it neither
builds nor tests on other platforms.

If `cargo` is not found, check how the local toolchain was installed before
assuming it is missing. Some installers do not put it in `~/.cargo/bin`.

## Process

This repo is worked with the Superpowers skills. Use them:

- **`superpowers:brainstorming` before any feature or behaviour change.** Adding
  a command or changing how one behaves is a bounded task: ask the questions
  that matter, present a short design in chat, then stop and wait for an
  explicit yes. A new subsystem is architectural and gets a spec under
  `docs/superpowers/specs/` plus a plan under `docs/superpowers/plans/`.
- **`superpowers:test-driven-development`.** Write the failing test, run it,
  confirm it fails for the reason you expect, then implement.
- **`superpowers:verification-before-completion`.** Run the commands and read
  the output before claiming anything works. Several claims in this project's
  history looked obviously true and were wrong.
- **`unslop`** when writing prose. Documentation here is factual: no marketing,
  no emoji, no inflated significance. Scan with the skill's
  `banned_phrase_scan.py` before committing docs.

## Dependencies

Prefer writing roughly 100 lines of our own code over adding a dependency. When
a dependency genuinely is warranted it must be well vetted. There are four
direct dependencies and 28 crates in the tree; keep it that way.

`anyhow` and `rpassword` were deliberately removed in favour of a small error
enum and a direct `termios` call. Do not reintroduce that kind of convenience
wrapper. `security-framework` stays because hand-rolling the CoreFoundation FFI
would be about 250 lines of `unsafe` with manual Create/Get memory rules.

Measure the real cost before proposing a dependency:

```sh
cargo tree --edges normal --prefix none 2>/dev/null | sed 's/ (\*)$//' | sort -u | wc -l
```

Send stderr to `/dev/null` as shown. Cargo's "Downloaded ..." progress lines
otherwise land in the count and roughly double it.

## Architecture

`Store` in `src/store.rs` is the seam. It moves opaque bytes and knows nothing
about JSON or variables. Everything above it is testable against `MemStore`
with no keychain involved: merging, parsing, validation, environment overlay,
and argv handling. Keep new logic above that line.

One keychain item per environment is the defining constraint. It is what makes
a read cost a single authorization. Do not split an environment across items.

## Things that will bite you

- **Never touch the user's login keychain in a test.** Tests create a
  throwaway keychain with `CreateOptions::new().password(...).create(path)` and
  point `kcv` at it with `KCV_KEYCHAIN`, then delete it on drop. This is also
  why the suite needs no GUI prompt and works on CI.
- **Prompts read standard input, not `/dev/tty`.** Opening `/dev/tty` fails with
  `ENXIO` for a process that has a terminal on stdin but no controlling
  terminal. `is_tty()` already gates the interactive path, so stdin is correct
  and `/dev/tty` buys nothing.
- **`exec` must keep using `execvp`.** Replacing the process is what preserves
  the process ID, stdio, controlling terminal, process group and session, so
  TUIs and job control work. Switching to fork-and-wait means taking on signal
  forwarding, exit status relay and job control by hand.
- **The dotenv parser does not strip inline comments from unquoted values.** A
  `#` is more likely to be part of a password than the start of a comment, and
  truncating a secret is worse than keeping a stray note. This is deliberate and
  has a test.
- **Secrets never reach stdout, stderr, log output, error messages, or process
  arguments.** Error messages name keys, never values. There is a test asserting
  this for both `set` and `import`.
- **Rebuilding changes the binary's code identity**, so macOS re-prompts for
  keychain access, once per environment. That is expected. It is why the tests
  use throwaway keychains rather than a shared one.

## Commands

```sh
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

All three must pass before a commit. CI runs these plus a coverage check that
fails when line coverage drops below 90%:

```sh
cargo llvm-cov --all-targets --fail-under-lines 90
```

Coverage sits around 92%. The uncovered remainder is code that needs a real
terminal or the user's actual login keychain, neither of which belongs in an
automated test. Do not chase the number by testing `Display` strings.

Two behaviours need a real terminal and are not covered by the suite: the
hidden-value prompt and the import delete prompt. Verify it by hand with a pty
harness (`pty.openpty` plus `subprocess`) rather than assuming it works.

## Releases

Tagging `v*` triggers `.github/workflows/release.yml`, which verifies the tag
matches the version in `Cargo.toml` before building. Before tagging: bump
`Cargo.toml`, move the `[Unreleased]` entries in `CHANGELOG.md` into a dated
version section, and add the comparison links.

Release binaries are ad-hoc signed. The signing step no-ops unless
`MACOS_CERT_P12`, `MACOS_CERT_PASSWORD` and `MACOS_SIGN_IDENTITY` are set as
repository secrets.
