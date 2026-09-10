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
  in-memory state. Opening a file verifies first, then requests explicit consent.
- Shared identity setup via OS credential storage; stable owner marker and exclusive
  profile lock. Missing/corrupt established identities are not regenerated.
- Shared approval/decline/join/remove transitions and persisted approved-reply retry.
  Approval stops/reaps the child, saves the current decision, projects the list,
  then restarts. Reply sharing waits for fresh owned private runtime readiness.
- Native allowed-people submenu and portable open/save/consent adapter. Windows
  runtime launch is deliberately blocked: the pinned runtime uses Windows known
  folders rather than respecting HOME/USERPROFILE for identity/trust paths.
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

Choose Private to create/reuse this app's owner identity with the OS credential
store. Existing unencrypted app identities remain readable; new identities never
silently fall back to plaintext. No CLI identity is imported. Unlock/enable the OS
credential service and retry if unavailable; moved/manual-passphrase identities
still need recovery support. Identity setup is file/keychain coordinated, not
crash-atomic; partial or damaged identities are preserved for recovery.

Request to join creates a signed public request. Open a friend's request while in
Private, confirm their identity through your known conversation, then Allow & reply.
Once the owned child has restarted with verified private identity and ready daemon
state, the share picker opens. The recipient opens the reply and explicitly chooses
Join. Decline durably rejects it; Cancel closes the dialog without deciding.

Share approved reply retries approved responses after failed or cancelled delivery,
including after restart. Up to 32 current replies are retained; approving another
person does not discard an earlier undelivered reply. People allowed lists local entries (claimed
names plus fingerprints); select an entry to remove. Cancel pending requests and
replies does not remove existing grants or recall already-sent files.

macOS uses AppKit sharing. Other desktop adapters use native open/save/dialogs via
rfd, saving a `.meshfile` attachment for your chat app. Linux no-tray fallback also
exposes these controls, Retry startup, and a changing status/error label. Portable
consent handles both native Windows and custom-labeled dialog results; unit tests
exercise that mapping without opening dialogs. Those platform paths are source
implementations, not yet packaged Windows/Linux verification. Windows child launch
remains safely gated. Portable Cancel-default and keyboard behavior still require
native validation; Cancel-default is currently established only in the macOS adapter.

Shared files/pickers are retained until normal app exit, bounded to 32 per session;
service-completion and crash-leftover cleanup are unfinished. Imported files remain
owned by the user. No transport/file-open behavior has been exercised in a launched
app in this checkpoint. A file extension alone is not registration: the application
bundle still needs document/UTI declarations and cold/warm Finder-open checks.
Do not replace winit 0.30.13’s delegate: its implementation requires its own
`WinitApplicationDelegate`, contradicting the custom-delegate example in its docs.

## Required before this draft can leave WIP
- [x] Native keychain-backed private-profile identity setup/load, shared with child.
- [ ] Real OS credential permission/unlock and partial-setup recovery validation.
- [x] Host explicit approval/decline with captured dialog epoch and duplicate handling.
- [x] Stop owned child → save latest consent + list atomically → project/start;
      only then seal/share the response. Surface effective startup failures.
- [x] Joiner explicit consent → consume + owner + connection in one transaction.
- [x] Native admitted-owner list/removal; stale callbacks/files invalidated.
- [x] Save/start failure, stop-before-save, busy serialization and stale-consent tests.
- [ ] Stop timeout, projection fault, OS reentrancy and crash recovery integration tests.
- [ ] Parent-directory sync/power-loss semantics for settings/trust writes.
- [ ] Share cancellation/completion cleanup and crash-leftover recovery.
- [ ] Bundle file types, cold/warm open, real share delivery, keyboard/accessibility checks.
- [ ] Live approve → connect → remove → disconnect → restart → still denied.
- [ ] Forwarded invite held by an unapproved identity stays denied.
- [ ] Resolve Windows runtime identity/trust/node-path isolation before enabling launch.
- [ ] Windows/Linux native behavior and packaging validation.

Do not merge or replace a running preview based on library tests alone.
