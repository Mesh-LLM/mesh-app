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

The pinned `d06600e` engine has a separate test-only proof on
[`dario/private-reset-proof`](https://github.com/Mesh-LLM/mesh-llm/commit/25a65ac19):
`mesh/tests/admission/requirements.rs::assert_private_reset_retires_old_members_and_invites`.
It deletes the same four files with a fixed in-memory test owner and temporary
HOME, then exercises real peer admission: a new policy/mesh ID, old-member
`MeshPolicyMismatch`, stale invite rejection and fresh invite acceptance.
It does not touch Keychain or prove the native menu journey. Tray package tests
separately assert byte-preservation of credential/wallet/config fixtures,
interrupted retirement, invalid file types, failed settings save, payment-off
verification, failed shutdown and Quit while a transition is pending.
