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
Use HUMAN_TESTING.md.
