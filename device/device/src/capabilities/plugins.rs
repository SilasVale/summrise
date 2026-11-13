//! The `plugins.*` tools (design §8.3): the operator's surface for the plugin host.
//!
//! These are ordinary registry tools with ordinary handlers, not a second API. The console, any MCP
//! client and the tests drive the same five calls; the token scope and the audit record need no
//! plugin-specific case. The module owns the argument shapes and the JSON the plan fixed; the host
//! owns the state machine. A failure is the product's shape — `{code, message, action}` in the tool
//! text — so the page can render the same sentence the device would say in a log.

use std::path::Path;

use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::plugins::host::{PluginError, PluginHost};
use crate::plugins::logs::DEFAULT_LOG_LINES;

/// Why a `plugins.*` call did not produce its result.
#[derive(Debug)]
pub enum PluginsError {
    /// The arguments do not match the tool's input schema; a protocol-level error, not a tool refusal.
    Arguments(String),
    /// The host refused or failed; the tool text carries the `{code, message, action}` shape.
    Plugin(PluginError),
}

impl std::fmt::Display for PluginsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PluginsError::Arguments(message) => write!(f, "{message}"),
            PluginsError::Plugin(error) => write!(f, "{error}"),
        }
    }
}

/// Run one `plugins.*` tool. The name is one the registry declared; an unknown one is a bug.
pub async fn dispatch(
    host: &PluginHost,
    name: &str,
    arguments: Map<String, Value>,
) -> Result<String, PluginsError> {
    match name {
        "plugins.list" => {
            let _: EmptyArgs = parse(arguments, name)?;
            let plugins = host.list().await;
            let plugins: Vec<Value> = plugins.iter().map(|plugin| plugin.to_json()).collect();
            Ok(Value::Array(plugins).to_string())
        }
        "plugins.inspect" => {
            let arguments: PathArgs = parse(arguments, name)?;
            let inspection = host
                .inspect(Path::new(&arguments.path))
                .await
                .map_err(PluginsError::Plugin)?;
            Ok(inspection.to_json().to_string())
        }
        "plugins.add" => {
            let arguments: PathArgs = parse(arguments, name)?;
            let (plugin, tools) = host
                .add(Path::new(&arguments.path))
                .await
                .map_err(PluginsError::Plugin)?;
            Ok(json!({ "plugin": plugin.to_json(), "tools": tools }).to_string())
        }
        "plugins.remove" => {
            let arguments: NamespaceArgs = parse(arguments, name)?;
            let plugin = host
                .remove(&arguments.namespace)
                .await
                .map_err(PluginsError::Plugin)?;
            Ok(json!({ "plugin": plugin.to_json() }).to_string())
        }
        "plugins.restart" => {
            let arguments: NamespaceArgs = parse(arguments, name)?;
            let plugin = host
                .restart(&arguments.namespace)
                .await
                .map_err(PluginsError::Plugin)?;
            Ok(json!({ "plugin": plugin.to_json() }).to_string())
        }
        "plugins.logs" => {
            let arguments: LogsArgs = parse(arguments, name)?;
            let lines = arguments.lines.unwrap_or(DEFAULT_LOG_LINES);
            let lines = host
                .logs(&arguments.namespace, lines)
                .await
                .map_err(PluginsError::Plugin)?;
            Ok(json!({
                "namespace": arguments.namespace,
                "lines": lines,
            })
            .to_string())
        }
        other => Err(PluginsError::Arguments(format!("unknown tool: {other}"))),
    }
}

/// The arguments `plugins.list` takes.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyArgs {}

/// The arguments `plugins.inspect` and `plugins.add` take.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PathArgs {
    path: String,
}

/// The arguments `plugins.remove` and `plugins.restart` take.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NamespaceArgs {
    namespace: String,
}

/// The arguments `plugins.logs` take.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LogsArgs {
    namespace: String,
    lines: Option<usize>,
}

/// Parse arguments without echoing a value into the error: a caller can put a secret in the wrong
/// field, and the schema's category is the whole fact an operator needs.
fn parse<T: DeserializeOwned>(
    arguments: Map<String, Value>,
    tool: &str,
) -> Result<T, PluginsError> {
    serde_json::from_value(Value::Object(arguments)).map_err(|error| {
        let reason = match error.classify() {
            serde_json::error::Category::Data => "a value has the wrong type or shape",
            serde_json::error::Category::Io | serde_json::error::Category::Syntax => {
                "the arguments are not valid JSON"
            }
            serde_json::error::Category::Eof => "the arguments ended early",
        };
        PluginsError::Arguments(format!("{tool}: {reason}"))
    })
}

