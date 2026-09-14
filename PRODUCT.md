# Product

Standalone native Mesh tray. Official prebuilt Mesh v0.76.1, unchanged; no Buzz
runtime, SDK conversion, engine source patch or updater/distribution publication.

- Public retains `serve --auto`. Private and invitation joins use `serve`, select
  a conservative local model, retain previous bootstrap seeds and member grants.
- Members → Invite a member → native share sheet. Recipient Accept & reply sends
  their signed identity bound to the invitation, but grants nothing and does not
  change their serving connection.
- Inviter compares the 80-bit matching code over a known conversation, then Allow
  or Decline. Allow signs that exact acceptance. Final approval must be delivered
  back; only then does the recipient join and admit the signed roster.
- After admission membership is transitive: Jo can invite Oli through the same
  identity check; Mic applies Jo's final approval without another pairwise decision.
- Current transport is files. Replies and final approvals must be shared/opened
  explicitly. Automatic propagation, QR and friendly identity names are unfinished.
- Existing Mesh QUIC and Allowlist enforcement stay intact. A bootstrap token,
  invitation, self-claimed name or acceptance alone is not an admission grant.
- Keep native jellyfish menu, optional Chat and existing console Settings. No
  automatic chat opening. Stop only this app's retained child.
- App-owned subprocess HOME routes trust/node/cache/runtime state. On macOS a
  narrow link to the OS-selected default keychain enables encrypted key unlock;
  no exported passphrases, ACL weakening, personal Mesh state or identity replacement.

## Current acceptance gate

Full package checks and release tray build pass. Official runtime encrypted-owner
startup and a three-full-node networking probe passed before the final confirmation
correction. Corrected Invite/Accept/Allow/transitive approval flow passes unit tests;
its live probe is updated but deliberately NOT rerun after Mic reported keychain
interruptions. No further native launches/keychain calls until that UX is resolved.
No inference, screenshot, native delivery or replacement-preview success claimed.
