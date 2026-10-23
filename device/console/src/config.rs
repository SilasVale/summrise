//! Command line and startup configuration.
//!
//! The console has exactly three facts to be told: where the operator's secret is, where the grant
//! signing key is, and where the registry lives. A missing secret file is a startup error naming the
//! flag that named it, because a console that starts without a usable credential fails open in a way
//! nobody notices until the first request.

use std::fmt;
use std::fs;
use std::io;
use std::net::{IpAddr, Ipv4Addr};
use std::path::{Path, PathBuf};

use ed25519_dalek::SigningKey;

use crate::hex;

/// The flag holding the operator's 32-byte token.
pub const OPERATOR_TOKEN_FLAG: &str = "--operator-token-file";
/// The flag holding the grant key's 32-byte Ed25519 seed.
pub const GRANT_KEY_FLAG: &str = "--grant-key-file";
/// The flag naming the directory whose `devices.json` is the registry.
pub const DATA_FLAG: &str = "--data";
/// The flag naming the address to listen on.
pub const BIND_FLAG: &str = "--bind";
/// The flag naming the label this console reports on `/health`.
pub const VERSION_FLAG: &str = "--version";
/// The flag naming the listening port.
pub const PORT_FLAG: &str = "--port";

/// The port the console serves on when `--port` is not given.
pub const DEFAULT_PORT: u16 = 18_082;

/// The address the console binds when `--bind` is not given: loopback, so an operator has to ask
/// for the network explicitly. The TLS-terminator deployment passes `--bind 0.0.0.0`.
pub const DEFAULT_BIND: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

/// Everything the service needs to start.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    /// The file holding the operator token: 64 hex characters.
    pub operator_token_file: PathBuf,
    /// The file holding the grant key's 32-byte Ed25519 seed, mode 0600.
    pub grant_key_file: PathBuf,
    /// The data directory; `<data>/devices.json` is the device registry.
    pub data: PathBuf,
    /// The address to listen on. Loopback unless the operator names another with `--bind`.
    pub bind: IpAddr,
    /// The TCP port to listen on.
    pub port: u16,
    /// The build label this console reports on `/health`.
    ///
    /// A console is deployed by replacing a binary, so "which build is running?" used to be
    /// unanswerable without a file's timestamp. The deploy passes the label; a bare start reports the
    /// crate version, which is honest about being a development build.
    pub version: String,
}

/// Why a command line could not produce a [`Config`].
///
/// Every variant is data, not a sentence: the entry point prints it, and tests match the flag
/// spelling rather than English.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConfigError {
    /// A required flag was not given.
    Missing(&'static str),
    /// A flag had no value after it.
    MissingValue(&'static str),
    /// A value was given but is not the type the flag needs.
    Invalid {
        /// The flag's spelling.
        option: &'static str,
        /// The value as given. None of these flags is a secret; the secret files hold theirs.
        value: String,
        /// What the flag would have accepted.
        expected: &'static str,
    },
    /// An argument that is not one of the flags.
    UnknownArgument(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::Missing(option) => write!(f, "missing {option}"),
            ConfigError::MissingValue(option) => write!(f, "{option} needs a value"),
            ConfigError::Invalid {
                option,
                value,
                expected,
            } => write!(f, "{option} expects {expected}, got '{value}'"),
            ConfigError::UnknownArgument(argument) => write!(f, "unknown argument '{argument}'"),
        }
    }
}

impl std::error::Error for ConfigError {}

impl Config {
    /// Build the configuration from the process command line.
    pub fn from_args() -> Result<Config, ConfigError> {
        Config::parse(std::env::args_os().skip(1))
    }

    /// The testable half of [`Config::from_args`]: an argument list.
    pub fn parse<I>(args: I) -> Result<Config, ConfigError>
    where
        I: IntoIterator<Item = std::ffi::OsString>,
    {
        let mut operator_token_file = None;
        let mut grant_key_file = None;
        let mut data = None;
        let mut bind = None;
        let mut port = None;
        let mut version = None;

        let mut arguments = args.into_iter();
        while let Some(argument) = arguments.next() {
            let flag = argument.to_str().and_then(Flag::parse).ok_or_else(|| {
                ConfigError::UnknownArgument(argument.to_string_lossy().into_owned())
            })?;
            let value = arguments
                .next()
                .ok_or(ConfigError::MissingValue(flag.name()))?;
            match flag {
                Flag::OperatorToken => operator_token_file = Some(PathBuf::from(value)),
                Flag::GrantKey => grant_key_file = Some(PathBuf::from(value)),
                Flag::Data => data = Some(PathBuf::from(value)),
                Flag::Bind => {
                    let text = value.to_string_lossy();
                    bind = Some(text.parse::<IpAddr>().map_err(|_| ConfigError::Invalid {
                        option: BIND_FLAG,
                        value: text.into_owned(),
                        expected: "an IP address (127.0.0.1, 0.0.0.0, ::1, ...)",
                    })?);
                }
                Flag::Version => version = Some(value.to_string_lossy().into_owned()),
                Flag::Port => {
                    let text = value.to_string_lossy();
                    port = Some(text.parse::<u16>().map_err(|_| ConfigError::Invalid {
                        option: PORT_FLAG,
                        value: text.into_owned(),
                        expected: "a TCP port (0..=65535)",
                    })?);
                }
            }
        }

        Ok(Config {
            operator_token_file: operator_token_file
                .ok_or(ConfigError::Missing(OPERATOR_TOKEN_FLAG))?,
            grant_key_file: grant_key_file.ok_or(ConfigError::Missing(GRANT_KEY_FLAG))?,
            data: data.ok_or(ConfigError::Missing(DATA_FLAG))?,
            bind: bind.unwrap_or(DEFAULT_BIND),
            port: port.unwrap_or(DEFAULT_PORT),
            version: version.unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_owned()),
        })
    }
}