// ---------------------------------------------------------------------------------------------
// The declaration's implementation: the device side of `summrise_core::capabilities::plugins`.
// ---------------------------------------------------------------------------------------------

use crate::mcp::ToolError;
use summrise_core::capabilities::plugins::CAPABILITY;

/// Whether `tool` is one this capability declares.
pub fn owns(tool: &str) -> bool {
    CAPABILITY
        .tools
        .iter()
        .any(|declared| declared.name == tool)
}

/// Every tool this capability declares, for the registry-alignment test.
#[cfg(test)]
pub fn names() -> impl Iterator<Item = &'static str> {
    CAPABILITY.tools.iter().map(|tool| tool.name)
}

/// Run one `plugins.*` tool, mapping the host's errors onto the surface's.
pub async fn call(
    host: &crate::plugins::host::PluginHost,
    tool: &str,
    arguments: Map<String, Value>,
) -> Result<String, ToolError> {
    match dispatch(host, tool, arguments).await {
        Ok(text) => Ok(text),
        Err(PluginsError::Arguments(message)) => Err(ToolError::Arguments(message)),
        Err(PluginsError::Plugin(error)) => Err(ToolError::Refused(error.to_text())),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::Arc;

    use super::*;
    use crate::plugins::host::PluginHost;
    use crate::plugins::test_support::{host as plugin_host, plugin_dir, ScriptedLauncher};
    use crate::test_support::TempDir;

    fn arguments(value: Value) -> Map<String, Value> {
        value.as_object().cloned().expect("an object of arguments")
    }

    fn text(error: PluginsError) -> String {
        match error {
            PluginsError::Arguments(message) => message,
            PluginsError::Plugin(error) => error.to_text(),
        }
    }

    #[tokio::test]
    async fn inspect_reads_a_manifest_and_starts_nothing() {
        let dir = TempDir::new("plugins-inspect");
        let data = dir.join("data");
        fs::create_dir_all(&data).expect("the data dir");
        let source = plugin_dir(&dir, "hello");
        let host = PluginHost::load(&data);

        let answer = dispatch(
            &host,
            "plugins.inspect",
            arguments(json!({ "path": source.display().to_string() })),
        )
        .await
        .expect("the manifest is readable");
        let inspection: Value = serde_json::from_str(&answer).expect("the answer is JSON");
        assert_eq!(
            inspection,
            json!({
                "id": "com.example.hello",
                "namespace": "hello",
                "name": "hello plugin",
                "version": "0.1.0",
                "description": "A scripted plugin for the host tests.",
            })
        );
        assert!(
            host.list().await.is_empty(),
            "inspect starts nothing and records nothing"
        );
    }

    #[tokio::test]
    async fn inspect_names_the_missing_path_missing_manifest_and_invalid_field() {
        let dir = TempDir::new("plugins-inspect-errors");
        let data = dir.join("data");
        fs::create_dir_all(&data).expect("the data dir");
        let host = PluginHost::load(&data);

        let missing = dir.join("does-not-exist");
        let error = dispatch(
            &host,
            "plugins.inspect",
            arguments(json!({ "path": missing.display().to_string() })),
        )
        .await
        .expect_err("there is no directory there");
        let error = match error {
            PluginsError::Plugin(error) => error,
            PluginsError::Arguments(message) => panic!("{message}"),
        };
        assert_eq!(error.code(), "NOT_FOUND");
        assert_eq!(
            error.to_string(),
            format!("no directory at {}", missing.display())
        );

        let empty = dir.join("empty-plugin");
        fs::create_dir_all(&empty).expect("the directory");
        let error = dispatch(
            &host,
            "plugins.inspect",
            arguments(json!({ "path": empty.display().to_string() })),
        )
        .await
        .expect_err("there is no plugin.json");
        let error = match error {
            PluginsError::Plugin(error) => error,
            PluginsError::Arguments(message) => panic!("{message}"),
        };
        assert_eq!(error.code(), "NO_MANIFEST");
        assert!(
            error.to_string().starts_with(&format!(
                "no plugin.json in {}",
                fs::canonicalize(&empty).unwrap().display()
            )),
            "{error}"
        );

        let invalid = dir.join("invalid-plugin");
        fs::create_dir_all(&invalid).expect("the directory");
        fs::write(
            invalid.join("plugin.json"),
            json!({
                "id": "com.example.invalid",
                "namespace": "Bad",
                "name": "Bad",
                "version": "0.1.0",
                "command": ["program"],
            })
            .to_string(),
        )
        .expect("the manifest");
        let error = dispatch(
            &host,
            "plugins.inspect",
            arguments(json!({ "path": invalid.display().to_string() })),
        )
        .await
        .expect_err("the namespace rule fails");
        let error = match error {
            PluginsError::Plugin(error) => error,
            PluginsError::Arguments(message) => panic!("{message}"),
        };
        assert_eq!(error.code(), "INVALID_MANIFEST");
        assert!(
            error.to_string().starts_with("namespace: "),
            "the message is \"{{field}}: {{reason}}\": {error}"
        );
        assert!(host.list().await.is_empty());
    }

    #[tokio::test]
    async fn list_add_restart_remove_logs_and_inspect_have_the_plan_shapes() {
        let dir = TempDir::new("plugins-shapes");
        let launcher = ScriptedLauncher::hello();
        let host = plugin_host(&dir, Arc::clone(&launcher));
        let source = plugin_dir(&dir, "hello");

        let added = dispatch(
            &host,
            "plugins.add",
            arguments(json!({ "path": source.display().to_string() })),
        )
        .await
        .expect("the plugin installs");
        let added: Value = serde_json::from_str(&added).expect("JSON");
        assert_eq!(added["tools"], 1);
        assert_eq!(added["plugin"]["namespace"], "hello");
        assert_eq!(added["plugin"]["state"], "running");
        assert_eq!(
            added["plugin"]["path"],
            fs::canonicalize(&source).unwrap().display().to_string()
        );

        let listed = dispatch(&host, "plugins.list", Map::new())
            .await
            .expect("the list answers");
        let listed: Value = serde_json::from_str(&listed).expect("JSON");
        assert!(listed.is_array(), "{listed}");
        assert_eq!(listed[0]["id"], "com.example.hello");
        assert_eq!(listed[0]["state"], "running");
        assert_eq!(listed[0]["tools"], 1);
        assert_eq!(listed[0]["last_error"], Value::Null);

        let logged = dispatch(
            &host,
            "plugins.logs",
            arguments(json!({ "namespace": "hello" })),
        )
        .await
        .expect("the log answers");
        let logged: Value = serde_json::from_str(&logged).expect("JSON");
        assert_eq!(logged["namespace"], "hello");
        assert!(logged["lines"].is_array(), "{logged}");

        let restarted = dispatch(
            &host,
            "plugins.restart",
            arguments(json!({ "namespace": "hello" })),
        )
        .await
        .expect("the restart answers");
        let restarted: Value = serde_json::from_str(&restarted).expect("JSON");
        assert_eq!(restarted["plugin"]["state"], "running");

        let removed = dispatch(
            &host,
            "plugins.remove",
            arguments(json!({ "namespace": "hello" })),
        )
        .await
        .expect("the removal answers");
        let removed: Value = serde_json::from_str(&removed).expect("JSON");
        assert_eq!(removed["plugin"]["state"], "stopped");
        assert!(host.list().await.is_empty());
    }

    #[tokio::test]
    async fn an_unknown_plugin_name_is_a_tool_error_with_the_next_action() {
        let dir = TempDir::new("plugins-errors");
        let launcher = ScriptedLauncher::hello();
        let host = plugin_host(&dir, Arc::clone(&launcher));

        let error = dispatch(
            &host,
            "plugins.remove",
            arguments(json!({ "namespace": "hello" })),
        )
        .await
        .expect_err("there is no such plugin");
        let text = text(error);
        let shape: Value = serde_json::from_str(&text).expect("the error text is JSON");
        assert_eq!(shape["code"], "PLUGIN_NOT_FOUND");
        assert_eq!(shape["message"], "no plugin named hello");
        assert_eq!(shape["action"], "plugins.list");
    }

    #[tokio::test]
    async fn bad_arguments_are_a_protocol_error_and_do_not_echo_a_value() {
        let dir = TempDir::new("plugins-arguments");
        let launcher = ScriptedLauncher::hello();
        let host = plugin_host(&dir, Arc::clone(&launcher));

        let error = dispatch(
            &host,
            "plugins.add",
            arguments(json!({ "namespace": "hello" })),
        )
        .await
        .expect_err("path is required");
        assert!(
            matches!(&error, PluginsError::Arguments(message) if message == "plugins.add: a value has the wrong type or shape"),
            "{error:?}"
        );

        let error = dispatch(
            &host,
            "plugins.inspect",
            arguments(json!({ "path": "SENTINEL", "extra": "SENTINEL" })),
        )
        .await
        .expect_err("an unknown key is refused");
        match error {
            PluginsError::Arguments(message) => {
                assert!(!message.contains("SENTINEL"), "{message}");
            }
            PluginsError::Plugin(error) => panic!("{error}"),
        }
    }
}
