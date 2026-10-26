//! Command-line and environment configuration for the app.
//!
//! The launcher starts the app with `--install`, `--data`, `--port`, `--release-version`,
//! `--release-sha` and `--release-channel`; an operator may add `--token-file`, `--files-root`,
//! `--console-url`/`--console-token-file` or `--mesh-broker`/`--mesh-relay`/`--mesh-stun`. A human
//! starting it by hand may set most facts through the environment. Flags win over the
//! environment, one spelling for each fact, and a missing install or data directory is a typed
//! error rather than a panic: a service that panics on a bad command line leaves nothing in the
//! event log that a reader can act on.
//!
//! The three release facts are the launcher's reading of the signed baseline sidecar, so a device
//! updated to a canary build reports `canary` in its state document and `/api/health` rather than
//! labelling every build stable.

use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::path::PathBuf;

use serde::Deserialize;
use summrise_release::Channel;

/// Port the app serves on when neither `--port` nor `SUMMRISE_PORT` is set.
pub const DEFAULT_PORT: u16 = 18_080;

/// The flag naming the console's base URL. Absent means the console link is off.
pub const CONSOLE_URL_FLAG: &str = "--console-url";

/// The flag naming the file the console's registration response was saved to.
pub const CONSOLE_TOKEN_FLAG: &str = "--console-token-file";

/// The flag naming the mesh rendezvous broker's origin. Absent means the mesh link is off.
pub const MESH_BROKER_FLAG: &str = "--mesh-broker";

/// The flag naming the mesh relay floor's address.
///
/// A relay without a broker is a configuration error rather than a silently ignored fact: the
/// relay is only reachable by a peer the rendezvous introduced, so the pair is useless alone.
pub const MESH_RELAY_FLAG: &str = "--mesh-relay";

/// The flag naming the mesh STUN reflector's address.
///
/// A STUN reflector without a broker is the same kind of half-fact as a relay without one: the
/// reflection only names the address this device publishes in the rendezvous, so there is nowhere
/// to publish it.
pub const MESH_STUN_FLAG: &str = "--mesh-stun";

/// The file `--install-service` / `run` persist the remote link into, under `<data>`.
///
/// The SCM starts the app from a fixed argv and its environment is not settable from
/// `--install-service`, so a supervised install must carry the console's facts (url, token file)
/// and the mesh's (broker, relay, STUN reflector) in a file the app can find from `--data`. The
/// name is the one the file has always had; it now carries both. The launcher writes it with mode
/// 0600 (Unix); the app refuses a looser one.
pub const CONSOLE_FILE: &str = "console.json";

/// The release facts the running build was started from (fix design F6).
///
/// The launcher passes these on the child's argv from the signed baseline sidecar. They are the
/// single answer to "which release is this process?" — the state document and `/api/health` report
/// them, rather than each re-deriving a version from its own compile time. The channel is part of
/// the signed release identity: a canary sidecar must not be recorded as a stable install.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    /// The release version, e.g. `2.0.3`.
    pub version: String,
    /// Lower-hex SHA-256 of the app binary the launcher installed.
    pub sha256: String,
    /// The release channel the booted sidecar names: `stable` or `canary`.
    pub channel: String,
}

impl Release {
    /// The facts of a standalone development start: this build's crate version, no digest because
    /// nothing verified a downloaded pair, and the stable channel because nothing opted in.
    pub fn standalone() -> Release {
        Release {
            version: env!("CARGO_PKG_VERSION").to_owned(),
            sha256: String::new(),
            channel: Channel::Stable.as_str().to_owned(),
        }
    }
}

/// Everything the app needs to start: where it lives, where it writes, and where the token is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    /// The install root the launcher owns (design §4.1).
    pub install: PathBuf,
    /// The data directory `<data>/state.json` is written under.
    pub data: PathBuf,
    /// The loopback port the HTTP API serves on.
    pub port: u16,
    /// The file holding the install token; the launcher does not pass it, so it defaults under
    /// `<data>`.
    pub token_file: PathBuf,
    /// The root the `files` capability is sandboxed to; defaults to `<data>/workspace`.
    pub files_root: PathBuf,
    /// `--release-version` from the launcher, or `None` for a standalone development start.
    pub release_version: Option<String>,
    /// `--release-sha` from the launcher, or `None` for a standalone development start.
    pub release_sha: Option<String>,
    /// `--release-channel` from the launcher, or `None` for a standalone development start.
    pub release_channel: Option<Channel>,
    /// Whether the process that started this one supervises it (`SUMMRISE_SUPERVISED=1`, set by the
    /// launcher for its child). Only a supervised app may exit after a panel enrollment: a process an
    /// operator started by hand would not come back.
    pub supervised: bool,
    /// The console's base URL, when the console link is on.
    ///
    /// Both console facts are optional, and they are all-or-nothing: a URL without the token file
    /// (or the reverse) is a configuration error, not a silently disabled feature.
    pub console_url: Option<String>,
    /// The file holding the console registration response (`id` and `token`).
    pub console_token_file: Option<PathBuf>,
    /// The mesh rendezvous broker's origin, when the mesh link is on.
    ///
    /// It is kept as the operator wrote it and validated where the link is built: the mesh knows
    /// what a broker URL is, and this type would only repeat it.
    pub mesh_broker: Option<String>,
    /// The mesh relay floor's address, when one is configured.
    ///
    /// `None` with a broker is a direct-only mesh: a connect that needs the relay fails with a
    /// typed error instead of waiting for a floor that is not there.
    pub mesh_relay: Option<String>,
    /// The mesh STUN reflector's address, when one is configured.
    ///
    /// It is kept as the operator wrote it and parsed where the link is built, like the relay.
    /// `None` publishes the bound address only.
    pub mesh_stun: Option<String>,
}