enum Flag {
    Version,
    OperatorToken,
    GrantKey,
    Data,
    Bind,
    Port,
}

impl Flag {
    fn parse(argument: &str) -> Option<Flag> {
        match argument {
            VERSION_FLAG => Some(Flag::Version),
            OPERATOR_TOKEN_FLAG => Some(Flag::OperatorToken),
            GRANT_KEY_FLAG => Some(Flag::GrantKey),
            DATA_FLAG => Some(Flag::Data),
            BIND_FLAG => Some(Flag::Bind),
            PORT_FLAG => Some(Flag::Port),
            _ => None,
        }
    }

    fn name(&self) -> &'static str {
        match self {
            Flag::OperatorToken => OPERATOR_TOKEN_FLAG,
            Flag::GrantKey => GRANT_KEY_FLAG,
            Flag::Data => DATA_FLAG,
            Flag::Bind => BIND_FLAG,
            Flag::Port => PORT_FLAG,
            Flag::Version => VERSION_FLAG,
        }
    }
}

/// Why the service could not start.
///
/// The operator token and grant key are fatal: there is no development mode in which the console
/// invents one, because a service that starts with an unusable credential is the failure this
/// service exists to prevent.
#[derive(Debug)]
pub enum StartupError {
    /// A secret file could not be read.
    Unreadable {
        /// The flag that named the file.
        flag: &'static str,
        /// The path as given.
        path: PathBuf,
        /// The OS error.
        source: io::Error,
    },
    /// A secret file did not hold a 32-byte hex secret.
    BadSecret {
        /// The flag that named the file.
        flag: &'static str,
        /// The path as given.
        path: PathBuf,
        /// What the file must contain.
        expected: &'static str,
    },
    /// A secret file is readable by someone other than its owner.
    InsecurePermissions {
        /// What named the file: the flag, or the data file's role.
        name: &'static str,
        /// The path as given.
        path: PathBuf,
        /// The mode found, with the file-type bits masked off.
        mode: u32,
    },
    /// The data directory could not be created.
    DataDirectory {
        /// The directory as given.
        path: PathBuf,
        /// The OS error.
        source: io::Error,
    },
    /// The registry on disk exists but is not one this service wrote.
    Registry {
        /// The registry file.
        path: PathBuf,
        /// What is wrong with it.
        reason: String,
    },
    /// The enrollment-code file on disk exists but is not one this service wrote.
    EnrollmentCodes {
        /// The codes file.
        path: PathBuf,
        /// What is wrong with it.
        reason: String,
    },
}

impl fmt::Display for StartupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StartupError::Unreadable { flag, path, source } => {
                write!(f, "{flag}: cannot read {}: {source}", path.display())
            }
            StartupError::BadSecret {
                flag,
                path,
                expected,
            } => write!(f, "{flag}: {} must contain {expected}", path.display()),
            StartupError::InsecurePermissions { name, path, mode } => write!(
                f,
                "{name}: {} must be mode 0600, found {mode:04o}",
                path.display()
            ),
            StartupError::DataDirectory { path, source } => {
                write!(f, "{DATA_FLAG}: cannot create {}: {source}", path.display())
            }
            StartupError::Registry { path, reason } => {
                write!(f, "device registry {}: {reason}", path.display())
            }
            StartupError::EnrollmentCodes { path, reason } => {
                write!(f, "enrollment codes {}: {reason}", path.display())
            }
        }
    }
}

