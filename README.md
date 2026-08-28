# kcv

Store environment secrets in the macOS keychain and inject them into a process.

```sh
kcv --environment prod set DATABASE_URL=postgres://localhost/app
kcv --environment prod exec -- ./server --port 8000
```

All of an environment's variables live in **one** keychain item, so reading an
environment costs exactly one authorization — one Touch ID or password prompt,
whether the environment holds two secrets or two hundred.

## Install

```sh
cargo install --path .
```

## Usage

`-e` is short for `--environment`, and both fall back to `$KCV_ENV`:

```sh
export KCV_ENV=prod
kcv set API_KEY=abc123
kcv exec -- ./server
```

### set

Assign directly, or omit the `=` to be prompted without echo — which keeps the
secret out of your shell history and out of `ps`:

```sh
kcv -e prod set API_KEY=abc123 REGION=us-east-1   # several at once
kcv -e prod set API_KEY                           # prompts: Value for API_KEY:
echo "$TOKEN" | kcv -e prod set API_KEY           # reads stdin when piped
```

`set` merges into the environment: variables you don't mention are left alone.
Values may contain `=`, spaces, newlines, and Unicode. `set` never prints a
value back to you.

### exec

```sh
kcv -e prod exec -- ./server --port 8000
kcv -e prod exec -- psql
```

Everything after `--` is passed through untouched, including arguments that
look like flags. `kcv` replaces itself with the command via `execvp`, so the
process keeps the same PID, terminal, and signal handling. Interactive programs
and TUIs — `vim`, `htop`, `psql`, `top` — behave exactly as they would without
the wrapper, and the command's exit code is yours.

Stored variables take precedence over ones already set in your shell.

## How it works

Each environment is a single generic-password keychain item, with service `kcv`
and account set to the environment name. Its data is a JSON object of all that
environment's variables.

The item uses the keychain's default access control, meaning it trusts the
binary that created it. The first read shows the standard macOS authorization
dialog, which accepts Touch ID or your login password and offers "Always
Allow"; after that, reads are silent.

**Rebuilding or moving the `kcv` binary changes its code identity, so macOS
will ask for approval once more.** That is expected, not a bug.

## Environment variables

| Variable | Effect |
|---|---|
| `KCV_ENV` | Supplies the environment name when `-e/--environment` is absent |
| `KCV_KEYCHAIN` | Use this keychain file instead of the default. Mainly for tests |

## Exit codes

| Code | Meaning |
|---|---|
| 0 | Success |
| 1 | Keychain error, missing environment, or corrupt item |
| 2 | Usage error |
| 126 | The command exists but could not be executed |
| 127 | The command was not found |
| *other* | Passed through from the executed command |

## Limitations

Secrets are placed in the child process's environment block, which other
processes running as your user can read. This is inherent to how environment
variable injection works — `direnv` and dotenv loaders share the property.

The authorization dialog is a GUI prompt. Over SSH with no GUI session, a read
that still needs approval will fail rather than prompt; once approved locally,
later reads succeed.

macOS only.

## Development

```sh
cargo test     # 48 tests; uses throwaway keychains, never your login keychain
cargo clippy --all-targets -- -D warnings
```

Design notes are in `docs/superpowers/specs/`.

## CI

`.github/workflows/ci.yml` runs fmt, clippy, and the full suite on every push
and pull request. `.github/workflows/release.yml` fires on a `v*` tag: it
re-runs the checks, verifies the tag matches the version in `Cargo.toml`,
builds a universal binary for Apple Silicon and Intel, and attaches a tarball
plus its SHA-256 to a GitHub Release.

To cut a release:

```sh
# bump the version in Cargo.toml first, then:
git tag v0.1.0 && git push origin v0.1.0
```

Set the `MACOS_CERT_P12`, `MACOS_CERT_PASSWORD`, and `MACOS_SIGN_IDENTITY`
repository secrets to have releases code-signed. Without them the release still
succeeds, unsigned.
