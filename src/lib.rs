use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Deserialize, Serialize)]
pub struct HistoryStore {
    #[serde(default)]
    tabs: BTreeMap<String, History>,
    #[serde(default)]
    panes: BTreeMap<String, History>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct History {
    entries: Vec<String>,
}

impl HistoryStore {
    /// Records a tab focus observation in workspace-scoped MRU order.
    pub fn observe_tab(&mut self, workspace_id: &str, tab_id: &str) {
        self.tabs
            .entry(workspace_id.to_owned())
            .or_default()
            .observe(tab_id);
    }

    /// Records a pane focus observation in tab-scoped MRU order.
    pub fn observe_pane(&mut self, tab_id: &str, pane_id: &str) {
        self.panes
            .entry(tab_id.to_owned())
            .or_default()
            .observe(pane_id);
    }

    /// Returns the most recently observed live tab other than `current`.
    pub fn previous_tab(
        &mut self,
        workspace_id: &str,
        current: &str,
        live_tab_ids: impl IntoIterator<Item = String>,
    ) -> Option<String> {
        self.tabs
            .entry(workspace_id.to_owned())
            .or_default()
            .previous(current, live_tab_ids)
    }

    /// Returns the most recently observed live pane in `tab_id` other than `current`.
    pub fn previous_pane(
        &mut self,
        tab_id: &str,
        current: &str,
        live_pane_ids: impl IntoIterator<Item = String>,
    ) -> Option<String> {
        self.panes
            .entry(tab_id.to_owned())
            .or_default()
            .previous(current, live_pane_ids)
    }

    pub fn load(path: &Path) -> io::Result<Self> {
        match fs::read(path) {
            Ok(contents) => serde_json::from_slice(&contents).map_err(io::Error::other),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error),
        }
    }

    /// Atomically replaces the persisted state. Callers serialize updates with the state lock.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        let parent = path.parent().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "history path has no parent directory",
            )
        })?;
        fs::create_dir_all(parent)?;
        let contents = serde_json::to_vec(self).map_err(io::Error::other)?;
        let temporary = path.with_extension("tmp");
        fs::write(&temporary, contents)?;
        fs::rename(temporary, path)
    }
}

impl History {
    fn observe(&mut self, id: &str) {
        self.entries.retain(|entry| entry != id);
        self.entries.push(id.to_owned());
    }

    fn previous(
        &mut self,
        current: &str,
        live_ids: impl IntoIterator<Item = String>,
    ) -> Option<String> {
        let live_ids: HashSet<_> = live_ids.into_iter().collect();
        self.entries.retain(|entry| live_ids.contains(entry));
        self.entries
            .iter()
            .rev()
            .find(|entry| entry.as_str() != current)
            .cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::HistoryStore;

    fn ids(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn focus_history_toggles_panes_that_were_focused_between_actions() {
        let mut history = HistoryStore::default();
        history.observe_pane("tab", "one");
        history.observe_pane("tab", "two");
        history.observe_pane("tab", "three");

        assert_eq!(
            history.previous_pane("tab", "three", ids(&["one", "two", "three"])),
            Some("two".into())
        );
    }

    #[test]
    fn repeated_event_replay_keeps_a_single_mru_entry() {
        let mut history = HistoryStore::default();
        history.observe_pane("tab", "one");
        history.observe_pane("tab", "two");
        history.observe_pane("tab", "two");

        assert_eq!(
            history.previous_pane("tab", "two", ids(&["one", "two"])),
            Some("one".into())
        );
    }

    #[test]
    fn tab_history_is_updated_by_focus_observations() {
        let mut history = HistoryStore::default();
        history.observe_tab("workspace", "one");
        history.observe_tab("workspace", "two");

        assert_eq!(
            history.previous_tab("workspace", "two", ids(&["one", "two"])),
            Some("one".into())
        );
    }

    #[test]
    fn closed_objects_are_pruned_before_selecting_a_target() {
        let mut history = HistoryStore::default();
        history.observe_tab("workspace", "one");
        history.observe_tab("workspace", "two");

        assert_eq!(
            history.previous_tab("workspace", "one", ids(&["one"])),
            None
        );
    }

    #[test]
    fn pane_history_is_scoped_to_the_current_tab() {
        let mut history = HistoryStore::default();
        history.observe_pane("tab-one", "one");
        history.observe_pane("tab-two", "two");

        assert_eq!(history.previous_pane("tab-one", "one", ids(&["one"])), None);
    }
}