/// Why a command line could not produce a [`Config`].
///
/// Every variant is data: the entry point prints it, and the tests match on it instead of on
/// English.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConfigError {
    /// Neither the flag nor its environment variable was given.
    Missing {
        /// The flag's spelling, for the message.
        option: &'static str,
        /// The environment variable that would have supplied it.
        env: &'static str,
    },
    /// A value was given but is not the type the flag needs.
    Invalid {
        /// The flag's spelling, for the message.
        option: &'static str,
        /// The value as given. A port is not a secret; no other flag's value is ever printed.
        value: String,
        /// What the flag would have accepted.
        expected: &'static str,
    },
    /// An argument that is not one of the flags.
    UnknownArgument(String),
    /// A flag with no value after it.
    MissingValue(&'static str),
    /// The launcher's persisted console link could not be used.
    ConsoleFile {
        /// `<data>/console.json`.
        path: PathBuf,
        /// Why it was refused (an I/O or parse error, an insecure mode, a missing field).
        detail: String,
    },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::Missing { option, env } => {
                write!(f, "missing {option} (or the {env} environment variable)")
            }
            ConfigError::Invalid {
                option,
                value,
                expected,
            } => write!(f, "{option} expects {expected}, got '{value}'"),
            ConfigError::UnknownArgument(argument) => write!(f, "unknown argument '{argument}'"),
            ConfigError::MissingValue(option) => write!(f, "{option} needs a value"),
            ConfigError::ConsoleFile { path, detail } => {
                write!(
                    f,
                    "cannot use the console link {}: {detail}",
                    path.display()
                )
            }
        }
    }
}

impl std::error::Error for ConfigError {}

impl Config {
    /// Build the configuration from the process command line and environment.
    ///
    /// The persisted remote link is the last source consulted: an explicit flag or a `SUMMRISE_*`
    /// variable is an operator's direct instruction and must win over a file a previous install
    /// wrote, per feature.
    pub fn from_args() -> Result<Config, ConfigError> {
        let mut config = Config::parse(std::env::args_os().skip(1), |name| std::env::var_os(name))?;
        config.resolve_console_file()?;
        Ok(config)
    }

    /// The testable half of [`Config::from_args`]: an argument list and an environment lookup.
    ///
    /// `env` is a function rather than a snapshot so the caller decides which facts exist; the
    /// process environment is one caller and a `HashMap` in a test is another.
    pub fn parse<I>(args: I, env: impl Fn(&str) -> Option<OsString>) -> Result<Config, ConfigError>
    where
        I: IntoIterator<Item = OsString>,
    {
        let mut install = None;
        let mut data = None;
        let mut port = None;
        let mut token_file = None;
        let mut files_root = None;
        let mut release_version = None;
        let mut release_sha = None;
        let mut release_channel = None;
        let mut console_url: Option<String> = None;
        let mut console_token_file: Option<PathBuf> = None;
        let mut mesh_broker: Option<String> = None;
        let mut mesh_relay: Option<String> = None;
        let mut mesh_stun: Option<String> = None;

        // The launcher's own mark, not an inference from the argv: a hand-started process given the
        // launcher's release flags is still not supervised, and must not exit after an enrollment.
        let supervised = env("SUMMRISE_SUPERVISED")
            .and_then(|value| value.to_str().map(|text| text == "1"))
            .unwrap_or(false);

        let mut arguments = args.into_iter();
        while let Some(argument) = arguments.next() {
            let flag = argument.to_str().and_then(Flag::parse).ok_or_else(|| {
                ConfigError::UnknownArgument(argument.to_string_lossy().into_owned())
            })?;
            let value = arguments
                .next()
                .ok_or(ConfigError::MissingValue(flag.name()))?;
            match flag {
                Flag::Install => install = Some(PathBuf::from(value)),
                Flag::Data => data = Some(PathBuf::from(value)),
                Flag::Port => port = Some(parse_port(&value)?),
                Flag::TokenFile => token_file = Some(PathBuf::from(value)),
                Flag::FilesRoot => files_root = Some(PathBuf::from(value)),
                Flag::ReleaseVersion => release_version = Some(text_value(&value, flag.name())?),
                Flag::ReleaseSha => release_sha = Some(text_value(&value, flag.name())?),
                Flag::ReleaseChannel => release_channel = Some(parse_channel(&value)?),
                Flag::ConsoleUrl => console_url = Some(text_value(&value, flag.name())?),
                Flag::ConsoleToken => console_token_file = Some(PathBuf::from(value)),
                Flag::MeshBroker => mesh_broker = Some(text_value(&value, flag.name())?),
                Flag::MeshRelay => mesh_relay = Some(text_value(&value, flag.name())?),
                Flag::MeshStun => mesh_stun = Some(text_value(&value, flag.name())?),
            }
        }

        let install = install
            .or_else(|| env("SUMMRISE_INSTALL").map(PathBuf::from))
            .ok_or(ConfigError::Missing {
                option: "--install",
                env: "SUMMRISE_INSTALL",
            })?;
        let data = data
            .or_else(|| env("SUMMRISE_DATA").map(PathBuf::from))
            .ok_or(ConfigError::Missing {
                option: "--data",
                env: "SUMMRISE_DATA",
            })?;
        let port = match port {
            Some(port) => port,
            None => match env("SUMMRISE_PORT") {
                Some(value) => parse_port(&value)?,
                None => DEFAULT_PORT,
            },
        };
        let token_file = token_file
            .or_else(|| env("SUMMRISE_TOKEN_FILE").map(PathBuf::from))
            .unwrap_or_else(|| data.join("token"));
        // The default is inside the data directory: one directory the uninstaller and the backup
        // already know about. An operator can move the sandbox with --files-root, never widen it.
        let files_root = files_root
            .or_else(|| env("SUMMRISE_FILES_ROOT").map(PathBuf::from))
            .unwrap_or_else(|| data.join("workspace"));

        let console_url = console_url.or_else(|| {
            env("SUMMRISE_CONSOLE_URL").and_then(|value| value.to_str().map(str::to_owned))
        });
        let console_token_file =
            console_token_file.or_else(|| env("SUMMRISE_CONSOLE_TOKEN_FILE").map(PathBuf::from));
        match (&console_url, &console_token_file) {
            (None, None) => {}
            (Some(_), Some(_)) => {}
            (None, Some(_)) => {
                return Err(ConfigError::Missing {
                    option: CONSOLE_URL_FLAG,
                    env: "SUMMRISE_CONSOLE_URL",
                })
            }
            (Some(_), None) => {
                return Err(ConfigError::Missing {
                    option: CONSOLE_TOKEN_FLAG,
                    env: "SUMMRISE_CONSOLE_TOKEN_FILE",
                })
            }
        }

        let mesh_broker = mesh_broker.or_else(|| {
            env("SUMMRISE_MESH_BROKER").and_then(|value| value.to_str().map(str::to_owned))
        });
        let mesh_relay = mesh_relay.or_else(|| {
            env("SUMMRISE_MESH_RELAY").and_then(|value| value.to_str().map(str::to_owned))
        });
        let mesh_stun = mesh_stun.or_else(|| {
            env("SUMMRISE_MESH_STUN").and_then(|value| value.to_str().map(str::to_owned))
        });
        if (mesh_relay.is_some() || mesh_stun.is_some()) && mesh_broker.is_none() {
            // A relay is the floor a rendezvous fallback lands on, and a STUN reflector only
            // names the address this device publishes there. Without a broker neither could ever
            // be used: the operator named half of a feature, and serving as if they had named
            // none is the silent failure this refuses.
            return Err(ConfigError::Missing {
                option: MESH_BROKER_FLAG,
                env: "SUMMRISE_MESH_BROKER",
            });
        }

        Ok(Config {
            install,
            data,
            port,
            token_file,
            files_root,
            release_version,
            release_sha,
            release_channel,
            supervised,
            console_url,
            console_token_file,
            mesh_broker,
            mesh_relay,
            mesh_stun,
        })
    }

