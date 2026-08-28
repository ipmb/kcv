# kcv

Stores environment variables in the macOS keychain and injects them into a
process.

```sh
kcv --environment myproject set DATABASE_URL=postgres://localhost/app
kcv --environment myproject exec -- ./server --port 8000
```

Each environment is stored as a single keychain item, so reading one requires a
single authorization regardless of how many variables it holds.

`kcv` is designed for local development. A project's secrets live in the
encrypted keychain database, readable only by programs you have approved,
rather than in a plaintext `.env` file that any process running as you can read
and that is easy to commit or back up by accident.

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
kcv -e myproject set API_KEY=abc123
kcv -e myproject set API_KEY=abc123 REGION=us-east-1
kcv -e myproject set API_KEY
echo "$TOKEN" | kcv -e myproject set API_KEY
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

### list

```sh
kcv -e myproject list
```

Prints the environment's variable names, sorted, one per line. Values are not
printed, so the output is safe to show on a screen share and safe to paste into
a log:

```
API_KEY
DATABASE_URL
REGION
```

Nothing but names goes to stdout, so it pipes:

```sh
kcv -e myproject list | grep DATABASE
kcv -e myproject list | wc -l
```

Listing reads the keychain item, so it costs one authorization like any other
read. The names are stored inside the encrypted blob and cannot be retrieved
without decrypting it.

### unset

```sh
kcv -e myproject unset API_KEY
kcv -e myproject unset API_KEY REGION
```

Removes one or more variables. A name that is not present is an error, and
nothing is removed, so a typo in one of several names leaves the whole
environment intact.

Removing the last variable deletes the environment, rather than leaving an
empty one that nothing could clean up.

### environments

```sh
kcv environments
kcv envs
```

Prints every environment name, sorted, one per line. Takes no `--environment`
flag. Printing nothing means no environments exist, which is not an error.

This is the only read in `kcv` that needs no authorization: it searches
keychain attributes and never decrypts item data. It therefore reports which
environments exist, not which ones you are approved to read.

### import

```sh
kcv -e myproject import .env
```

Reads a `.env` file and merges its variables into the environment, then asks
whether to delete the file:

```
Imported 7 variables into environment "myproject"
Delete .env? [y/N]
```

The answer defaults to no, so pressing Return keeps the file. When there is no
terminal to ask on, the file is kept and a line on stderr says so. A file that
fails to parse is never partially imported, and is never deleted.

The parser handles comments, blank lines, an optional `export ` prefix, and
single- and double-quoted values. Inside double quotes, `\n`, `\t`, `\r`,
`\"`, and `\\` are interpreted and the value may span lines. Single-quoted
values are literal. Unquoted values run to the end of the line.

Inline comments are not stripped from unquoted values, so
`PASSWORD=hunter2#x` stores `hunter2#x`. Quote the value if you want a trailing
comment.

### exec

```sh
kcv -e myproject exec -- ./server --port 8000
kcv -e myproject exec -- psql
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

## Limitations

Secrets are placed in the child process's environment block, which other
processes running as the same user can read. This applies to environment
variable injection generally, including `direnv` and dotenv loaders.

The authorization dialog requires a GUI session. A read that still needs
approval will block waiting for a dialog that cannot appear, rather than
failing quickly, so `set`, `list`, `import` and `exec` can hang in a session
with no GUI. Approve the binary once locally and later reads succeed without a
dialog. `environments` is unaffected, since it never decrypts item data.

Values are held in ordinary heap allocations and are not zeroed after use.

There is no command to read back a single value. That is planned but not
implemented.

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

## License

MIT. See [LICENSE](LICENSE).
