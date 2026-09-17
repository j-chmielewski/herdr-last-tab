use std::env;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Command, ExitCode};

use fs2::FileExt;
use herdr_last_tab::HistoryStore;
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Default, Deserialize)]
struct InvocationContext {
    workspace_id: Option<String>,
    tab_id: Option<String>,
    focused_pane_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct EventEnvelope {
    event: String,
    data: EventData,
}

#[derive(Debug, Deserialize)]
struct EventData {
    #[serde(rename = "type")]
    kind: String,
    workspace_id: String,
    #[serde(default)]
    tab_id: Option<String>,
    #[serde(default)]
    pane_id: Option<String>,
}

enum FocusEvent {
    Tab {
        workspace_id: String,
        tab_id: String,
    },
    Pane {
        pane_id: String,
    },
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("herdr-last-tab: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let action = env::args()
        .nth(1)
        .ok_or_else(|| "expected action: tab, pane, or event".to_string())?;
    let state_path = state_path()?;
    let _lock = state_lock(&state_path)?;
    let mut history = HistoryStore::load(&state_path).map_err(|error| error.to_string())?;

    match action.as_str() {
        "event" => record_focus_event(&mut history)?,
        "tab" => toggle_tab(&mut history)?,
        "pane" => toggle_pane(&mut history)?,
        _ => {
            return Err(format!(
                "unknown action {action:?}; expected tab, pane, or event"
            ));
        }
    }

    history.save(&state_path).map_err(|error| error.to_string())
}

fn state_lock(state_path: &std::path::Path) -> Result<File, String> {
    let parent = state_path
        .parent()
        .ok_or_else(|| "history path has no parent directory".to_string())?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let lock =
        File::create(state_path.with_extension("lock")).map_err(|error| error.to_string())?;
    lock.lock_exclusive().map_err(|error| error.to_string())?;
    Ok(lock)
}

fn record_focus_event(history: &mut HistoryStore) -> Result<(), String> {
    let event_name =
        env::var("HERDR_PLUGIN_EVENT").map_err(|_| "HERDR_PLUGIN_EVENT is not set".to_string())?;
    let event_json = env::var("HERDR_PLUGIN_EVENT_JSON")
        .map_err(|_| "HERDR_PLUGIN_EVENT_JSON is not set".to_string())?;
    let event = parse_focus_event(&event_name, &event_json)?;

    match event {
        FocusEvent::Tab {
            workspace_id,
            tab_id,
        } => history.observe_tab(&workspace_id, &tab_id),
        FocusEvent::Pane { pane_id } => {
            let context = invocation_context()?;
            let tab_id = required_context_value("tab", context.tab_id)?;
            history.observe_pane(&tab_id, &pane_id);
        }
    }
    Ok(())
}

fn toggle_tab(history: &mut HistoryStore) -> Result<(), String> {
    let context = invocation_context()?;
    let workspace_id = required_context_value("workspace", context.workspace_id)?;
    let current_tab_id = required_context_value("tab", context.tab_id)?;
    let tabs = herdr_json(&["tab", "list", "--workspace", &workspace_id])?;
    let live_tab_ids = ids_at(&tabs, &["result", "tabs"], "tab_id")?;
    if let Some(target) = history.previous_tab(&workspace_id, &current_tab_id, live_tab_ids) {
        herdr_json(&["tab", "focus", &target])?;
    }
    Ok(())
}

fn toggle_pane(history: &mut HistoryStore) -> Result<(), String> {
    let context = invocation_context()?;
    let workspace_id = required_context_value("workspace", context.workspace_id)?;
    let tab_id = required_context_value("tab", context.tab_id)?;
    let current_pane_id = required_context_value(
        "focused pane",
        env::var("HERDR_PANE_ID").ok().or(context.focused_pane_id),
    )?;
    let panes = herdr_json(&["pane", "list", "--workspace", &workspace_id])?;
    let live_pane_ids = panes_in_tab(&panes, &tab_id)?;
    if let Some(target) = history.previous_pane(&tab_id, &current_pane_id, live_pane_ids) {
        focus_pane(&target)?;
    }
    Ok(())
}

fn parse_focus_event(event_name: &str, event_json: &str) -> Result<FocusEvent, String> {
    let event: EventEnvelope = serde_json::from_str(event_json)
        .map_err(|error| format!("invalid plugin event JSON: {error}"))?;
    let expected_kind = event_name.replace('.', "_");
    if event.event != expected_kind || event.data.kind != expected_kind {
        return Err(format!("plugin event payload does not match {event_name}"));
    }

    match event_name {
        "tab.focused" => Ok(FocusEvent::Tab {
            workspace_id: event.data.workspace_id,
            tab_id: event
                .data
                .tab_id
                .ok_or_else(|| "tab.focused event has no tab_id".to_string())?,
        }),
        "pane.focused" => Ok(FocusEvent::Pane {
            pane_id: event
                .data
                .pane_id
                .ok_or_else(|| "pane.focused event has no pane_id".to_string())?,
        }),
        _ => Err(format!("unsupported plugin event {event_name:?}")),
    }
}

fn invocation_context() -> Result<InvocationContext, String> {
    let context = env::var("HERDR_PLUGIN_CONTEXT_JSON")
        .map_err(|_| "HERDR_PLUGIN_CONTEXT_JSON is not set".to_string())?;
    serde_json::from_str(&context).map_err(|error| format!("invalid plugin context: {error}"))
}

fn required_context_value(name: &str, value: Option<String>) -> Result<String, String> {
    value
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("the action requires a {name} context"))
}

fn state_path() -> Result<PathBuf, String> {
    let state_dir = env::var_os("HERDR_PLUGIN_STATE_DIR")
        .ok_or_else(|| "HERDR_PLUGIN_STATE_DIR is not set".to_string())?;
    Ok(PathBuf::from(state_dir).join("history.json"))
}

fn focus_pane(pane_id: &str) -> Result<(), String> {
    let socket_path = env::var_os("HERDR_SOCKET_PATH")
        .ok_or_else(|| "HERDR_SOCKET_PATH is not set".to_string())?;
    let request = pane_focus_request(pane_id);
    let mut stream = UnixStream::connect(socket_path)
        .map_err(|error| format!("could not connect to Herdr socket: {error}"))?;
    let encoded = serde_json::to_vec(&request)
        .map_err(|error| format!("could not encode pane focus request: {error}"))?;
    stream
        .write_all(&encoded)
        .and_then(|()| stream.write_all(b"\n"))
        .and_then(|()| stream.flush())
        .map_err(|error| format!("could not send pane focus request: {error}"))?;

    let mut response = String::new();
    BufReader::new(stream)
        .read_line(&mut response)
        .map_err(|error| format!("could not read pane focus response: {error}"))?;
    let response: Value = serde_json::from_str(&response)
        .map_err(|error| format!("Herdr returned invalid pane focus JSON: {error}"))?;
    if let Some(error) = response.pointer("/error/message").and_then(Value::as_str) {
        return Err(format!("Herdr pane focus failed: {error}"));
    }
    if response.get("result").is_none() {
        return Err("Herdr pane focus response did not contain a result".to_string());
    }
    Ok(())
}

fn pane_focus_request(pane_id: &str) -> Value {
    serde_json::json!({
        "id": "plugin:last-tab:pane-focus",
        "method": "pane.focus",
        "params": { "pane_id": pane_id },
    })
}

fn herdr_json(args: &[&str]) -> Result<Value, String> {
    let herdr = env::var_os("HERDR_BIN_PATH").unwrap_or_else(|| "herdr".into());
    let output = Command::new(herdr)
        .args(args)
        .output()
        .map_err(|error| format!("could not run Herdr: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "Herdr command {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("Herdr command {:?} returned invalid JSON: {error}", args))
}

fn ids_at(response: &Value, path: &[&str], id_key: &str) -> Result<Vec<String>, String> {
    let values = path.iter().try_fold(response, |value, key| value.get(*key));
    let values = values
        .and_then(Value::as_array)
        .ok_or_else(|| "Herdr response did not contain the expected list".to_string())?;
    values
        .iter()
        .map(|value| {
            value
                .get(id_key)
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| format!("Herdr response contained an item without {id_key}"))
        })
        .collect()
}

fn panes_in_tab(response: &Value, tab_id: &str) -> Result<Vec<String>, String> {
    let panes = response
        .pointer("/result/panes")
        .and_then(Value::as_array)
        .ok_or_else(|| "Herdr response did not contain a pane list".to_string())?;
    panes
        .iter()
        .filter(|pane| pane.get("tab_id").and_then(Value::as_str) == Some(tab_id))
        .map(|pane| {
            pane.get("pane_id")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| "Herdr response contained a pane without pane_id".to_string())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{FocusEvent, pane_focus_request, parse_focus_event};

    #[test]
    fn pane_focus_request_uses_the_v0_9_wire_method_and_target() {
        let request = pane_focus_request("workspace:tab:pane");

        assert_eq!(request["method"], "pane.focus");
        assert_eq!(request["params"]["pane_id"], "workspace:tab:pane");
    }

    #[test]
    fn parses_pane_focused_event_payload() {
        let event = parse_focus_event(
            "pane.focused",
            r#"{"event":"pane_focused","data":{"type":"pane_focused","pane_id":"pane-2","workspace_id":"workspace"}}"#,
        )
        .unwrap();

        assert!(matches!(event, FocusEvent::Pane { pane_id } if pane_id == "pane-2"));
    }

    #[test]
    fn parses_tab_focused_event_payload() {
        let event = parse_focus_event(
            "tab.focused",
            r#"{"event":"tab_focused","data":{"type":"tab_focused","tab_id":"tab-2","workspace_id":"workspace"}}"#,
        )
        .unwrap();

        assert!(
            matches!(event, FocusEvent::Tab { workspace_id, tab_id } if workspace_id == "workspace" && tab_id == "tab-2")
        );
    }

    #[test]
    fn rejects_event_payload_with_a_different_kind() {
        assert!(parse_focus_event(
            "pane.focused",
            r#"{"event":"tab_focused","data":{"type":"tab_focused","tab_id":"tab","workspace_id":"workspace"}}"#,
        )
        .is_err());
    }
}
