# herdr-last-tab

A Rust [Herdr](https://herdr.dev) plugin with two manifest actions:

- `j-chmielewski.last-tab.toggle-tab` toggles the most recently observed tab
  within the current workspace.
- `j-chmielewski.last-tab.toggle-pane` toggles the most recently observed pane
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

History is stored in `HERDR_PLUGIN_STATE_DIR/history.json`. On each invocation,
the current object is recorded, duplicate entries are removed, and objects no
longer returned by `tab list` or `pane list` are pruned. Tab histories are keyed
by workspace and pane histories by tab, so a target cannot cross either boundary.
The first invocation in a scope only records the current object because Herdr's
plugin API does not expose focus-change events or an existing MRU history.

## Compatibility

The plugin requires Herdr 0.9.0 or newer. It sends the v0.9 `pane.focus` socket
request with the resolved ordinary pane ID through `HERDR_SOCKET_PATH`; this
avoids depending on an older CLI that only exposes directional pane focus.

The release command uses a Unix-style path, so the manifest currently declares
Linux and macOS only.
