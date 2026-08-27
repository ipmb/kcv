use crate::error::Result;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::io::AsRawFd;

/// Strips a single trailing newline, and the `\r` of a preceding CRLF.
/// Internal and leading whitespace is preserved: a secret may legitimately
/// contain spaces.
fn trim_line(s: &str) -> &str {
    let s = s.strip_suffix('\n').unwrap_or(s);
    s.strip_suffix('\r').unwrap_or(s)
}

/// Restores the terminal's original flags when dropped, so that echo is
/// re-enabled on every exit path including errors and Ctrl-C.
pub struct EchoGuard {
    fd: Option<i32>,
    original: libc::termios,
}

impl EchoGuard {
    /// A guard that does nothing, for the non-TTY path.
    pub fn none() -> Self {
        Self {
            fd: None,
            // Never applied, because `fd` is None.
            original: unsafe { std::mem::zeroed() },
        }
    }

    /// Clears the ECHO flag on `fd`, remembering the previous state.
    fn disable_echo(fd: i32) -> Result<Self> {
        unsafe {
            let mut original: libc::termios = std::mem::zeroed();
            if libc::tcgetattr(fd, &mut original) != 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            let mut quiet = original;
            quiet.c_lflag &= !libc::ECHO;
            if libc::tcsetattr(fd, libc::TCSAFLUSH, &quiet) != 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            Ok(Self {
                fd: Some(fd),
                original,
            })
        }
    }
}

impl Drop for EchoGuard {
    fn drop(&mut self) {
        if let Some(fd) = self.fd {
            unsafe {
                libc::tcsetattr(fd, libc::TCSAFLUSH, &self.original);
            }
        }
    }
}

/// Reads one line from an arbitrary reader. Split out so the parsing is
/// testable without a terminal.
fn read_secret_from<R: Read>(reader: &mut R) -> Result<String> {
    let mut line = String::new();
    BufReader::new(reader).read_line(&mut line)?;
    Ok(trim_line(&line).to_string())
}

/// Prompts for a secret without echoing it. Falls back to reading stdin when
/// stdin is not a terminal, so `echo secret | kcv -e prod set FOO` works.
pub fn read_secret(key: &str) -> Result<String> {
    let stdin_is_tty = unsafe { libc::isatty(libc::STDIN_FILENO) == 1 };
    if !stdin_is_tty {
        return read_secret_from(&mut std::io::stdin());
    }

    let tty = File::options().read(true).write(true).open("/dev/tty")?;
    eprint!("Value for {key}: ");
    std::io::stderr().flush()?;

    let _guard = EchoGuard::disable_echo(tty.as_raw_fd())?;
    let mut line = String::new();
    BufReader::new(&tty).read_line(&mut line)?;
    // The user's Return was not echoed, so emit the newline ourselves.
    eprintln!();
    Ok(trim_line(&line).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trims_exactly_one_trailing_newline() {
        assert_eq!(trim_line("secret\n"), "secret");
        assert_eq!(trim_line("secret\r\n"), "secret");
        assert_eq!(trim_line("secret"), "secret");
        assert_eq!(trim_line("secret\n\n"), "secret\n");
    }

    #[test]
    fn preserves_internal_whitespace_and_empty_input() {
        assert_eq!(trim_line("  padded value  \n"), "  padded value  ");
        assert_eq!(trim_line("\n"), "");
        assert_eq!(trim_line(""), "");
    }

    #[test]
    fn reads_a_secret_from_a_non_tty_reader() {
        let mut input = &b"piped-secret\n"[..];
        assert_eq!(read_secret_from(&mut input).unwrap(), "piped-secret");
    }

    #[test]
    fn echo_guard_restores_flags_on_drop() {
        let guard = EchoGuard::none();
        drop(guard);
    }
}
