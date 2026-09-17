# herdr-last-tab

A Rust [Herdr](https://herdr.dev) plugin with two manifest actions:

- `j-chmielewski.last-tab.toggle-tab` toggles the most recently focused tab
  within the current workspace.
- `j-chmielewski.last-tab.toggle-pane` toggles the most recently focused pane
  in the current tab.

## Install

Clone the repository, build the plugin, and link it to Herdr:

```sh
git clone https://github.com/j-chmielewski/herdr-last-tab.git
cd herdr-last-tab
cargo build --release
herdr plugin link .
```

The manifest build hook runs `cargo build --release` for managed installs.

## History semantics

The manifest subscribes to Herdr's `tab.focused` and `pane.focused` events.
Every focus event updates MRU history immediately, including focus changes made
without invoking either action. Replayed or duplicate events move the same ID
to the end once, rather than adding duplicates.

History is stored in `HERDR_PLUGIN_STATE_DIR/history.json`. Updates are
serialized with a plugin-local lock and persisted by atomic replacement. Tab
histories are keyed by workspace and pane histories by tab. When an action is
invoked, it only selects the most recent *live* entry other than the current
one: `tab list` and `pane list` prune closed entries at that point. Consequently
closed or moved panes cannot be focused from another tab. `toggle-pane` sends
its selected ordinary pane ID with the `pane.focus` socket request.

## Compatibility

The plugin requires Herdr 0.9.0 or newer. This version supports manifest event
hooks and the v0.9 `pane.focus` socket request used by the pane action.

The release command uses a Unix-style path, so the manifest currently declares
Linux and macOS only.
