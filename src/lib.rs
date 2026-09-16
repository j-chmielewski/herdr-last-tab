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
    /// Returns the most recently recorded live object other than `current`.
    /// Closed objects are discarded before every toggle.
    pub fn toggle_tab(
        &mut self,
        workspace_id: &str,
        current: &str,
        live_tab_ids: impl IntoIterator<Item = String>,
    ) -> Option<String> {
        self.tabs
            .entry(workspace_id.to_owned())
            .or_default()
            .toggle(current, live_tab_ids)
    }

    /// Pane histories are keyed by tab, so panes in another tab can never be selected.
    pub fn toggle_pane(
        &mut self,
        tab_id: &str,
        current: &str,
        live_pane_ids: impl IntoIterator<Item = String>,
    ) -> Option<String> {
        self.panes
            .entry(tab_id.to_owned())
            .or_default()
            .toggle(current, live_pane_ids)
    }

    pub fn load(path: &Path) -> io::Result<Self> {
        match fs::read(path) {
            Ok(contents) => serde_json::from_slice(&contents).map_err(io::Error::other),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error),
        }
    }

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
    fn toggle(
        &mut self,
        current: &str,
        live_ids: impl IntoIterator<Item = String>,
    ) -> Option<String> {
        let live_ids: HashSet<_> = live_ids.into_iter().collect();
        self.entries.retain(|entry| live_ids.contains(entry));
        self.entries.retain(|entry| entry != current);

        let target = self.entries.last().cloned();
        if live_ids.contains(current) {
            self.entries.push(current.to_owned());
        }
        target
    }
}

#[cfg(test)]
mod tests {
    use super::HistoryStore;

    fn ids(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn tab_history_toggles_between_two_recent_tabs() {
        let mut history = HistoryStore::default();
        assert_eq!(
            history.toggle_tab("workspace", "one", ids(&["one", "two"])),
            None
        );
        assert_eq!(
            history.toggle_tab("workspace", "two", ids(&["one", "two"])),
            Some("one".into())
        );
        assert_eq!(
            history.toggle_tab("workspace", "one", ids(&["one", "two"])),
            Some("two".into())
        );
    }

    #[test]
    fn pane_history_is_scoped_to_the_current_tab() {
        let mut history = HistoryStore::default();
        assert_eq!(
            history.toggle_pane("tab-one", "one", ids(&["one", "two"])),
            None
        );
        assert_eq!(
            history.toggle_pane("tab-one", "two", ids(&["one", "two"])),
            Some("one".into())
        );
        assert_eq!(
            history.toggle_pane("tab-two", "three", ids(&["three"])),
            None
        );
    }

    #[test]
    fn closed_objects_are_pruned_before_selecting_a_target() {
        let mut history = HistoryStore::default();
        history.toggle_tab("workspace", "one", ids(&["one", "two", "three"]));
        history.toggle_tab("workspace", "two", ids(&["one", "two", "three"]));
        history.toggle_tab("workspace", "three", ids(&["one", "two", "three"]));

        assert_eq!(history.toggle_tab("workspace", "one", ids(&["one"])), None);
    }

    #[test]
    fn duplicate_observations_do_not_make_the_current_object_its_own_target() {
        let mut history = HistoryStore::default();
        assert_eq!(history.toggle_pane("tab", "one", ids(&["one"])), None);
        assert_eq!(history.toggle_pane("tab", "one", ids(&["one"])), None);
    }

    #[test]
    fn histories_do_not_cross_workspace_boundaries() {
        let mut history = HistoryStore::default();
        history.toggle_tab("one", "first", ids(&["first", "second"]));
        assert_eq!(history.toggle_tab("two", "other", ids(&["other"])), None);
    }
}
