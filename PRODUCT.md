# Product

Standalone native Mesh tray. Official prebuilt Mesh v0.76.2, unchanged; no Buzz
runtime, SDK conversion, engine source patch or updater/distribution publication.

- Public retains `serve --auto`. Private and invitation joins use `serve`, select
  a conservative local model, retain previous bootstrap seeds and member grants.
- Members → Invite a member → native share sheet. Recipient Accept & reply sends
  their signed identity bound to the invitation, but grants nothing and does not
  change their serving connection.
- Inviter checks with their friend outside Mesh, best efforts, then Allow or
  Decline. The 80-bit reference code is optional, not a required ceremony. Allow signs that exact acceptance. Final approval must be delivered
  back; only then does the recipient join and admit the signed roster.
- After admission membership is transitive: Jo can invite Oli through the same
  identity check; Mic applies Jo's final approval without another pairwise decision.
- Current transport is files. Replies and final approvals must be shared/opened
  explicitly. Automatic propagation, QR and friendly identity names are unfinished.
- Existing Mesh QUIC and Allowlist enforcement stay intact. A bootstrap token,
  invitation, self-claimed name or acceptance alone is not an admission grant.
- Keep native jellyfish menu, optional Chat and existing console Settings. No
  automatic chat opening. Stop only this app's retained child.
- One machine identity: the child runs as the user against `~/.mesh-llm`, the
  profile the CLI and Buzz already share. No app-owned HOME, no second keystore,
  no trust-store writes and no symlinks. The engine `config.toml` is written
  once, only when the machine has none, and never rewritten afterwards. Public
  and Private are flags on that one node, so switching modes preserves identity
  and outstanding invitations. The admitted roster is passed as `--trust-owner`
  arguments and merged in memory with the machine's trust store, which the
  serving runtime never writes. The effective allowlist is that union, so
  forgetting someone in the tray does not revoke a grant the user or Buzz made
  in `~/.mesh-llm/trusted-owners.json`.
- Launcher state is `~/.mesh-app/launcher.json` and `mesh.log`, and nothing else.
- One reset concept, not two: changing mode forgets the Mesh being left -- the
  trusted list, outstanding invitations and seeds. No separate Start Over item.
  The machine identity, engine config and models are the user's and are never
  touched.
- One credential prompt per launch, not two: startup verifies the profile's
  identity from public keystore metadata (owner id checked against the signing
  key) and never unlocks the secret. Only the runtime child unlocks it, plus the
  explicit invite/share actions the user just clicked.
- A missing OS file dialog is reported, never a panic: losing the tray also
  orphans the user's running Mesh.
- Members menu has exactly two items: invite someone, or accept an invitation or
  RSVP that was pasted in. Every card is copied to the clipboard the moment it
  exists, so there is no retry item; a card that never arrived is replaced by
  inviting again. No roster and no per-person remove in the menu: the console
  lists members, and going Public is the only revoke on this build.
- A config.toml the user has taken over decides the model: when it declares
  `[[models]]` the tray passes no `--model`, because the flag would beat the
  file. Ports, connection mode and the allowlist stay flags -- the console port
  has no config key and the mode is what the radio buttons mean.
- Pairings persist across ordinary runs; changing mode forgets them after
  stopping the child. Identity persists through both, and downloaded models, the
  engine config and other Mesh nodes are never touched.

## Current human trial

Full package checks and release tray build pass. Official runtime encrypted-owner
startup and a three-full-node networking probe passed before the final confirmation
correction. Corrected Invite/Accept/Allow/transitive approval flow passes unit tests;
its live probe is updated but not rerun unattended. Mic cleared the prior
zero-Keychain-prompts blocker: ordinary attended first-run authorization is allowed.
Use HUMAN_TESTING.md; no engine change or zero-prompt guarantee is required.
No inference, screenshot, native delivery or replacement-preview success claimed.
