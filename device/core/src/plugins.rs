//! The plugin manifest (design §8.2): one directory, one `plugin.json`, and the rules a typo cannot
//! slip past.
//!
//! A plugin is an MCP server the operator installed by path. The manifest names it, says how to run
//! it, and is the only thing the device trusts before it starts a child. Validation is a pure
//! function here — no filesystem, no process, no clock — so every rule in the spec's table is proven
//! on the build machine. Resolving `command[0]`, canonicalising the directory and refusing a
//! namespace that a built-in capability already owns are the host's job ([`crate::registry`] is the
//! authority for that last one), because they need the world.
//!
//! Unknown keys are refused rather than ignored: in a manifest, an unknown key is almost always a
//! misspelled one, and a plugin that silently ignored it would run with a default the author did not
//! write.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

/// The manifest file's name, inside the plugin's directory.
pub const MANIFEST_FILE: &str = "plugin.json";

/// The longest `id`, in bytes; the shortest is three.
pub const MAX_ID_BYTES: usize = 64;
/// The longest `namespace`, in bytes; the shortest is two.
pub const MAX_NAMESPACE_BYTES: usize = 32;
/// The longest human `name`, in characters.
pub const MAX_NAME_CHARS: usize = 64;
/// The longest one plugin tool name, before its namespace is prefixed, in bytes.
pub const MAX_TOOL_BYTES: usize = 64;

/// One `plugin.json`, as the device reads it.
///
/// Field order is the order in this document and the order the host persists it in; it is not a wire
/// contract of its own.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginManifest {
    /// A stable id, unique on the device, e.g. `com.example.hello`.
    pub id: String,
    /// The prefix every tool this plugin exposes is advertised under.
    pub namespace: String,
    /// The human name a page shows.
    pub name: String,
    /// The plugin's version, `x.y.z`.
    pub version: String,
    /// One sentence on what the plugin adds.
    #[serde(default)]
    pub description: Option<String>,
    /// The argv to start: `command[0]` is the executable, the rest are arguments.
    pub command: Vec<String>,
    /// The working directory, relative to the plugin's directory. Defaults to the directory.
    #[serde(default)]
    pub cwd: Option<String>,
    /// Extra environment variables, added to the inherited environment.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// The transport; only `stdio` exists in v1, and an absent value means `stdio`.
    #[serde(default)]
    pub transport: Option<String>,
}

impl PluginManifest {
    /// Parse one `plugin.json` document.
    ///
    /// This is serde and nothing else: [`Self::check`] is the rules, so a caller can test a manifest
    /// it built in code the same way it tests one it read. Unknown keys are refused here, because
    /// only the derived decoder knows the allowed set.
    pub fn parse(text: &str) -> Result<Self, ManifestError> {
        serde_json::from_str(text).map_err(|error| ManifestError::Malformed {
            detail: error.to_string(),
        })
    }

    /// Check every rule that does not need the filesystem or the registry.
    pub fn check(&self) -> Result<(), ManifestError> {
        if !valid_id(&self.id) {
            return Err(ManifestError::Id {
                id: self.id.clone(),
            });
        }
        if !valid_namespace(&self.namespace) {
            return Err(ManifestError::Namespace {
                namespace: self.namespace.clone(),
            });
        }
        if !valid_name(&self.name) {
            return Err(ManifestError::Name {
                name: self.name.clone(),
            });
        }
        if !valid_version(&self.version) {
            return Err(ManifestError::Version {
                version: self.version.clone(),
            });
        }
        if self.command.is_empty() {
            return Err(ManifestError::EmptyCommand);
        }
        if self.command[0].is_empty() {
            return Err(ManifestError::CommandExecutable);
        }
        if let Some(cwd) = &self.cwd {
            if !valid_cwd(cwd) {
                return Err(ManifestError::Cwd { cwd: cwd.clone() });
            }
        }
        if let Some(transport) = &self.transport {
            if transport != "stdio" {
                return Err(ManifestError::Transport {
                    transport: transport.clone(),
                });
            }
        }
        for key in self.env.keys() {
            if !valid_env_key(key) {
                return Err(ManifestError::EnvKey { key: key.clone() });
            }
        }
        Ok(())
    }
}