    /// Read `<data>/console.json` for every feature its own flag and environment did not name.
    ///
    /// Absent is the normal standalone case and a no-op. Anything else is loud: a link that parses
    /// but names a world-readable credential file, half a console pair, a relay with no broker, or
    /// no fact at all, is a device that would otherwise serve with a remote path silently closed —
    /// the failure mode the whole file exists to remove. A feature named by a flag (or by the
    /// environment) is left alone, and the file's facts for the *other* feature still apply.
    pub fn resolve_console_file(&mut self) -> Result<(), ConfigError> {
        // A feature the operator named on the command line (or in the environment) is an explicit
        // instruction and the file is not consulted for it. The file is still read when the
        // *other* feature needs it: one install step persists both, and naming one does not name
        // the other.
        let console_named = self.console_url.is_some() || self.console_token_file.is_some();
        let mesh_named = self.mesh_broker.is_some();
        if console_named && mesh_named {
            return Ok(());
        }
        let path = self.data.join(CONSOLE_FILE);
        let metadata = match fs::metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => {
                return Err(ConfigError::ConsoleFile {
                    path,
                    detail: error.to_string(),
                })
            }
        };
        if !metadata.is_file() {
            return Err(ConfigError::ConsoleFile {
                path,
                detail: "not a regular file".to_owned(),
            });
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = metadata.permissions().mode() & 0o777;
            if mode & 0o077 != 0 {
                return Err(ConfigError::ConsoleFile {
                    path,
                    detail: format!(
                        "mode {mode:04o}; it names the console credential file and must be 0600"
                    ),
                });
            }
        }
        let text = fs::read_to_string(&path).map_err(|error| ConfigError::ConsoleFile {
            path: path.clone(),
            detail: error.to_string(),
        })?;
        let link: ConsoleLinkFile =
            serde_json::from_str(&text).map_err(|error| ConfigError::ConsoleFile {
                path: path.clone(),
                detail: error.to_string(),
            })?;

        if !console_named {
            match (&link.url, &link.token_file) {
                (None, None) => {}
                (Some(url), Some(token_file)) => {
                    if url.trim().is_empty() {
                        return Err(ConfigError::ConsoleFile {
                            path: path.clone(),
                            detail: "the url is empty".to_owned(),
                        });
                    }
                    if token_file.as_os_str().is_empty() {
                        return Err(ConfigError::ConsoleFile {
                            path: path.clone(),
                            detail: "the token_file is empty".to_owned(),
                        });
                    }
                    self.console_url = Some(url.clone());
                    self.console_token_file = Some(token_file.clone());
                }
                _ => {
                    return Err(ConfigError::ConsoleFile {
                        path: path.clone(),
                        detail: "names one console fact without the other".to_owned(),
                    })
                }
            }
        }

        if !mesh_named {
            match (&link.mesh_broker, &link.mesh_relay, &link.mesh_stun) {
                (None, None, None) => {}
                (Some(broker), relay, stun) => {
                    if broker.trim().is_empty() {
                        return Err(ConfigError::ConsoleFile {
                            path: path.clone(),
                            detail: "the mesh_broker is empty".to_owned(),
                        });
                    }
                    if relay
                        .as_deref()
                        .is_some_and(|relay| relay.trim().is_empty())
                    {
                        return Err(ConfigError::ConsoleFile {
                            path: path.clone(),
                            detail: "the mesh_relay is empty".to_owned(),
                        });
                    }
                    if stun.as_deref().is_some_and(|stun| stun.trim().is_empty()) {
                        return Err(ConfigError::ConsoleFile {
                            path: path.clone(),
                            detail: "the mesh_stun is empty".to_owned(),
                        });
                    }
                    self.mesh_broker = Some(broker.clone());
                    self.mesh_relay = relay.clone();
                    self.mesh_stun = stun.clone();
                }
                // The same rule the command line enforces: a relay is the floor a rendezvous
                // leads to, and a STUN reflector only names the address the rendezvous
                // publishes. Neither can stand without a broker.
                (None, _, _) => {
                    return Err(ConfigError::ConsoleFile {
                        path: path.clone(),
                        detail: format!(
                            "names a mesh_relay or mesh_stun but no mesh_broker \
                             ({MESH_BROKER_FLAG})"
                        ),
                    })
                }
            }
        }

        if link.url.is_none()
            && link.token_file.is_none()
            && link.mesh_broker.is_none()
            && link.mesh_relay.is_none()
            && link.mesh_stun.is_none()
        {
            return Err(ConfigError::ConsoleFile {
                path,
                detail: "names neither the console (url, token_file) nor the mesh (mesh_broker, \
                         mesh_relay, mesh_stun)"
                    .to_owned(),
            });
        }
        Ok(())
    }

    /// The release facts this process was started with.
    ///
    /// The launcher always passes all three flags; a standalone start passes none and gets the crate
    /// version with an empty digest and the stable channel, because a development process has no
    /// verified release pair to claim and has opted into nothing.
    pub fn release(&self) -> Release {
        let standalone = Release::standalone();
        Release {
            version: self.release_version.clone().unwrap_or(standalone.version),
            sha256: self.release_sha.clone().unwrap_or(standalone.sha256),
            channel: self
                .release_channel
                .unwrap_or(Channel::Stable)
                .as_str()
                .to_owned(),
        }
    }
}

