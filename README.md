# mesh-tray

A ~380-line native menu-bar/tray app for Mesh. **No webview, no Tauri, no npm, no window.**

Does exactly four things:

- start / stop the `mesh-llm` daemon
- show members (peers) in a submenu
- approve / reject incoming pairing requests inline (plain HTTP POST, no browser)
- open the console in your default browser

## Run it locally

```bash
export PATH=$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH
cargo build --release
codesign -s - --force target/release/mesh-tray   # macOS only
MESH_LLM_BIN=$HOME/bin-mesh-main/mesh-llm ./target/release/mesh-tray
```

Then click the icon in the menu bar. **Start Mesh** spawns
`$MESH_LLM_BIN serve` (default `mesh-llm` off `PATH`).

Env:

| Var | Default | Meaning |
|---|---|---|
| `MESH_LLM_BIN` | `mesh-llm` | binary to spawn for `serve` |
| `MESH_LLM_CONSOLE_PORT` | `3131` | management API + console port |

## Version-aware, not version-locked

Pairing endpoints only exist on `feature/secure-pairing-launcher-1630`, not in
any release. The tray probes `GET /api/pairing/sessions` each poll; on `404`
the pairing and approve/reject items are **omitted entirely**. So the same
binary works against released 0.76.0-rc9 today (start/stop/members/console)
and lights up admission the moment a release ships pairing.

## Deps

`tray-icon` + `muda` (the crates Tauri itself uses) + `winit` (event loop, zero
windows) + `open` + `serde`. 117 crates total; `cargo tree` contains **zero**
matches for wry/webkit/javascriptcore/tauri.

## Not covered

- `mesh-llm://pair/...` deep links (need per-platform registration)
- Start-at-login (`auto-launch` would add it; ~10 lines)
- Linux: **untested.** AppIndicator/StatusNotifier varies by desktop and GNOME
  needs an extension to show tray icons at all.
