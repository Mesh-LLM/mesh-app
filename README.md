# Mesh Tray — native sharing draft

A thin native tray for Public/auto or Private Mesh with an explicit, per-node
admitted-owner list. **This is a development draft, not a finished private-join app.**

## What is implemented
- Existing jellyfish tray, Public/Private selection, optional Chat and existing
  console Settings. Launch and file open do not automatically open chat.
- Public keeps `serve --auto`; Private requires owner identity and Allowlist,
  never silently falling back to Public.
- App-owned HOME/runtime paths and retained-child stop/restart behavior.
- Private startup replaces positive trust entries from the saved owner list,
  preserving revocations, so removed grants do not return via the old disk merge.
- Mesh identity library signs both public keys in requests and encrypts responses
  for the recipient. Verification and consent are deliberately separate.
- macOS request sharing with `NSSharingServicePicker`; local `NSOpenPanel` feeds a bounded file verifier.
- Persist pending correlation before sharing; persist cancellation before updating
  in-memory state. Opening a file only displays verification information.
- Immutable verified response fields, expiry/correlation/replay checks, bounded
  local file reads, symlink/FIFO rejection on Unix, 0600 temporary share files.

The rejected companion Settings webpage and custom Bonjour/TCP handoff are not
part of this draft. This adds no Mesh wire protocol or Buzz runtime dependency.

## Build / checks

```sh
just verify  # fmt, check, full package tests/doctests, all-targets Clippy -D warnings
just build   # release executable only, not a packaged app
just clean
```

`mesh-llm-identity` is pinned to upstream commit
`c025263e40bb7fe66b0bbc53bec80ef23f302bd2`, not a sibling checkout. Its identity
sources match the prior reviewed development dependency at `11f4f9cca`.
A compatible Mesh runtime must be supplied beside the tray or with `MESH_LLM_BIN`.
The app owns `~/.mesh-tray` by default; tests can override `MESH_TRAY_DATA_DIR`,
`MESH_LLM_CONSOLE_PORT` and `MESH_LLM_API_PORT`. Never point test runs at real state.

## Native transport checkpoint

For developers with an **existing, unlocked app-profile owner keystore**, Request
to join creates a signed public `.meshrequest` file and presents the OS share
picker. It uses only `<app-data>/home/.mesh-llm/owner-keystore.json`; it never reads
or overwrites a CLI identity. Missing/encrypted identity reports a blocker.

Open Mesh file accepts a signed request or a response matching a persisted request.
The native dialog shows the verified owner but **cannot approve, share a response,
or join yet**. An authentic stranger is not automatically your friend. Cancel
pending requests invalidates local correlation, but cannot recall a sent file.

Shared files/pickers are retained until normal app exit, bounded to 32 per session;
service-completion and crash-leftover cleanup are unfinished. Imported files remain
owned by the user. No transport/file-open behavior has been exercised in a launched
app in this checkpoint. A file extension alone is not registration: the application
bundle still needs document/UTI declarations and cold/warm Finder-open checks.
Do not replace winit 0.30.13’s delegate: its implementation requires its own
`WinitApplicationDelegate`, contradicting the custom-delegate example in its docs.

## Required before this draft can leave WIP
- [ ] Native private-profile identity provisioning/unlock, shared with the child.
- [ ] Host explicit approval/decline with captured dialog epoch and duplicate handling.
- [ ] Stop owned child → save latest consent + list atomically → project/start;
      only then seal/share the response. Surface effective startup failures.
- [ ] Joiner explicit consent → consume + owner + connection in one transaction.
- [ ] Native admitted-owner list/removal; stale callbacks/files cannot restore grants.
- [ ] Save/start/stop failure and stale-dialog controller tests; no rollback to old grants.
- [ ] Parent-directory sync/power-loss semantics for settings/trust writes.
- [ ] Share cancellation/completion cleanup and crash-leftover recovery.
- [ ] Bundle file types, cold/warm open, real share delivery, keyboard/accessibility checks.
- [ ] Live approve → connect → remove → disconnect → restart → still denied.
- [ ] Forwarded invite held by an unapproved identity stays denied.
- [ ] Windows/Linux native behavior and packaging validation.

Do not merge or replace a running preview based on library tests alone.
