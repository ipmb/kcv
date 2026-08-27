# kcv — keychain-backed environment secrets

Date: 2026-08-27
Status: approved for implementation

## Purpose

A macOS CLI that stores environment variables in the login keychain and injects
them into a child process. Two operations in v1:

```
kcv --environment prod set FOO=bar
kcv --environment prod exec -- ./server --port 8000
```

The defining constraint: **all secrets for an environment live in a single
keychain item.** Reading an environment is therefore exactly one `SecItem`
lookup, which means one authorization event regardless of how many variables
the environment holds.

## Non-goals for v1

Deliberately excluded, listed here so their absence is a decision rather than an
oversight: `list`, `get`, `rm`, `import`, `export`; biometric re-authentication
on every read; non-macOS platforms; in-memory secret zeroization; shell
completions.

## CLI surface

```
kcv --environment <name> set KEY=VALUE [KEY=VALUE ...]
kcv --environment <name> set KEY
kcv --environment <name> exec -- <command> [args ...]
```

`--environment` has the short alias `-e` and is a global flag, accepted before
the subcommand. When it is absent, the value falls back to the `KCV_ENV`
environment variable; if neither is present, `kcv` exits with a usage error.

`set` accepts one or more arguments. An argument containing `=` is split on the
**first** `=`: everything before is the key, everything after is the value, and
an empty value (`FOO=`) is legal and sets an empty string. An argument with no
`=` is a key whose value is prompted for: if stdin is a TTY, `kcv` reads it from
`/dev/tty` with echo disabled; otherwise it reads one line from stdin, so
`echo secret | kcv -e prod set FOO` works in scripts. Within a single
invocation, a repeated key takes its last value.

`set` writes a confirmation to stderr naming the environment and the number of
keys written. It never prints a value.

`exec` requires at least one argument after `--`. Since v1 has no command that
prints a secret, nothing writes secrets to stdout.

`KCV_KEYCHAIN` overrides the keychain file `kcv` operates on. It exists for
integration tests; when unset, `kcv` uses the user's default keychain.

## Storage model

Each environment is one generic-password keychain item:

| Attribute | Value |
|---|---|
| Service (`kSecAttrService`) | the constant `kcv` |
| Account (`kSecAttrAccount`) | the environment name |
| Data | UTF-8 JSON object of `{"KEY": "value"}` |

The JSON is serialized from a `BTreeMap<String, String>`, so key order is
deterministic and diffs in tests are stable.

Access control uses the keychain's default ACL: the item trusts the binary that
created it. The first read from a given build of `kcv` shows the standard macOS
authorization dialog, which accepts Touch ID or the login password and offers
"Always Allow"; subsequent reads are silent. Rebuilding or moving the binary
changes its code identity and costs one more approval. This is a known,
accepted trade-off, chosen over entitlement-gated biometry to keep the build
free of code-signing ceremony.

## Architecture

| File | Responsibility |
|---|---|
| `src/main.rs` | clap derive definitions, argument resolution, dispatch |
| `src/store.rs` | the `Store` trait; `KeychainStore` and `MemStore` |
| `src/envset.rs` | the key/value map: JSON round-trip, merge, key validation |
| `src/cmd.rs` | `set` and `exec` logic, written against `Store` |

`Store` is the seam that keeps the keychain out of nearly every test:

```rust
pub trait Store {
    fn load(&self, environment: &str) -> Result<Option<Vec<u8>>>;
    fn save(&self, environment: &str, data: &[u8]) -> Result<()>;
}
```

Two methods, opaque bytes, no JSON knowledge. `KeychainStore` wraps
`security-framework`; `MemStore` wraps a `BTreeMap` for tests. Every piece of
real logic — merging, parsing, validation, environment overlay, argv handling —
sits above this boundary.

`set` is a read-modify-write against the single item: load, parse (or start
empty when the item is absent), merge the new pairs, serialize, save.

## exec semantics

`exec` loads and parses the item, overlays the secrets onto the inherited
process environment, and then calls `execvp` via
`std::os::unix::process::CommandExt::exec`, replacing the current process rather
than forking. Exit codes and signals pass through untouched and no supervisor
process lingers in the tree.

Secrets overlay the inherited environment: a variable stored in `kcv` wins over
one already present in the shell.

Replacing the process rather than forking is what makes interactive programs and
TUIs work unchanged. The command inherits the same stdio file descriptors, the
same controlling terminal, and the same process group and session, so `isatty`
checks pass, job control (Ctrl-C, Ctrl-Z, `fg`) behaves normally, and SIGWINCH
reaches the child for redraw on resize. `kcv -e prod exec -- vim` or `-- psql`
is indistinguishable from running the command directly. No implementation may
replace `exec` with fork-and-wait without taking on signal forwarding, exit
status relay, and job control by hand.

Note that the keychain authorization dialog is a GUI prompt. In a session with
no GUI — over SSH, or on a CI runner — a read that still requires approval
fails rather than prompting. Once approved locally, later reads are silent and
succeed over SSH.

To keep this testable, the pure part is factored out:

```rust
fn resolve_exec(store: &dyn Store, environment: &str, argv: &[String])
    -> Result<(String, Vec<String>, Vec<(String, String)>)>
```

It returns the program, its arguments, and the full child environment. Tests
assert on that tuple; the thin `exec` wrapper that performs the syscall holds no
logic worth testing.

## Error handling

- A missing environment produces `no environment 'prod'` plus the `kcv -e prod
  set KEY=VALUE` hint, not a silently empty variable set.
- An item whose data is not valid UTF-8 JSON is a hard error. `set` refuses to
  overwrite it, so a corrupt item is never silently clobbered.
- Keys are rejected at write time when empty, or when they contain `=` or NUL.
- Environment names are rejected when empty or containing NUL.
- `exec` maps a missing command to exit status 127 and a non-executable command
  to 126, matching shell convention.
- Error messages name keys but never values.

## Testing strategy

Test-driven throughout.

Unit tests, no keychain involved:

- `envset`: JSON round-trip, merge semantics, last-write-wins within an
  invocation, preservation of untouched keys, rejection of invalid keys, error
  on corrupt JSON.
- `cmd` over `MemStore`: `set` creates a new environment, `set` merges into an
  existing one, `resolve_exec` overlays correctly and reports a missing
  environment, argument parsing splits on the first `=` and handles empty
  values.

Integration test, real keychain: create a temporary keychain file with a known
password via `SecKeychain::create`, unlock it, point `kcv` at it with
`KCV_KEYCHAIN`, round-trip a `set` and a read, and delete the keychain on
teardown. This exercises `KeychainStore` end to end without an authorization
prompt and without touching the login keychain.

## Dependencies

`clap` 4 (derive), `serde` 1 (derive), `serde_json` 1, `security-framework` 3,
`anyhow` 1, `rpassword` 7. Rust stable, edition 2024.

## Security notes

Secrets are placed in the child process's environment block, which is readable
by other processes running as the same user. This is inherent to the category —
`direnv` and dotenv loaders share the property — and is a real limit of the
tool, not an implementation defect.

Values are held in ordinary heap allocations and are not zeroized after use.

## Future work

`list` (key names, and an environment enumeration via an attribute-only search
that needs no authorization), `get`, `rm`, `import` from dotenv, `export`, and
biometry-on-every-read for users willing to sign the binary.