impl std::error::Error for StartupError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            StartupError::Unreadable { source, .. } => Some(source),
            StartupError::DataDirectory { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Read the operator token: 32 bytes of hex, with the trailing newline an editor or installer
/// leaves behind. The file must be private: whoever can read it can mint sessions and grants.
pub(crate) fn load_operator_token(path: &Path) -> Result<[u8; 32], StartupError> {
    check_private_file(OPERATOR_TOKEN_FLAG, path)?;
    parse_secret(OPERATOR_TOKEN_FLAG, path)
}

/// Read the grant key's seed. The file must be private: a grant key another account can read is a
/// key that can mint grants for that account.
pub(crate) fn load_grant_key(path: &Path) -> Result<SigningKey, StartupError> {
    check_private_file(GRANT_KEY_FLAG, path)?;
    let seed = parse_secret(GRANT_KEY_FLAG, path)?;
    Ok(SigningKey::from_bytes(&seed))
}

fn parse_secret(flag: &'static str, path: &Path) -> Result<[u8; 32], StartupError> {
    let bytes = fs::read(path).map_err(|source| StartupError::Unreadable {
        flag,
        path: path.to_path_buf(),
        source,
    })?;
    let text = std::str::from_utf8(&bytes).map_err(|_| StartupError::BadSecret {
        flag,
        path: path.to_path_buf(),
        expected: "64 hex characters (a 32-byte secret)",
    })?;

    hex::decode_32(text.trim()).ok_or_else(|| StartupError::BadSecret {
        flag,
        path: path.to_path_buf(),
        expected: "64 hex characters (a 32-byte secret)",
    })
}

/// Refuse a private file whose mode is not exactly 0600.
///
/// A missing file is not this check's business: every caller reads it next and reports that failure
/// with the name it was given by. The data files pass their role ("device registry") rather than a
/// flag, which is why the message names both the role and the path.
#[cfg(unix)]
pub(crate) fn check_private_file(name: &'static str, path: &Path) -> Result<(), StartupError> {
    use std::os::unix::fs::PermissionsExt;

    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(source) => {
            return Err(StartupError::Unreadable {
                flag: name,
                path: path.to_path_buf(),
                source,
            })
        }
    };
    let mode = metadata.permissions().mode() & 0o777;
    if mode != 0o600 {
        return Err(StartupError::InsecurePermissions {
            name,
            path: path.to_path_buf(),
            mode,
        });
    }
    Ok(())
}

#[cfg(not(unix))]
pub(crate) fn check_private_file(_name: &'static str, _path: &Path) -> Result<(), StartupError> {
    // Windows has no POSIX mode to inspect. The ACL on the file and its directory is the install's
    // job; the process still refuses a file it cannot read, and never prints it.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn the_port_defaults_and_every_other_flag_is_required() {
        let config = Config::parse(args(&[
            OPERATOR_TOKEN_FLAG,
            "op.token",
            GRANT_KEY_FLAG,
            "grant.key",
            DATA_FLAG,
            "data",
        ]))
        .expect("the three required flags are enough");

        assert_eq!(config.operator_token_file, PathBuf::from("op.token"));
        assert_eq!(config.grant_key_file, PathBuf::from("grant.key"));
        assert_eq!(config.data, PathBuf::from("data"));
        assert_eq!(config.bind, DEFAULT_BIND);
        assert_eq!(config.port, DEFAULT_PORT);
    }

    #[test]
    fn a_missing_flag_is_named() {
        let error = Config::parse(args(&[GRANT_KEY_FLAG, "grant.key", DATA_FLAG, "data"]))
            .expect_err("the operator token flag is required");

        assert_eq!(error, ConfigError::Missing(OPERATOR_TOKEN_FLAG));
        assert!(error.to_string().contains(OPERATOR_TOKEN_FLAG));
    }

    #[test]
    fn the_listener_defaults_to_loopback_and_binds_what_the_flag_names() {
        let config = Config::parse(args(&[
            OPERATOR_TOKEN_FLAG,
            "op.token",
            GRANT_KEY_FLAG,
            "grant.key",
            DATA_FLAG,
            "data",
        ]))
        .expect("the three required flags are enough");

        assert_eq!(config.bind, DEFAULT_BIND);
        assert_eq!(DEFAULT_BIND, IpAddr::V4(Ipv4Addr::LOCALHOST));

        let exposed = Config::parse(args(&[
            OPERATOR_TOKEN_FLAG,
            "op.token",
            GRANT_KEY_FLAG,
            "grant.key",
            DATA_FLAG,
            "data",
            BIND_FLAG,
            "0.0.0.0",
        ]))
        .expect("an explicit bind address is accepted");

        assert_eq!(exposed.bind, IpAddr::V4(Ipv4Addr::UNSPECIFIED));
    }

    #[test]
    fn a_bad_bind_address_is_an_error_not_a_default() {
        let error = Config::parse(args(&[
            OPERATOR_TOKEN_FLAG,
            "op.token",
            GRANT_KEY_FLAG,
            "grant.key",
            DATA_FLAG,
            "data",
            BIND_FLAG,
            "not-an-address",
        ]))
        .expect_err("a bind address must parse");

        assert!(matches!(error, ConfigError::Invalid { option, .. } if option == BIND_FLAG));
    }

    #[test]
    fn a_bad_port_is_an_error_not_a_default() {
        let error = Config::parse(args(&[
            OPERATOR_TOKEN_FLAG,
            "op.token",
            GRANT_KEY_FLAG,
            "grant.key",
            DATA_FLAG,
            "data",
            PORT_FLAG,
            "18080x",
        ]))
        .expect_err("a port must parse");

        assert!(matches!(error, ConfigError::Invalid { option, .. } if option == PORT_FLAG));
    }
}
