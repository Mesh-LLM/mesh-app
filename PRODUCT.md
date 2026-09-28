# Product

Standalone native Mesh tray. Packaging supplies one explicitly pinned engine
product (source commit or numbered release) and matching native runtime; no Buzz
runtime, SDK conversion or updater/distribution publication. Durable private
restart requires an engine containing mesh-llm #1896; see DESIGN.md.

- Public retains `serve --auto`. Private is `serve --owner-required
  --trust-policy require-owned`, plus `--min-node-version` when this node
  creates the Mesh or `--join <code>` when it joins one, and a conservative
  local model pick.
- Membership is the Mesh, not the person. `require-owned` means every peer with
  a valid owner attestation and the same signed Mesh policy is trusted mutually,
  so there is no allowlist, no `--trust-owner`, no roster and no approval step.
- Invites → Copy an invite puts the signed bearer token on the clipboard. Invites
  → Join with an invite pastes one and restarts to join. Nothing is sent back.
- The code is a 24-hour bearer pass bound to nobody: it can be forwarded, and
  whoever joins is trusted by everybody already in. Only the originating node can
  mint one; a joiner's Invite passes on the code it was given, and an engine that
  can neither mint nor re-share returns an empty token, which the tray reports as
  "ask the person who invited you for a new one".
- No eviction, and nothing claims otherwise: going Public forgets the invite that
  put this node in the Mesh. Each person can refuse someone locally with
  `mesh-llm auth`; the clean kick is re-forming the Mesh.
- The version floor is a deliberate constant, not the bundled version. The Mesh
  ID is the policy hash, so changing it forms a different Mesh, and the engine
  refuses to start Private against a persisted genesis policy whose requirements
  no longer match the flags.
- Existing Mesh QUIC and ownership enforcement stay intact. The tray adds no
  trust decisions of its own and writes nothing to the user's trust store.
- Payments (engine with mesh-llm #1926): balance on the menu title, and three
  items: Pay (daily limit, off by default), Get paid (one price for the served
  model) and Add funds (Lightning invoice, optional amount). Mesh keeps
  all money state; the tray only asks and displays. See DESIGN.md.
- Keep native jellyfish menu, optional Chat and existing console Settings. No
  automatic chat opening. Stop only this app's retained child.
- One machine identity: the child runs as the user against `~/.mesh-llm`, the
  profile the CLI and Buzz already share. No app-owned HOME, no second keystore,
  no trust-store writes and no symlinks. The engine `config.toml` is written
  once, only when the machine has none, and never rewritten afterwards. Public
  and Private are flags on that one node, so switching modes preserves identity.
- Launcher state is `~/.mesh-app/launcher.json` and `mesh.log`, and nothing else.
- One reset concept, not two: changing mode forgets the Mesh being left. No
  separate Start Over item. The machine identity, engine config and models are
  the user's and are never touched.
- Invite reads the running child's verified owner from its own status endpoint
  rather than unlocking the keystore, so it stays one credential prompt per
  launch. The PID check is what ties the code to the child this app started.
- A missing OS file dialog is reported, never a panic: losing the tray also
  orphans the user's running Mesh.
- A config.toml the user has taken over decides the model: when it declares
  `[[models]]` the tray passes no `--model`, because the flag would beat the
  file. Ports and connection mode stay flags.
- Invites persist across ordinary runs; changing mode forgets them after stopping
  the child. Identity persists through both, and downloaded models, the engine
  config and other Mesh nodes are never touched.

## Current human trial

The trust model is proven on the unmodified runtime: four identities on two Macs
all joined with one originator's code, every peer mutually verified, cross-host
inference served both ways (LAN only — NAT traversal between houses is untested).
The tray build carrying it is covered by unit tests, fmt/check/Clippy `-D
warnings`, and is **not yet clicked through in the menu bar by a human**. The engine-level expiry/restart behavior is implemented and was exercised by
the #1896 live matrix; see DESIGN.md. Candidate-specific UI behavior, including
the expired-token message for a new joiner, still needs human verification.

## Private automatic model tiers

On Apple Silicon, Qwen3.8-27B requires at least 128 GiB total RAM and
22 GiB of available-memory budget. Gemma 4 12B is the default on 64-GiB
machines; its floor remains 24 GiB with a 12-GiB budget. Qwen3.5-4B is the
16-GiB tier with a 6-GiB budget. Memory pressure can select a smaller tier.
Non-accelerated platforms retain the small-model recipe.

This is the tray's Private-mode policy only. Public still delegates model
selection to the engine's `serve --auto` policy. An explicit `[[models]]`
configuration takes precedence over the tray's automatic selection.

## Reset contract (supersedes the earlier mode-only reset)

Reset settings is an explicit menu action. It disables paying/charging, waits
for this app's engine to stop successfully, retires private mesh policy and
membership, and restores launcher defaults. Afterwards the user can accept a
new invite or originate a fresh private mesh; old membership is not admission
to that new mesh. Owner credentials, funds/history, config and models survive.
Public remains open discovery. Custom node-key configurations fail
closed rather than resetting another namespace. Keep other apps using the
shared engine profile stopped while resetting. See DESIGN.md for interruption
and failure semantics.

A full Reset requires a healthy owned engine to verify payments are off. If
startup fails, quit/reopen and repair startup first; Reset does not claim a
partial success with spending potentially enabled. An interrupted retirement
marker can be resumed without starting the engine.

The embedded SDK, payment types and matching packaged native runtime are pinned
to the Mesh v0.77.0 release commit `4ae1ace57dbbe28d0c3d10a05ee542328e8e64e7`.
