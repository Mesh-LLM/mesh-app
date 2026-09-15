# Mesh Tray

A native menu-bar tray that runs a private Mesh for you and the people you invite.
No webview. It launches the official prebuilt Mesh executable unchanged — the tray
owns the menus, the pairing journey and the child process, not the runtime.

## Inviting someone

Two cards, like a party invitation.

1. **Members → Invite someone.** The invitation lands in the share sheet; send it
   however you like — Messages, Mail, AirDrop, a USB stick. It grants no access.
2. **They open it and RSVP.** That joins them to you on their side and puts their
   RSVP in their share sheet to send back. Nothing of yours has changed yet.
3. **You open the RSVP and confirm it is really them.** That admits that exact
   identity and connects you. Nothing further is needed from either of you.

A third card is produced when you confirm, and it is optional: it introduces the
new person to the other members of your Mesh. Ignore it and the two of you are
still connected.

Delivery is by file on purpose. It works for family on another network, on a
phone, or with no network at all, and it needs no discovery service.

### What each step does and does not do

- An invitation is a signed offer with your identity and how to reach you. It is
  a file, so it can be forwarded: opening one adds **the inviter only**, never
  the inviter's other members.
- An RSVP proves the sender holds their key and binds their reply to your exact
  invitation. It admits nobody on your side.
- Confirming is the only thing that admits an identity to your Mesh, and only
  the one identity in front of you. A name is not proof; only you know whether
  you asked for it.
- Invitations expire after 30 minutes and are single-decision: Confirm and
  Decline both consume the pending invitation.
- Members you admit can be removed from the Members menu.

## Runtime and identity

`MESH_LLM_BIN` points to the official executable, retaining its adjacent
native-runtimes bundle. The child gets an app-owned HOME and XDG/runtime paths;
inherited `MESH_LLM_*` overrides are stripped, while the GUI keeps the real OS
HOME. The private identity lives at `<app>/home/.mesh-llm/owner-keystore.json`
with a stable keychain account, and Hugging Face caches are shared with yours so
models are not downloaded twice.

On macOS the OS-selected keychain is discovered with
`security default-keychain -d user`, and only `login.keychain-db` is linked into
the app home. No secrets or keychain ACLs are copied or changed. This is storage
separation, **not** an OS security sandbox, and unsigned developer builds can
still raise OS prompts. Missing or corrupt identities are never silently
regenerated.

Ports are overridable with `MESH_LLM_CONSOLE_PORT` / `MESH_LLM_API_PORT`.
Occupied ports are not adopted or stopped, and only children this tray started
are ever terminated. Tests and demos must use a fresh `MESH_APP_DATA_DIR`.

Private model selection follows the same ladder Buzz recommends, classified on
the machine's rated memory rather than what is free at launch: Gemma 4 E4B below
32 GB, Qwen3.5 9B from 32 GB, Qwen3.8 27B from 80 GB, and an error below 8 GB.
It is a total-memory heuristic, not a free-VRAM assurance, and first start can
download weights.

## Developer checks

```sh
just verify # fmt, check, full tests/doctests, all-targets Clippy -D warnings
just build  # release tray executable, no distribution/updater
just clean
# Interactive probes may prompt for keychain permission. Fresh roots only,
# never an existing identity, and not while someone is working:
just profile-probe /absolute/fresh/root
just released-pool-probe /absolute/official/mesh-llm /absolute/fresh/pool-root
```

The identity crate is pinned to an immutable upstream revision rather than a
sibling checkout, and no Mesh SDK or runtime is built here.

## State of it

The pairing logic — accept, confirm, decline, replay, tamper, expiry and seed
preservation — is covered by unit tests. Live multi-node connectivity has been
exercised with official `serve` nodes.

Not verified: the journey clicked end to end on two machines by a human. The
dialog wording, the share-sheet timing and Finder file association want an
attended trial. Windows is launch-gated; Linux is not natively verified.

[HUMAN_TESTING.md](HUMAN_TESTING.md) has the reproducible checksum-verified
macOS arm64 bundle and the manual test procedure.
