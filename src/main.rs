use std::env;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Command, ExitCode};

use herdr_last_tab::HistoryStore;
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Default, Deserialize)]
struct InvocationContext {
    workspace_id: Option<String>,
    tab_id: Option<String>,
    focused_pane_id: Option<String>,
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
        .ok_or_else(|| "expected action: tab or pane".to_string())?;
    let context = invocation_context()?;
    let state_path = state_path()?;
    let mut history = HistoryStore::load(&state_path).map_err(|error| error.to_string())?;

    match action.as_str() {
        "tab" => {
            let workspace_id = required_context_value("workspace", context.workspace_id)?;
            let current_tab_id = required_context_value("tab", context.tab_id)?;
            let tabs = herdr_json(&["tab", "list", "--workspace", &workspace_id])?;
            let live_tab_ids = ids_at(&tabs, &["result", "tabs"], "tab_id")?;
            let target = history.toggle_tab(&workspace_id, &current_tab_id, live_tab_ids);
            history
                .save(&state_path)
                .map_err(|error| error.to_string())?;
            if let Some(target) = target {
                herdr_json(&["tab", "focus", &target])?;
            }
        }
        "pane" => {
            let workspace_id = required_context_value("workspace", context.workspace_id)?;
            let tab_id = required_context_value("tab", context.tab_id)?;
            let current_pane_id = required_context_value(
                "focused pane",
                env::var("HERDR_PANE_ID").ok().or(context.focused_pane_id),
            )?;
            let panes = herdr_json(&["pane", "list", "--workspace", &workspace_id])?;
            let live_pane_ids = panes_in_tab(&panes, &tab_id)?;
            let target = history.toggle_pane(&tab_id, &current_pane_id, live_pane_ids);
            history
                .save(&state_path)
                .map_err(|error| error.to_string())?;
            if let Some(target) = target {
                focus_pane(&target)?;
            }
        }
        _ => return Err(format!("unknown action {action:?}; expected tab or pane")),
    }

    Ok(())
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
    use super::pane_focus_request;

    #[test]
    fn pane_focus_request_uses_the_v0_9_wire_method_and_target() {
        let request = pane_focus_request("workspace:tab:pane");

        assert_eq!(request["method"], "pane.focus");
        assert_eq!(request["params"]["pane_id"], "workspace:tab:pane");
    }
}
