use std::fmt;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    MissingEnvironment,
    NoCommand,
    InvalidEnvironmentName(String),
    InvalidKey(String),
    EnvironmentNotFound(String),
    CorruptItem(String),
    Dotenv {
        path: String,
        line: usize,
        reason: String,
    },
    EmptyImport(String),
    ReadFile {
        path: String,
        source: std::io::Error,
    },
    Keychain(security_framework::base::Error),
    Io(std::io::Error),
    Exec {
        program: String,
        source: std::io::Error,
    },
}

impl Error {
    /// Shell-conventional exit statuses: 2 for usage, 127 for a missing
    /// command, 126 for one that cannot be executed, 1 otherwise.
    pub fn exit_code(&self) -> i32 {
        match self {
            Error::MissingEnvironment
            | Error::InvalidEnvironmentName(_)
            | Error::InvalidKey(_)
            | Error::NoCommand => 2,
            Error::Exec { source, .. } => match source.kind() {
                std::io::ErrorKind::NotFound => 127,
                _ => 126,
            },
            _ => 1,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::MissingEnvironment => write!(
                f,
                "no environment given; pass --environment NAME or set KCV_ENV"
            ),
            Error::NoCommand => write!(f, "no command given; usage: kcv -e NAME exec -- COMMAND"),
            Error::InvalidEnvironmentName(n) => write!(f, "invalid environment name {n:?}"),
            Error::InvalidKey(k) => write!(
                f,
                "invalid variable name {k:?}: names must be non-empty and free of '=' and NUL"
            ),
            Error::EnvironmentNotFound(e) => write!(
                f,
                "no environment {e:?}; create it with: kcv -e {e} set KEY=VALUE"
            ),
            Error::CorruptItem(e) => write!(
                f,
                "the keychain item for environment {e:?} is not valid kcv data; \
                 refusing to overwrite it"
            ),
            Error::Dotenv { path, line, reason } => {
                write!(f, "{path}:{line}: {reason}")
            }
            Error::EmptyImport(p) => write!(f, "no variables found in {p}"),
            Error::ReadFile { path, source } => write!(f, "{path}: {source}"),
            Error::Keychain(e) => write!(f, "keychain error: {e}"),
            Error::Io(e) => write!(f, "{e}"),
            Error::Exec { program, source } => write!(f, "cannot execute {program:?}: {source}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<security_framework::base::Error> for Error {
    fn from(e: security_framework::base::Error) -> Self {
        Error::Keychain(e)
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_style_errors_exit_2() {
        assert_eq!(Error::MissingEnvironment.exit_code(), 2);
        assert_eq!(Error::InvalidKey("A=B".into()).exit_code(), 2);
    }

    #[test]
    fn missing_command_exits_127_and_permission_denied_exits_126() {
        let enoent = Error::Exec {
            program: "nope".into(),
            source: std::io::Error::from(std::io::ErrorKind::NotFound),
        };
        assert_eq!(enoent.exit_code(), 127);
        let denied = Error::Exec {
            program: "nope".into(),
            source: std::io::Error::from(std::io::ErrorKind::PermissionDenied),
        };
        assert_eq!(denied.exit_code(), 126);
    }

    #[test]
    fn display_never_leaks_a_value() {
        let e = Error::InvalidKey("BAD=KEY".into());
        assert!(e.to_string().contains("BAD=KEY"));
        assert!(!e.to_string().contains("secret"));
    }
}
