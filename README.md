# Mesh tray — automatic-default native preview

Native menu, no webview. macOS and Windows use winit; Linux uses GTK with a
small fallback window when no StatusNotifierWatcher is present.

## Behavior

Opening the app starts the adjacent `mesh-llm` with real `serve --auto` and opens
`/chat`. Menu: status, Open Chat, Settings, Quit. Pending pairing requests add a
review shortcut; startup errors add Retry. Settings opens `/configuration/mesh`
in the existing console; Advanced stays at `/configuration`.

The app uses a persistent isolated home under `~/.mesh-tray/home`, runtime root
under `~/.mesh-tray/runtime`, and ports 3232 (console) / 9447 (API). It refuses
busy ports, does not adopt another service, and only signals its retained child.
On Windows it checks the child PID against management metadata before requesting
shutdown. Quit waits for the child to exit and leaves the app open on timeout.

`MESH_TRAY_DATA_DIR`, `MESH_LLM_BIN`, `MESH_LLM_CONSOLE_PORT`, and
`MESH_LLM_API_PORT` are development overrides. A developer-authored `launcher.json`
can choose `connection: {"mode":"private","invite":null}` or an existing legacy
invitation. That is NOT the approved pairing UX. Invalid settings fail closed;
private choice never adds --auto or --publish. There is no preferences writer yet.

## Build

Run `just verify` then `just build` (stable Rust), `just clean` after preserving
artifacts. Place the normal product host and its adjacent native-runtimes tree
beside mesh-tray. Build the product through its own `just release-build` for
an app handoff: this packages the real console, not the debug designer harness.
Do not replace the dynamic host with a static shortcut.

## Not complete

- Console mode switching, approved-access persistence/removal, and guided owner
  setup are not implemented. Fresh advanced configuration is read-only until
  existing owner setup is completed.
- Nearby mDNS discovery is still coupled to LAN-only runtime transport. The
  intended nearby + internet private flow is not wired. Existing pairing UI
  requires both-party matching-code approval but is not a certified per-peer
  revocation boundary.
- Ollama/LM Studio sharing and agent setup are not implemented here.
- Windows/Linux paths exist but have not been built/run in this verification.
  Linux needs GTK3 and libayatana-appindicator or libappindicator.
- Native menu clicks require manual verification; tests cover lifecycle and
  state logic, not OS menu interaction. A returned model listing is availability,
  not proof that every listed model can answer.
- Local macOS bundle is development-signed, not a notarized distribution/installer.

## Verification

Whole-package tests (8), fmt, check, and all-targets Clippy with warnings denied.
Console whole suite: 1665 passed, 3 skipped; typecheck, ESLint/Prettier and normal
product build passed. Test stderr includes React/chart warnings. Product linking
warns about native object deployment target 26.5 versus host minimum 11.0; this
preview does not establish support for older macOS versions.

Local workflow: Finder-style app launch, isolated auto join, loopback management
ownership, Settings deep link and model shortcut, desktop/mobile console, and a
real browser chat completing with OK. No local model download. Native screen
capture/accessibility was unavailable, so tray clicks are not claimed tested.
