# kcv

Stores environment variables in the macOS keychain and injects them into a
process.

```sh
kcv --environment prod set DATABASE_URL=postgres://localhost/app
kcv --environment prod exec -- ./server --port 8000
```

Each environment is stored as a single keychain item, so reading one requires a
single authorization regardless of how many variables it holds.

macOS only. `kcv` links against the system Security framework.

## Install

```sh
cargo install --path .
```

Requires a Rust toolchain. The binary has no runtime dependencies.

## Commands

`-e` is an alias for `--environment`. Both fall back to `$KCV_ENV`.

### set

```sh
kcv -e prod set API_KEY=abc123
kcv -e prod set API_KEY=abc123 REGION=us-east-1
kcv -e prod set API_KEY
echo "$TOKEN" | kcv -e prod set API_KEY
```

Arguments are split on the first `=`, so values may contain `=`. An empty value
(`FOO=`) is valid and stores an empty string.

An argument with no `=` is a key whose value is read separately. When stdin is a
terminal, `kcv` prompts for it with echo disabled. Otherwise it reads one line
from stdin. Either way the value stays out of shell history and out of `ps`.

`set` merges into the existing environment. Variables that are not named are
left unchanged. Values may contain spaces, newlines, and Unicode. Nothing is
written until every argument has been resolved, so a rejected key leaves the
stored data untouched.

`set` reports the number of variables written. It does not print values.

### exec

```sh
kcv -e prod exec -- ./server --port 8000
kcv -e prod exec -- psql
```

Everything after `--` is passed through unmodified, including arguments that
begin with `-`.

`kcv` replaces itself with the command using `execvp`. The command keeps the
same process ID, standard streams, controlling terminal, process group, and
session. Interactive programs and full-screen terminal applications behave as
they do when run directly, and job control works normally. The command's exit
code becomes the exit code of `kcv`.

Stored variables are layered over the inherited environment. A variable present
in both takes its stored value.

## Storage

Each environment is one generic-password keychain item:

| Attribute | Value |
|---|---|
| Service | `kcv` |
| Account | the environment name |
| Data | a JSON object of the environment's variables |

The item uses the keychain's default access control, which trusts the binary
that created it. The first read from a given build shows the standard macOS
authorization dialog, which accepts Touch ID or the login password and offers
"Always Allow". Later reads are silent.

Rebuilding or moving the binary changes its code identity, and macOS then asks
for approval once more. Signing the binary with a stable certificate avoids
this.

## Environment variables

| Variable | Effect |
|---|---|
| `KCV_ENV` | Supplies the environment name when `-e`/`--environment` is absent |
| `KCV_KEYCHAIN` | Path to a keychain file to use instead of the default |

## Exit codes

| Code | Meaning |
|---|---|
| 0 | Success |
| 1 | Keychain error, missing environment, or unreadable stored data |
| 2 | Usage error |
| 126 | The command was found but could not be executed |
| 127 | The command was not found |
| other | Passed through from the executed command |

## Limits

A keychain item holds far more than `exec` can pass on. Measured on an Apple
Silicon Mac:

| Limit | Value |
|---|---|
| Keychain item | At least 64 MB. No failure was observed |
| `exec` | About 1 MB, set by `ARG_MAX` (1048576 bytes) |

The `exec` limit applies to the whole child environment at once: every variable
name and value, plus the inherited environment, plus the command's own
arguments. Exceeding it fails with `Argument list too long` and exit code 126.
Note the asymmetry: `set` will store data that `exec` cannot then pass on.

Because the entire item is read on each invocation, time per `exec` grows with
the stored size:

| Stored data | Time per `exec` |
|---|---|
| 20 variables, about 800 bytes | 17 ms |
| 16 KB | 22 ms |
| 256 KB | 33 ms |
| 1000 KB | 62 ms |

About 17 ms of this is fixed cost: process startup plus one keychain read. These
figures are for an authorized binary against an unlocked keychain and do not
include the authorization dialog.

## Limitations

Secrets are placed in the child process's environment block, which other
processes running as the same user can read. This applies to environment
variable injection generally, including `direnv` and dotenv loaders.

The authorization dialog requires a GUI session. Over SSH with no GUI, a read
that still needs approval fails instead of prompting. Once approved locally,
later reads succeed over SSH.

Values are held in ordinary heap allocations and are not zeroed after use.

There is no command to list, read back, or delete stored variables. Those are
planned but not implemented.

## Development

```sh
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

The suite is 48 tests. Keychain coverage runs against throwaway keychain files
created and deleted by the tests, so the login keychain is never touched and no
authorization dialog appears.

Four direct dependencies: `clap`, `serde_json`, `security-framework`, and
`libc`, for 28 crates in total.

Design notes are in `docs/superpowers/specs/`.

## CI

`.github/workflows/ci.yml` runs formatting, lint, and tests on every push and
pull request.

`.github/workflows/release.yml` runs on a `v*` tag. It repeats the checks,
verifies the tag matches the version in `Cargo.toml`, builds a universal binary
for Apple Silicon and Intel, and attaches a tarball and its SHA-256 to a GitHub
release.

```sh
# update the version in Cargo.toml first
git tag v0.1.0 && git push origin v0.1.0
```

Releases are signed when the `MACOS_CERT_P12`, `MACOS_CERT_PASSWORD`, and
`MACOS_SIGN_IDENTITY` repository secrets are set. Without them the release still
succeeds and the binary is unsigned.
