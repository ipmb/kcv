# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `list` command, printing an environment's variable names sorted, one per line,
  to stdout. Values are not printed. A missing environment is an error rather
  than empty output.
- `import` command, reading variables from a `.env` file and merging them into
  an environment. After a successful import it asks whether to delete the file,
  defaulting to no. Without a terminal the file is kept and a note is written to
  stderr. A file that fails to parse is neither partially imported nor deleted.
- Dotenv parsing covering comments, blank lines, an optional `export ` prefix,
  single- and double-quoted values, `\n` `\t` `\r` `\"` `\\` escapes inside
  double quotes, and quoted values spanning lines. Inline comments are not
  stripped from unquoted values, so a `#` in a password survives.

- CI reports test coverage and fails when line coverage drops below 90%.

### Fixed

- Prompts read from standard input rather than `/dev/tty`. Opening `/dev/tty`
  fails with `ENXIO` for a process that has a terminal on stdin but no
  controlling terminal, which made the hidden-value prompt unusable in some
  environments.

## [0.1.0] - 2026-08-27

### Added

- `set` command, storing one or more variables in a named environment.
  Arguments are split on the first `=`, so values may contain `=`. An argument
  with no `=` is read from a hidden terminal prompt, or from stdin when stdin is
  not a terminal.
- `exec` command, running a command with the environment's variables injected.
  Everything after `--` is passed through unmodified. The process is replaced
  using `execvp`, so the command keeps the same process ID, standard streams,
  controlling terminal, process group, and session, and its exit code is
  returned.
- Storage of each environment as a single macOS keychain generic-password item,
  with service `kcv`, account set to the environment name, and a JSON object as
  the data. One read per invocation means one authorization per environment.
- `--environment` / `-e` flag, falling back to the `KCV_ENV` environment
  variable.
- `KCV_KEYCHAIN` environment variable, selecting a keychain file other than the
  default.
- Exit codes: 2 for usage errors, 126 for a command that cannot be executed,
  127 for a command that was not found, and pass-through of the executed
  command's own exit code.
- GitHub Actions workflow running formatting, lint, and tests on every push and
  pull request.
- GitHub Actions workflow building a universal binary for Apple Silicon and
  Intel on a `v*` tag, verifying the tag matches `Cargo.toml`, and attaching a
  tarball and SHA-256 checksum to a GitHub release. Code signing runs when the
  `MACOS_CERT_P12`, `MACOS_CERT_PASSWORD`, and `MACOS_SIGN_IDENTITY` secrets are
  set, and is skipped otherwise.

- MIT license.

### Security

- Values are never written to stdout, stderr, log output, or error messages.
  `set` reports only the number of variables written.
- Values given without `=` are read from a hidden prompt or stdin rather than
  from the command line, keeping them out of shell history and out of `ps`.
- `set` resolves every argument before writing, so a rejected key leaves the
  stored data unchanged.
- A stored item that cannot be parsed is an error rather than being overwritten.

[Unreleased]: https://github.com/ipmb/kcv/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/ipmb/kcv/releases/tag/v0.1.0