/// The shape the launcher persists at `<data>/console.json`.
///
/// The file keeps its name and now carries both remote facts a supervised install cannot put on
/// the service's argv: the console's and the mesh's. Every field is optional, because an install
/// may have one, the other, or both, and the launcher omits what it did not write.
///
/// `deny_unknown_fields`: a field this build does not understand is a link written by a newer
/// launcher, and silently ignoring it would arm a channel with a fact the operator did not choose.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConsoleLinkFile {
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    token_file: Option<PathBuf>,
    #[serde(default)]
    mesh_broker: Option<String>,
    #[serde(default)]
    mesh_relay: Option<String>,
    #[serde(default)]
    mesh_stun: Option<String>,
}

/// The thirteen flags the app accepts, so an unknown argument can be refused by name.
#[derive(Clone, Copy)]
enum Flag {
    Install,
    Data,
    Port,
    TokenFile,
    FilesRoot,
    ReleaseVersion,
    ReleaseSha,
    ReleaseChannel,
    ConsoleUrl,
    ConsoleToken,
    MeshBroker,
    MeshRelay,
    MeshStun,
}

impl Flag {
    fn parse(argument: &str) -> Option<Flag> {
        match argument {
            "--install" => Some(Flag::Install),
            "--data" => Some(Flag::Data),
            "--port" => Some(Flag::Port),
            "--token-file" => Some(Flag::TokenFile),
            "--files-root" => Some(Flag::FilesRoot),
            "--release-version" => Some(Flag::ReleaseVersion),
            "--release-sha" => Some(Flag::ReleaseSha),
            "--release-channel" => Some(Flag::ReleaseChannel),
            CONSOLE_URL_FLAG => Some(Flag::ConsoleUrl),
            CONSOLE_TOKEN_FLAG => Some(Flag::ConsoleToken),
            MESH_BROKER_FLAG => Some(Flag::MeshBroker),
            MESH_RELAY_FLAG => Some(Flag::MeshRelay),
            MESH_STUN_FLAG => Some(Flag::MeshStun),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Flag::Install => "--install",
            Flag::Data => "--data",
            Flag::Port => "--port",
            Flag::TokenFile => "--token-file",
            Flag::FilesRoot => "--files-root",
            Flag::ReleaseVersion => "--release-version",
            Flag::ReleaseSha => "--release-sha",
            Flag::ReleaseChannel => "--release-channel",
            Flag::ConsoleUrl => CONSOLE_URL_FLAG,
            Flag::ConsoleToken => CONSOLE_TOKEN_FLAG,
            Flag::MeshBroker => MESH_BROKER_FLAG,
            Flag::MeshRelay => MESH_RELAY_FLAG,
            Flag::MeshStun => MESH_STUN_FLAG,
        }
    }
}

fn parse_port(value: &OsString) -> Result<u16, ConfigError> {
    let text = value.to_str().ok_or_else(|| ConfigError::Invalid {
        option: "--port",
        value: value.to_string_lossy().into_owned(),
        expected: "a number between 0 and 65535",
    })?;
    text.parse::<u16>().map_err(|_| ConfigError::Invalid {
        option: "--port",
        value: text.to_owned(),
        expected: "a number between 0 and 65535",
    })
}

fn text_value(value: &OsString, option: &'static str) -> Result<String, ConfigError> {
    value
        .to_str()
        .map(str::to_owned)
        .ok_or_else(|| ConfigError::Invalid {
            option,
            value: value.to_string_lossy().into_owned(),
            expected: "UTF-8 text",
        })
}

