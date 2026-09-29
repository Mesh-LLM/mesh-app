# Testing

`just verify` runs what CI runs: fmt, build, unit tests, Clippy `-D warnings`.
Unit tests use a fake wallet server; they check the requests the tray sends and
how it handles bad responses, not what the real engine returns.

## Real engine wallet test (manual, not in CI)

```sh
MESH_TRAY_WALLET_E2E=1 cargo test --test isolated_wallet
```

About 3 s. Starts the embedded engine client-only (no model) with a temp config
dir and a temp `HOME`, no mesh join, no publishing, no relays, then drives every
wallet command the tray sends: policy read/save, pricing save/read, balance
(expects 0), fund (expects a 10-sat `lnbc` invoice) and transactions. Nothing
in `~/.mesh-llm` or `~/.mesh-app` changes.

Each run provisions a fresh, empty **mainnet** Lexe wallet over the network and
pays nothing, so it needs Lexe to be reachable. That is why it is not in CI.
Whether a Lexe testnet or regtest would work as well is untested. Without
`MESH_TRAY_WALLET_E2E=1` the test prints `skipped`.

Run it before a release and whenever the pinned engine changes.

## Manual checks on a packaged build (~10 min)

Use a clean macOS user or move `~/.mesh-llm` and `~/.mesh-app` aside first.

1. Install and launch. Click **Always Allow** on the keychain prompt. The menu
   status reaches ready.
2. Untick **Share compute**: the model unloads and the node stays joined.
   Tick it again: the model reloads. The tick matches the state.
3. Payments: **Get paid** shows an invoice/QR, **Add funds** shows the amount
   form. If funds are available, send a small payment between two machines and
   confirm it is received.
4. Quit and relaunch: no second keychain prompt, and settings persist.
5. Reset: paying/charging are off, launcher defaults are restored, and private
   policy/membership are retired. Owner credentials, wallet/history, config and
   models remain. Create a new private mesh and verify old members cannot enter;
   separately accept a new invite after Reset. Never remove the real profile as
   part of agent validation.

## Private reset regression evidence

The pinned v0.77.0 (`4ae1ace57`) engine has a separate test-only proof on
[`dario/private-reset-proof`](https://github.com/Mesh-LLM/mesh-llm/commit/5ce1ff824d353588d3f6f5779549c8a632877a44):
`mesh/tests/admission/requirements.rs::assert_private_reset_retires_old_members_and_invites`.
It deletes the same four files with a fixed in-memory test owner and temporary
HOME, then exercises real peer admission: a new policy/mesh ID, old-member
`MeshPolicyMismatch`, stale invite rejection and fresh invite acceptance.
It does not touch Keychain or prove the native menu journey. Tray package tests
separately assert byte-preservation of credential/wallet/config fixtures,
interrupted retirement, invalid file types, failed settings save, payment-off
verification, failed shutdown and Quit while a transition is pending.

Dario ran the full host-runtime package at `5ce1ff824`: 4072 library tests
passed, 11 ignored, plus all integration binaries (25 tests) passed. This is
the release source plus the test-only proof, not a new engine dependency.

## Embedded reset/restart regression

`just verify` also runs `embedded_restart`, a real engine test using the tray's
lifecycle, payment-reset and private-state retirement functions. It runs three
start/reset/stop cycles in one process against a fresh profile, verifies the
profile payment database, free-only policy and empty pricing, preserves a
synthetic wallet-ID and seed-file fixtures byte-for-byte, and checks config-copy cleanup. It
does not provision a wallet, access Keychain, or establish a real balance.

For manual remote inference evidence, `MESH_TRAY_RESTART_MODEL` selects an
already-cached local model and pauses each cycle for 20 seconds after reset.
The test prints its API/console ports; join a separate node using its status
invite and request the exact advertised model before and after restart. Use
the matching packaged native runtime. This is additional evidence, not a
claim that the native menu was clicked.