/// The advertised name of one plugin tool: `<namespace>.<tool>`, with the tool's own charset checked.
///
/// The namespace is checked first so a caller with an unchecked manifest gets the field that is
/// actually wrong, not the segment that was derived from it.
pub fn tool_name(namespace: &str, tool: &str) -> Result<String, ManifestError> {
    if !valid_namespace(namespace) {
        return Err(ManifestError::Namespace {
            namespace: namespace.to_owned(),
        });
    }
    if !valid_tool(tool) {
        return Err(ManifestError::ToolName {
            tool: tool.to_owned(),
        });
    }
    Ok(format!("{namespace}.{tool}"))
}

/// The `id` rule: `[a-z0-9][a-z0-9._-]{2,63}`.
fn valid_id(value: &str) -> bool {
    let bytes = value.as_bytes();
    (3..=MAX_ID_BYTES).contains(&bytes.len())
        && (bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
        && bytes[1..]
            .iter()
            .all(|byte| matches!(byte, b'a'..=b'z' | b'0'..=b'9' | b'.' | b'_' | b'-'))
}

/// The `namespace` rule: `[a-z][a-z0-9-]{1,31}`.
fn valid_namespace(value: &str) -> bool {
    let bytes = value.as_bytes();
    (2..=MAX_NAMESPACE_BYTES).contains(&bytes.len())
        && bytes[0].is_ascii_lowercase()
        && bytes[1..]
            .iter()
            .all(|byte| matches!(byte, b'a'..=b'z' | b'0'..=b'9' | b'-'))
}

/// The human name rule: 1–64 characters, any character.
fn valid_name(value: &str) -> bool {
    let chars = value.chars().count();
    (1..=MAX_NAME_CHARS).contains(&chars)
}

/// The version rule, semver-ish: exactly `x.y.z`, each component a number.
fn valid_version(value: &str) -> bool {
    let mut parts = value.split('.');
    let (Some(major), Some(minor), Some(patch), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    [major, minor, patch]
        .iter()
        .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
}

/// The `cwd` rule: relative to the plugin's directory, and never climbing out of it.
fn valid_cwd(value: &str) -> bool {
    if value.is_empty() {
        return true;
    }
    // `Path::is_absolute` answers for the host platform; the checks below also refuse the other
    // platform's rooted spelling, so a manifest written on Windows is refused on Unix for the same
    // reason it is refused on Windows.
    if std::path::Path::new(value).is_absolute()
        || value.starts_with('/')
        || value.starts_with('\\')
    {
        return false;
    }
    let bytes = value.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' {
        return false;
    }
    !value.split(['/', '\\']).any(|component| component == "..")
}

/// The `env` key rule: `[A-Za-z_][A-Za-z0-9_]*`.
fn valid_env_key(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && (bytes[0].is_ascii_alphabetic() || bytes[0] == b'_')
        && bytes[1..]
            .iter()
            .all(|byte| matches!(byte, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_'))
}

/// A plugin tool's own name: 1–64 bytes from `[A-Za-z0-9_.-]`.
fn valid_tool(value: &str) -> bool {
    (1..=MAX_TOOL_BYTES).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

/// Why a manifest was refused. Each variant is one rule, and [`fmt::Display`] is the sentence a
/// caller shows: it names the field, never only "invalid manifest".
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ManifestError {
    /// The document is not JSON, has an unknown key, or is missing a required one.
    Malformed {
        /// serde's sentence, which names the offending key.
        detail: String,
    },
    /// `id` does not match `[a-z0-9][a-z0-9._-]{2,63}`.
    Id {
        /// The value as written.
        id: String,
    },
    /// `namespace` does not match `[a-z][a-z0-9-]{1,31}`.
    Namespace {
        /// The value as written.
        namespace: String,
    },
    /// `name` is empty or longer than 64 characters.
    Name {
        /// The value as written.
        name: String,
    },
    /// `version` is not `x.y.z`.
    Version {
        /// The value as written.
        version: String,
    },
    /// `command` is an empty array.
    EmptyCommand,
    /// `command[0]` is the empty string.
    CommandExecutable,
    /// `cwd` is absolute, rooted, drive-relative, or contains `..`.
    Cwd {
        /// The value as written.
        cwd: String,
    },
    /// `transport` is present and is not `stdio`.
    Transport {
        /// The value as written.
        transport: String,
    },
    /// An `env` key is not `[A-Za-z_][A-Za-z0-9_]*`.
    EnvKey {
        /// The key as written.
        key: String,
    },
    /// A plugin tool name is empty, longer than 64 bytes, or outside `[A-Za-z0-9_.-]`.
    ToolName {
        /// The tool name as written.
        tool: String,
    },
}

impl ManifestError {
    /// The manifest key the error is about.
    ///
    /// A caller that has to show "{field}: {reason}" (the console's inspect sheet does) can name the
    /// field without matching on the variant; the display already leads with it.
    pub fn field(&self) -> &'static str {
        match self {
            ManifestError::Malformed { .. } => MANIFEST_FILE,
            ManifestError::Id { .. } => "id",
            ManifestError::Namespace { .. } => "namespace",
            ManifestError::Name { .. } => "name",
            ManifestError::Version { .. } => "version",
            ManifestError::EmptyCommand | ManifestError::CommandExecutable => "command",
            ManifestError::Cwd { .. } => "cwd",
            ManifestError::Transport { .. } => "transport",
            ManifestError::EnvKey { .. } => "env",
            ManifestError::ToolName { .. } => "tool",
        }
    }

    /// The reason, without the field prefix: `{field}: {reason}` is the whole sentence.
    pub fn reason(&self) -> String {
        match self {
            ManifestError::Malformed { detail } => detail.clone(),
            ManifestError::Id { id } => {
                format!("value {id:?} must match [a-z0-9][a-z0-9._-]{{2,63}}")
            }
            ManifestError::Namespace { namespace } => {
                format!("value {namespace:?} must match [a-z][a-z0-9-]{{1,31}}")
            }
            ManifestError::Name { name } => {
                format!("value {name:?} must be 1 to {MAX_NAME_CHARS} characters")
            }
            ManifestError::Version { version } => {
                format!("value {version:?} must be x.y.z")
            }
            ManifestError::EmptyCommand => "must name an executable to run".to_owned(),
            ManifestError::CommandExecutable => {
                "the executable (command[0]) must not be empty".to_owned()
            }
            ManifestError::Cwd { cwd } => format!(
                "value {cwd:?} must be relative to the plugin directory and must not use '..'"
            ),
            ManifestError::Transport { transport } => {
                format!("value {transport:?} is not supported; the only transport is \"stdio\"")
            }
            ManifestError::EnvKey { key } => {
                format!("key {key:?} must match [A-Za-z_][A-Za-z0-9_]*")
            }
            ManifestError::ToolName { tool } => {
                format!("name {tool:?} must be 1 to {MAX_TOOL_BYTES} bytes from [A-Za-z0-9_.-]")
            }
        }
    }
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.field(), self.reason())
    }
}

impl std::error::Error for ManifestError {}

#[cfg(test)]
mod tests {
    use super::*;

    /// The spec's reference manifest (§8.2), as an author would write it.
    const HELLO: &str = r#"{
      "id": "com.example.hello",
      "namespace": "hello",
      "name": "Hello",
      "version": "0.1.0",
      "description": "Two friendly tools, and the reference for writing a plugin.",
      "command": ["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", "plugin.ps1"],
      "cwd": ".",
      "env": { "HELLO_GREETING": "hi" },
      "transport": "stdio"
    }"#;

    fn parsed(text: &str) -> PluginManifest {
        PluginManifest::parse(text).expect("the manifest is JSON")
    }

    #[test]
    fn the_reference_manifest_parses_and_checks() {
        let manifest = parsed(HELLO);
        manifest.check().expect("the reference manifest is valid");

        assert_eq!(manifest.id, "com.example.hello");
        assert_eq!(manifest.namespace, "hello");
        assert_eq!(manifest.name, "Hello");
        assert_eq!(manifest.version, "0.1.0");
        assert_eq!(
            manifest.description.as_deref(),
            Some("Two friendly tools, and the reference for writing a plugin.")
        );
        assert_eq!(
            manifest.command,
            [
                "powershell",
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
                "plugin.ps1"
            ]
        );
        assert_eq!(manifest.cwd.as_deref(), Some("."));
        assert_eq!(manifest.env["HELLO_GREETING"], "hi");
        assert_eq!(manifest.transport.as_deref(), Some("stdio"));
    }

    #[test]
    fn the_optional_fields_default_to_their_absence() {
        let manifest = parsed(
            r#"{
              "id": "com.example.min",
              "namespace": "min",
              "name": "Min",
              "version": "1.2.3",
              "command": ["plugin"]
            }"#,
        );
        manifest.check().expect("the minimal manifest is valid");
        assert_eq!(manifest.description, None);
        assert_eq!(manifest.cwd, None);
        assert!(manifest.env.is_empty());
        assert_eq!(manifest.transport, None);
    }

    #[test]
    fn an_unknown_key_is_a_typo_and_is_refused() {
        let error = PluginManifest::parse(
            r#"{
              "id": "com.example.hello",
              "namespace": "hello",
              "name": "Hello",
              "version": "0.1.0",
              "command": ["plugin"],
              "comand": ["plugin"]
            }"#,
        )
        .expect_err("an unknown key is not a silent default");

        assert!(
            matches!(error, ManifestError::Malformed { .. }),
            "{error:?}"
        );
        let sentence = error.to_string();
        assert!(sentence.contains("unknown field"), "{sentence}");
        assert!(sentence.contains("comand"), "{sentence}");
    }

    #[test]
    fn a_missing_required_field_names_it() {
        let error = PluginManifest::parse(
            r#"{
              "id": "com.example.hello",
              "name": "Hello",
              "version": "0.1.0",
              "command": ["plugin"]
            }"#,
        )
        .expect_err("a required field is missing");
        assert!(error.to_string().contains("namespace"), "{error}");
    }

    #[test]
    fn an_invalid_id_is_refused() {
        for id in ["", "ab", "Abc", "com example", "-hello", "com/example"] {
            let mut manifest = parsed(HELLO);
            manifest.id = id.to_owned();
            assert!(
                matches!(manifest.check(), Err(ManifestError::Id { .. })),
                "{id:?}"
            );
        }
        for id in ["abc", "a.b-c_d", "0hello", &"a".repeat(64)] {
            let mut manifest = parsed(HELLO);
            manifest.id = id.to_owned();
            manifest
                .check()
                .unwrap_or_else(|error| panic!("{id:?}: {error}"));
        }
        let mut manifest = parsed(HELLO);
        manifest.id = "a".repeat(65);
        assert!(matches!(manifest.check(), Err(ManifestError::Id { .. })));
    }

    #[test]
    fn a_namespace_with_an_uppercase_letter_or_underscore_is_refused() {
        for namespace in ["Hello", "hel_lo", "h", "1hello", "hello!", &"a".repeat(33)] {
            let mut manifest = parsed(HELLO);
            manifest.namespace = namespace.to_owned();
            let error = manifest.check().expect_err(namespace);
            assert!(
                matches!(error, ManifestError::Namespace { .. }),
                "{error:?}"
            );
            assert!(error.to_string().contains("namespace"), "{error}");
        }
        for namespace in ["hello", "h1", "hello-world", &"a".repeat(32)] {
            let mut manifest = parsed(HELLO);
            manifest.namespace = namespace.to_owned();
            manifest
                .check()
                .unwrap_or_else(|error| panic!("{namespace:?}: {error}"));
        }
    }

    #[test]
    fn an_empty_or_long_human_name_is_refused() {
        for name in ["", &"n".repeat(65)] {
            let mut manifest = parsed(HELLO);
            manifest.name = name.to_owned();
            assert!(matches!(manifest.check(), Err(ManifestError::Name { .. })));
        }
        let mut manifest = parsed(HELLO);
        manifest.name = "n".repeat(64);
        manifest
            .check()
            .expect("64 characters is the cap, not the first refusal");
    }

    #[test]
    fn a_version_that_is_not_x_y_z_is_refused() {
        for version in ["", "1", "1.2", "1.2.3.4", "1.2.x", "1..3", "v1.2.3"] {
            let mut manifest = parsed(HELLO);
            manifest.version = version.to_owned();
            assert!(
                matches!(manifest.check(), Err(ManifestError::Version { .. })),
                "{version:?}"
            );
        }
        let mut manifest = parsed(HELLO);
        manifest.version = "10.20.30".to_owned();
        manifest
            .check()
            .expect("three numeric components are a version");
    }

    #[test]
    fn an_empty_command_is_refused() {
        let mut manifest = parsed(HELLO);
        manifest.command = Vec::new();
        let error = manifest.check().expect_err("nothing to run");
        assert!(matches!(error, ManifestError::EmptyCommand), "{error:?}");
        assert!(error.to_string().contains("command"), "{error}");
    }

    #[test]
    fn an_empty_executable_is_refused() {
        let mut manifest = parsed(HELLO);
        manifest.command = vec![String::new(), "arg".to_owned()];
        let error = manifest.check().expect_err("nothing to run");
        assert!(
            matches!(error, ManifestError::CommandExecutable),
            "{error:?}"
        );
        assert!(error.to_string().contains("command"), "{error}");
    }

    #[test]
    fn a_cwd_that_leaves_the_plugin_directory_is_refused() {
        for cwd in [
            "..",
            "../x",
            "sub/../x",
            "sub\\..\\x",
            "/tmp",
            "\\rooted",
            "C:\\plugin",
            "C:plugin",
        ] {
            let mut manifest = parsed(HELLO);
            manifest.cwd = Some(cwd.to_owned());
            let error = manifest.check().expect_err(cwd);
            assert!(
                matches!(error, ManifestError::Cwd { .. }),
                "{cwd:?} {error:?}"
            );
            assert!(error.to_string().contains("cwd"), "{error}");
        }
        for cwd in ["", ".", "sub", "sub/dir", "sub\\dir"] {
            let mut manifest = parsed(HELLO);
            manifest.cwd = Some(cwd.to_owned());
            manifest
                .check()
                .unwrap_or_else(|error| panic!("{cwd:?}: {error}"));
        }
    }

    #[test]
    fn a_transport_other_than_stdio_is_refused() {
        let mut manifest = parsed(HELLO);
        manifest.transport = Some("sse".to_owned());
        let error = manifest.check().expect_err("v1 speaks stdio");
        assert!(
            matches!(error, ManifestError::Transport { .. }),
            "{error:?}"
        );
        assert!(error.to_string().contains("transport"), "{error}");

        let mut manifest = parsed(HELLO);
        manifest.transport = Some("stdio".to_owned());
        manifest.check().expect("stdio is the transport v1 has");
    }

    #[test]
    fn an_env_key_that_is_not_an_identifier_is_refused() {
        for key in ["", "1HELLO", "HELLO-GREETING", "HELLO GREETING", "héllo"] {
            let mut manifest = parsed(HELLO);
            manifest.env.insert(key.to_owned(), "x".to_owned());
            let error = manifest.check().expect_err(key);
            assert!(matches!(error, ManifestError::EnvKey { .. }), "{key:?}");
            assert!(error.to_string().contains("env"), "{error}");
        }
        for key in ["HELLO", "_HELLO", "HELLO_1"] {
            let mut manifest = parsed(HELLO);
            manifest.env.insert(key.to_owned(), "x".to_owned());
            manifest
                .check()
                .unwrap_or_else(|error| panic!("{key:?}: {error}"));
        }
    }

    #[test]
    fn a_tool_name_outside_the_charset_or_length_is_refused() {
        for tool in [
            "",
            "has space",
            "semi;colon",
            "slash/name",
            "héllo",
            &"t".repeat(65),
        ] {
            let error = tool_name("hello", tool).expect_err(tool);
            assert!(matches!(error, ManifestError::ToolName { .. }), "{tool:?}");
            assert!(error.to_string().contains("tool"), "{error}");
        }
        for tool in ["greet", "greet_2", "greet-2", "greet.read", &"t".repeat(64)] {
            assert_eq!(
                tool_name("hello", tool).expect(tool),
                format!("hello.{tool}")
            );
        }
    }

    #[test]
    fn the_field_and_the_reason_compose_the_sentence() {
        let mut manifest = parsed(HELLO);
        manifest.namespace = "Hello".to_owned();
        let error = manifest.check().expect_err("the namespace is invalid");
        assert_eq!(error.field(), "namespace");
        assert_eq!(
            error.to_string(),
            format!("{}: {}", error.field(), error.reason()),
            "a caller can show the field and the reason without matching on the variant"
        );
        assert!(error.reason().contains("must match"), "{error}");

        for (text, field) in [
            (HELLO.replace("\"hello\"", "\"Hello\""), "namespace"),
            (HELLO.replace("\"0.1.0\"", "\"0.1\""), "version"),
            (HELLO.replace("\"id\"", "\"id2\""), "plugin.json"),
        ] {
            let error = PluginManifest::parse(&text)
                .and_then(|manifest| manifest.check())
                .expect_err("the manifest is invalid");
            assert_eq!(error.field(), field, "{text}: {error}");
        }
    }

    #[test]
    fn a_tool_name_is_namespaced_by_its_plugin() {
        assert_eq!(tool_name("hello", "greet").unwrap(), "hello.greet");
        // A namespace that is not a namespace is the namespace rule's refusal, not the tool's.
        let error = tool_name("Hello", "greet").expect_err("the namespace is checked too");
        assert!(
            matches!(error, ManifestError::Namespace { .. }),
            "{error:?}"
        );
    }
}