/// Parse a release channel by the two names the signed sidecar may carry.
///
/// The channel is part of the signed release identity, so an unknown spelling is a configuration
/// error naming the flag, not a value the document records and the update decision later trips on.
fn parse_channel(value: &OsString) -> Result<Channel, ConfigError> {
    let text = value.to_str().ok_or_else(|| ConfigError::Invalid {
        option: "--release-channel",
        value: value.to_string_lossy().into_owned(),
        expected: "stable or canary",
    })?;
    text.parse::<Channel>().map_err(|_| ConfigError::Invalid {
        option: "--release-channel",
        value: text.to_owned(),
        expected: "stable or canary",
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn parse(args: &[&str], env: &[(&str, &str)]) -> Result<Config, ConfigError> {
        let env: HashMap<String, OsString> = env
            .iter()
            .map(|(name, value)| ((*name).to_owned(), OsString::from(*value)))
            .collect();
        Config::parse(
            args.iter().map(|argument| OsString::from(*argument)),
            move |name| env.get(name).cloned(),
        )
    }

    #[test]
    fn all_thirteen_flags_are_parsed() {
        let config = parse(
            &[
                "--install",
                "/opt/summrise",
                "--data",
                "/var/lib/summrise",
                "--port",
                "18081",
                "--token-file",
                "/etc/summrise/token",
                "--files-root",
                "/srv/summrise/workspace",
                "--release-version",
                "2.0.3",
                "--release-sha",
                "b1946ac92492d2347c6235b4d2611184",
                "--release-channel",
                "canary",
                MESH_BROKER_FLAG,
                "http://127.0.0.1:8080",
                MESH_RELAY_FLAG,
                "127.0.0.1:9000",
                MESH_STUN_FLAG,
                "203.0.113.10:3478",
            ],
            &[],
        )
        .expect("a complete command line parses");

        assert_eq!(
            config,
            Config {
                install: PathBuf::from("/opt/summrise"),
                data: PathBuf::from("/var/lib/summrise"),
                port: 18_081,
                token_file: PathBuf::from("/etc/summrise/token"),
                files_root: PathBuf::from("/srv/summrise/workspace"),
                release_version: Some("2.0.3".to_owned()),
                release_sha: Some("b1946ac92492d2347c6235b4d2611184".to_owned()),
                release_channel: Some(Channel::Canary),
                supervised: false,
                console_url: None,
                console_token_file: None,
                mesh_broker: Some("http://127.0.0.1:8080".to_owned()),
                mesh_relay: Some("127.0.0.1:9000".to_owned()),
                mesh_stun: Some("203.0.113.10:3478".to_owned()),
            }
        );
        assert_eq!(
            config.release(),
            Release {
                version: "2.0.3".to_owned(),
                sha256: "b1946ac92492d2347c6235b4d2611184".to_owned(),
                channel: "canary".to_owned(),
            }
        );
    }

    #[test]
    fn the_mesh_link_is_off_when_no_flag_names_a_broker() {
        let config = parse(
            &["--install", "/opt/summrise", "--data", "/var/lib/summrise"],
            &[],
        )
        .expect("install and data are enough");

        assert_eq!(
            (config.mesh_broker, config.mesh_relay, config.mesh_stun),
            (None, None, None),
            "absent flags mean the mesh is not started, exactly as before it existed"
        );
    }

    #[test]
    fn the_mesh_facts_come_from_the_environment_too() {
        let config = parse(
            &["--install", "/opt/summrise", "--data", "/var/lib/summrise"],
            &[
                ("SUMMRISE_MESH_BROKER", "http://10.0.0.5:8080"),
                ("SUMMRISE_MESH_RELAY", "10.0.0.5:9000"),
                ("SUMMRISE_MESH_STUN", "203.0.113.10:3478"),
            ],
        )
        .expect("the environment supplies both facts");

        assert_eq!(
            (
                config.mesh_broker.as_deref(),
                config.mesh_relay.as_deref(),
                config.mesh_stun.as_deref(),
            ),
            (
                Some("http://10.0.0.5:8080"),
                Some("10.0.0.5:9000"),
                Some("203.0.113.10:3478")
            )
        );
    }

    #[test]
    fn a_mesh_broker_without_a_relay_is_a_direct_only_mesh() {
        let config = parse(
            &[
                "--install",
                "/opt/summrise",
                "--data",
                "/var/lib/summrise",
                MESH_BROKER_FLAG,
                "http://127.0.0.1:8080",
            ],
            &[],
        )
        .expect("a broker alone is a complete mesh configuration");

        assert_eq!(config.mesh_broker.as_deref(), Some("http://127.0.0.1:8080"));
        assert_eq!(
            config.mesh_relay, None,
            "no relay means a connect that needs one is a typed failure, not a hang"
        );
    }

    #[test]
    fn a_mesh_relay_without_a_broker_is_refused() {
        let error = parse(
            &[
                "--install",
                "/opt/summrise",
                "--data",
                "/var/lib/summrise",
                MESH_RELAY_FLAG,
                "127.0.0.1:9000",
            ],
            &[],
        )
        .expect_err("a relay with no rendezvous can never be reached");

        assert_eq!(
            error,
            ConfigError::Missing {
                option: MESH_BROKER_FLAG,
                env: "SUMMRISE_MESH_BROKER",
            },
            "the refusal names the flag that would make the relay usable"
        );
    }

    #[test]
    fn a_mesh_stun_without_a_broker_is_refused() {
        let error = parse(
            &[
                "--install",
                "/opt/summrise",
                "--data",
                "/var/lib/summrise",
                MESH_STUN_FLAG,
                "203.0.113.10:3478",
            ],
            &[],
        )
        .expect_err("a reflection with nowhere to publish it is a half-fact");

        assert_eq!(
            error,
            ConfigError::Missing {
                option: MESH_BROKER_FLAG,
                env: "SUMMRISE_MESH_BROKER",
            },
            "the refusal names the flag that would make the reflector useful"
        );
    }

    #[test]
    fn the_release_facts_fall_back_to_the_crate_version_for_a_standalone_start() {
        let config = parse(
            &["--install", "/opt/summrise", "--data", "/var/lib/summrise"],
            &[],
        )
        .expect("install and data are enough");

        assert_eq!(config.release_version, None);
        assert_eq!(config.release_sha, None);
        assert_eq!(config.release_channel, None);
        assert_eq!(
            config.release(),
            Release {
                version: env!("CARGO_PKG_VERSION").to_owned(),
                sha256: String::new(),
                channel: "stable".to_owned(),
            },
            "a development start claims no verified digest and follows stable"
        );
    }

    #[test]
    fn the_launcher_passes_the_release_channel_the_sidecar_names() {
        let config = parse(
            &[
                "--install",
                "/opt/summrise",
                "--data",
                "/var/lib/summrise",
                "--release-channel",
                "canary",
            ],
            &[],
        )
        .expect("the channel flag parses");

        assert_eq!(
            config.release_channel,
            Some(summrise_release::Channel::Canary)
        );
        assert_eq!(
            config.release().channel,
            "canary",
            "the release facts report the channel the launcher booted the sidecar for"
        );
    }

    #[test]
    fn an_unknown_release_channel_is_refused_by_name() {
        let error = parse(
            &[
                "--install",
                "/opt/summrise",
                "--data",
                "/var/lib/summrise",
                "--release-channel",
                "beta",
            ],
            &[],
        )
        .expect_err("an unknown channel is not one this build follows");

        assert!(
            matches!(
                error,
                ConfigError::Invalid {
                    option: "--release-channel",
                    ..
                }
            ),
            "the refusal names the flag: {error}"
        );
    }

    #[test]
    fn the_port_and_token_file_default_from_the_data_directory() {
        let config = parse(
            &["--install", "/opt/summrise", "--data", "/var/lib/summrise"],
            &[],
        )
        .expect("install and data are enough");

        assert_eq!(config.port, DEFAULT_PORT);
        assert_eq!(config.port, 18_080);
        assert_eq!(
            config.token_file,
            PathBuf::from("/var/lib/summrise/token"),
            "the launcher does not pass --token-file, so it must default under <data>"
        );
        assert_eq!(
            config.files_root,
            PathBuf::from("/var/lib/summrise/workspace"),
            "the capability's sandbox roots under <data> unless the operator moves it"
        );
    }

    #[test]
    fn the_files_root_flag_and_environment_are_read() {
        let from_flag = parse(
            &[
                "--install",
                "/opt/summrise",
                "--data",
                "/var/lib/summrise",
                "--files-root",
                "/srv/flag",
            ],
            &[],
        )
        .expect("the flag parses");
        assert_eq!(from_flag.files_root, PathBuf::from("/srv/flag"));

        let from_env = parse(
            &["--install", "/opt/summrise", "--data", "/var/lib/summrise"],
            &[("SUMMRISE_FILES_ROOT", "/srv/env")],
        )
        .expect("the environment parses");
        assert_eq!(from_env.files_root, PathBuf::from("/srv/env"));
    }

    #[test]
    fn the_environment_supplies_every_fact() {
        let config = parse(
            &[],
            &[
                ("SUMMRISE_INSTALL", "/opt/summrise"),
                ("SUMMRISE_DATA", "/var/lib/summrise"),
                ("SUMMRISE_PORT", "18082"),
                ("SUMMRISE_TOKEN_FILE", "/etc/summrise/token"),
                ("SUMMRISE_FILES_ROOT", "/srv/summrise/workspace"),
            ],
        )
        .expect("the environment can configure every field");

        assert_eq!(
            config,
            Config {
                install: PathBuf::from("/opt/summrise"),
                data: PathBuf::from("/var/lib/summrise"),
                port: 18_082,
                token_file: PathBuf::from("/etc/summrise/token"),
                files_root: PathBuf::from("/srv/summrise/workspace"),
                release_version: None,
                release_sha: None,
                release_channel: None,
                supervised: false,
                console_url: None,
                console_token_file: None,
                mesh_broker: None,
                mesh_relay: None,
                mesh_stun: None,
            }
        );
    }

    #[test]
    fn a_flag_wins_over_the_environment() {
        let config = parse(
            &[
                "--install",
                "/flag/install",
                "--port",
                "19000",
                "--files-root",
                "/flag/files",
            ],
            &[
                ("SUMMRISE_INSTALL", "/env/install"),
                ("SUMMRISE_DATA", "/env/data"),
                ("SUMMRISE_PORT", "18082"),
                ("SUMMRISE_FILES_ROOT", "/env/files"),
            ],
        )
        .expect("the command line parses");

        assert_eq!(config.install, PathBuf::from("/flag/install"));
        assert_eq!(config.data, PathBuf::from("/env/data"));
        assert_eq!(config.port, 19_000);
        assert_eq!(config.files_root, PathBuf::from("/flag/files"));
    }

    #[test]
    fn the_launcher_marks_its_child_as_supervised() {
        let args = [
            "--install",
            "/opt/summrise",
            "--data",
            "/var/lib/summrise",
            "--port",
            "18081",
            "--token-file",
            "/etc/summrise/token",
            "--files-root",
            "/srv/summrise/workspace",
        ];
        let plain = parse(&args, &[]).expect("the command line parses");
        assert!(
            !plain.supervised,
            "a hand-started process is not supervised"
        );

        let marked =
            parse(&args, &[("SUMMRISE_SUPERVISED", "1")]).expect("the launcher's mark parses");
        assert!(
            marked.supervised,
            "the launcher's child knows it is supervised"
        );

        let other = parse(&args, &[("SUMMRISE_SUPERVISED", "0")]).expect("any other value parses");
        assert!(!other.supervised, "only 1 is the mark");
    }

    #[test]
    fn the_console_link_is_off_unless_both_flags_are_given() {
        let off = parse(
            &["--install", "/opt/summrise", "--data", "/var/lib/summrise"],
            &[],
        )
        .expect("the console is optional");
        assert_eq!(off.console_url, None);
        assert_eq!(off.console_token_file, None);

        let on = parse(
            &[
                "--install",
                "/opt/summrise",
                "--data",
                "/var/lib/summrise",
                "--console-url",
                "https://console.example.test",
                "--console-token-file",
                "/etc/summrise/console.json",
            ],
            &[],
        )
        .expect("both console facts parse");
        assert_eq!(
            on.console_url.as_deref(),
            Some("https://console.example.test")
        );
        assert_eq!(
            on.console_token_file,
            Some(PathBuf::from("/etc/summrise/console.json"))
        );

        let url_only = parse(
            &[
                "--install",
                "/opt/summrise",
                "--data",
                "/var/lib/summrise",
                "--console-url",
                "https://console.example.test",
            ],
            &[],
        )
        .expect_err("a URL without a token would connect and be refused");
        assert_eq!(
            url_only,
            ConfigError::Missing {
                option: "--console-token-file",
                env: "SUMMRISE_CONSOLE_TOKEN_FILE",
            }
        );
    }

    #[test]
    fn a_missing_install_is_a_typed_error_not_a_panic() {
        let error = parse(&["--data", "/var/lib/summrise"], &[]).expect_err("install is required");

        assert_eq!(
            error,
            ConfigError::Missing {
                option: "--install",
                env: "SUMMRISE_INSTALL",
            }
        );
    }

    #[test]
    fn a_missing_data_directory_is_a_typed_error_not_a_panic() {
        let error = parse(&["--install", "/opt/summrise"], &[]).expect_err("data is required");

        assert_eq!(
            error,
            ConfigError::Missing {
                option: "--data",
                env: "SUMMRISE_DATA",
            }
        );
    }

    #[test]
    fn an_invalid_port_is_a_typed_error() {
        for value in ["not-a-port", "70000", "-1", ""] {
            let error = parse(
                &[
                    "--install",
                    "/opt/summrise",
                    "--data",
                    "/var/lib/summrise",
                    "--port",
                    value,
                ],
                &[],
            )
            .expect_err("that is not a port");

            assert!(
                matches!(
                    error,
                    ConfigError::Invalid {
                        option: "--port",
                        ..
                    }
                ),
                "'{value}' must name the port flag: {error:?}"
            );
        }

        let error = parse(
            &["--install", "/opt/summrise", "--data", "/var/lib/summrise"],
            &[("SUMMRISE_PORT", "eighty")],
        )
        .expect_err("the environment port is read with the same rule");

        assert!(
            matches!(
                error,
                ConfigError::Invalid {
                    option: "--port",
                    ..
                }
            ),
            "an environment port fails the same way: {error:?}"
        );
    }

    #[test]
    fn an_unknown_argument_is_refused_by_name() {
        let error = parse(
            &[
                "--install",
                "/opt/summrise",
                "--data",
                "/var/lib/summrise",
                "--verbose",
            ],
            &[],
        )
        .expect_err("--verbose is not a flag");

        assert_eq!(error, ConfigError::UnknownArgument("--verbose".to_owned()));
    }

    #[test]
    fn a_flag_without_a_value_is_refused_by_name() {
        let error = parse(&["--install"], &[]).expect_err("--install needs a value");

        assert_eq!(error, ConfigError::MissingValue("--install"));
    }

    // ─────────────────────────────────────────────────────────────────────────────────────────────
    // C3: the console link a supervised install carries
    // ─────────────────────────────────────────────────────────────────────────────────────────────

    /// The launcher persists `--console-url` / `--console-token-file` as `<data>/console.json`; the
    /// app, started by the SCM with no console flags and no `SUMMRISE_CONSOLE_*` environment, reads
    /// that file. This is what makes the console armable without starting a second instance.
    #[test]
    fn a_console_link_file_arms_a_supervised_install() {
        let dir = crate::test_support::TempDir::new("console-link");
        let data = dir.join("data");
        std::fs::create_dir_all(&data).expect("the data directory is creatable");
        let token_file = data.join("console-token.json");
        std::fs::write(
            data.join(super::CONSOLE_FILE),
            format!(
                r#"{{"url":"https://console.example.test","token_file":{}}}"#,
                serde_json::json!(token_file.to_string_lossy())
            ),
        )
        .expect("the launcher's console link is written");
        // The launcher writes 0600; the test must too, or the app's own mode check (covered below)
        // refuses this fixture before it reaches the assertion.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                data.join(super::CONSOLE_FILE),
                std::fs::Permissions::from_mode(0o600),
            )
            .expect("the fixture mode is settable");
        }

        let mut config = parse(
            &[
                "--install",
                "/opt/summrise",
                "--data",
                data.to_str().expect("the fixture path is UTF-8"),
            ],
            &[],
        )
        .expect("the supervised command line parses");

        config
            .resolve_console_file()
            .expect("the persisted console link is readable");

        assert_eq!(
            config.console_url.as_deref(),
            Some("https://console.example.test")
        );
        assert_eq!(
            config.console_token_file.as_deref(),
            Some(token_file.as_path())
        );
    }

    /// A supervised install's mesh facts live in the same persisted file as the console's: the
    /// SCM's fixed argv cannot carry them either, and the file is the one path that can.
    #[test]
    fn a_supervised_install_reads_the_mesh_facts_from_the_persisted_link() {
        let dir = crate::test_support::TempDir::new("link-mesh");
        let data = dir.join("data");
        std::fs::create_dir_all(&data).expect("the data directory is creatable");
        write_link_fixture(
            &data,
            r#"{"mesh_broker":"http://127.0.0.1:8080","mesh_relay":"127.0.0.1:9000","mesh_stun":"203.0.113.10:3478"}"#,
        );

        let mut config = parse(
            &[
                "--install",
                "/opt/summrise",
                "--data",
                data.to_str().expect("the fixture path is UTF-8"),
            ],
            &[],
        )
        .expect("the supervised command line parses");
        config
            .resolve_console_file()
            .expect("the persisted link is readable");

        assert_eq!(config.mesh_broker.as_deref(), Some("http://127.0.0.1:8080"));
        assert_eq!(config.mesh_relay.as_deref(), Some("127.0.0.1:9000"));
        assert_eq!(
            config.mesh_stun.as_deref(),
            Some("203.0.113.10:3478"),
            "the STUN reflector is a mesh fact the file can carry"
        );
        assert_eq!(
            (config.console_url, config.console_token_file),
            (None, None),
            "a mesh-only install arms no console"
        );
    }

    #[test]
    fn a_mesh_flag_beats_the_persisted_mesh_facts() {
        let dir = crate::test_support::TempDir::new("link-mesh-precedence");
        let data = dir.join("data");
        std::fs::create_dir_all(&data).expect("the data directory is creatable");
        write_link_fixture(
            &data,
            r#"{"mesh_broker":"http://persisted.example.test:8080","mesh_relay":"10.0.0.9:9000","mesh_stun":"203.0.113.10:3478"}"#,
        );

        let mut config = parse(
            &[
                "--install",
                "/opt/summrise",
                "--data",
                data.to_str().expect("the fixture path is UTF-8"),
                MESH_BROKER_FLAG,
                "http://127.0.0.1:9090",
            ],
            &[],
        )
        .expect("the command line parses");
        config
            .resolve_console_file()
            .expect("the persisted link is readable for the facts the flag did not name");

        assert_eq!(
            config.mesh_broker.as_deref(),
            Some("http://127.0.0.1:9090"),
            "the operator's flag is the instruction; the file is the fallback"
        );
        assert_eq!(
            config.mesh_relay, None,
            "the file's relay is not grafted onto a broker the operator named"
        );
        assert_eq!(
            config.mesh_stun, None,
            "the file's reflector is not grafted onto a broker the operator named either"
        );
    }

    #[test]
    fn a_mesh_environment_beats_the_persisted_mesh_facts() {
        let dir = crate::test_support::TempDir::new("link-mesh-env-precedence");
        let data = dir.join("data");
        std::fs::create_dir_all(&data).expect("the data directory is creatable");
        write_link_fixture(
            &data,
            r#"{"mesh_broker":"http://persisted.example.test:8080","mesh_relay":"10.0.0.9:9000","mesh_stun":"203.0.113.10:3478"}"#,
        );

        let mut config = parse(
            &[
                "--install",
                "/opt/summrise",
                "--data",
                data.to_str().expect("the fixture path is UTF-8"),
            ],
            &[
                ("SUMMRISE_MESH_BROKER", "http://env.example.test:8080"),
                ("SUMMRISE_MESH_RELAY", "10.0.0.8:9000"),
                ("SUMMRISE_MESH_STUN", "203.0.113.20:3478"),
            ],
        )
        .expect("the environment supplies every mesh fact");
        config
            .resolve_console_file()
            .expect("the file is not consulted for a feature the environment named");

        assert_eq!(
            (
                config.mesh_broker.as_deref(),
                config.mesh_relay.as_deref(),
                config.mesh_stun.as_deref(),
            ),
            (
                Some("http://env.example.test:8080"),
                Some("10.0.0.8:9000"),
                Some("203.0.113.20:3478")
            ),
            "the environment is the operator's instruction and wins over a previous install's file"
        );
    }

    #[test]
    fn a_persisted_relay_without_a_broker_is_refused() {
        let dir = crate::test_support::TempDir::new("link-mesh-half");
        let data = dir.join("data");
        std::fs::create_dir_all(&data).expect("the data directory is creatable");
        write_link_fixture(&data, r#"{"mesh_relay":"127.0.0.1:9000"}"#);

        let mut config = parse(
            &[
                "--install",
                "/opt/summrise",
                "--data",
                data.to_str().expect("the fixture path is UTF-8"),
            ],
            &[],
        )
        .expect("the command line parses");

        let error = config
            .resolve_console_file()
            .expect_err("a relay with no rendezvous can never be reached");
        assert!(
            error.to_string().contains(MESH_BROKER_FLAG),
            "the refusal names the flag that would fix it: {error}"
        );
    }

    #[test]
    fn a_persisted_stun_without_a_broker_is_refused() {
        let dir = crate::test_support::TempDir::new("link-mesh-stun-half");
        let data = dir.join("data");
        std::fs::create_dir_all(&data).expect("the data directory is creatable");
        write_link_fixture(&data, r#"{"mesh_stun":"203.0.113.10:3478"}"#);

        let mut config = parse(
            &[
                "--install",
                "/opt/summrise",
                "--data",
                data.to_str().expect("the fixture path is UTF-8"),
            ],
            &[],
        )
        .expect("the command line parses");

        let error = config
            .resolve_console_file()
            .expect_err("a reflection with nowhere to publish it is a half-fact");
        assert!(
            error.to_string().contains(MESH_BROKER_FLAG),
            "the refusal names the flag that would fix it: {error}"
        );
    }

    #[test]
    fn an_empty_persisted_stun_is_refused() {
        let dir = crate::test_support::TempDir::new("link-mesh-stun-empty");
        let data = dir.join("data");
        std::fs::create_dir_all(&data).expect("the data directory is creatable");
        write_link_fixture(
            &data,
            r#"{"mesh_broker":"http://127.0.0.1:8080","mesh_stun":"  "}"#,
        );

        let mut config = parse(
            &[
                "--install",
                "/opt/summrise",
                "--data",
                data.to_str().expect("the fixture path is UTF-8"),
            ],
            &[],
        )
        .expect("the command line parses");

        let error = config
            .resolve_console_file()
            .expect_err("an empty reflector is a broken install, not an absent one");
        assert!(
            error.to_string().contains("mesh_stun is empty"),
            "the refusal names the field: {error}"
        );
    }

    #[test]
    fn a_link_that_names_neither_console_nor_mesh_is_refused() {
        let dir = crate::test_support::TempDir::new("link-empty");
        let data = dir.join("data");
        std::fs::create_dir_all(&data).expect("the data directory is creatable");
        write_link_fixture(&data, "{}");

        let mut config = parse(
            &[
                "--install",
                "/opt/summrise",
                "--data",
                data.to_str().expect("the fixture path is UTF-8"),
            ],
            &[],
        )
        .expect("the command line parses");

        let error = config
            .resolve_console_file()
            .expect_err("a link file with no fact in it is a broken install");
        assert!(
            error.to_string().contains("neither"),
            "the refusal says what is missing: {error}"
        );
    }

    /// Write a link fixture the way the launcher does: owner-only, or the mode check refuses it
    /// before the content is read.
    fn write_link_fixture(data: &std::path::Path, json: &str) {
        let path = data.join(super::CONSOLE_FILE);
        std::fs::write(&path, json).expect("the link fixture is written");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
                .expect("the fixture mode is settable");
        }
    }

    #[test]
    fn a_missing_console_link_file_leaves_the_console_off() {
        let dir = crate::test_support::TempDir::new("console-link-absent");
        let data = dir.join("data");
        std::fs::create_dir_all(&data).expect("the data directory is creatable");
        let mut config = parse(
            &[
                "--install",
                "/opt/summrise",
                "--data",
                data.to_str().expect("the fixture path is UTF-8"),
            ],
            &[],
        )
        .unwrap();

        config
            .resolve_console_file()
            .expect("an absent console link is not an error");

        assert_eq!(config.console_url, None);
        assert_eq!(config.console_token_file, None);
        assert_eq!(
            (config.mesh_broker, config.mesh_relay, config.mesh_stun),
            (None, None, None),
            "an absent link leaves the mesh off too: no file is no change"
        );
    }

    /// A group- or world-readable link names a credential file; refuse it rather than arm a channel
    /// whose token path anyone on the box can rewrite.
    #[cfg(unix)]
    #[test]
    fn a_console_link_file_that_is_not_owner_only_is_refused() {
        use std::os::unix::fs::PermissionsExt;

        let dir = crate::test_support::TempDir::new("console-link-mode");
        let data = dir.join("data");
        std::fs::create_dir_all(&data).expect("the data directory is creatable");
        let path = data.join(super::CONSOLE_FILE);
        std::fs::write(
            &path,
            r#"{"url":"https://console.example.test","token_file":"console-token.json"}"#,
        )
        .expect("the launcher's console link is written");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))
            .expect("the fixture mode is settable");

        let mut config = parse(
            &[
                "--install",
                "/opt/summrise",
                "--data",
                data.to_str().expect("the fixture path is UTF-8"),
            ],
            &[],
        )
        .unwrap();

        let error = config
            .resolve_console_file()
            .expect_err("a 0644 console link must be refused");

        let text = error.to_string();
        assert!(text.contains("0644"), "the refusal names the mode: {text}");
        assert!(text.contains("0600"), "the refusal names the rule: {text}");
        assert_eq!(config.console_url, None);
    }

    #[test]
    fn a_console_flag_beats_the_persisted_link() {
        let dir = crate::test_support::TempDir::new("console-link-precedence");
        let data = dir.join("data");
        std::fs::create_dir_all(&data).expect("the data directory is creatable");
        write_link_fixture(
            &data,
            r#"{"url":"https://persisted.example.test","token_file":"persisted-token.json"}"#,
        );

        let mut config = parse(
            &[
                "--install",
                "/opt/summrise",
                "--data",
                data.to_str().expect("the fixture path is UTF-8"),
                "--console-url",
                "https://flag.example.test",
                "--console-token-file",
                "/tmp/flag-token.json",
            ],
            &[],
        )
        .unwrap();

        config
            .resolve_console_file()
            .expect("a complete pair is left alone");

        assert_eq!(
            config.console_url.as_deref(),
            Some("https://flag.example.test")
        );
        assert_eq!(
            config.console_token_file.as_deref(),
            Some(std::path::Path::new("/tmp/flag-token.json"))
        );
    }
}
