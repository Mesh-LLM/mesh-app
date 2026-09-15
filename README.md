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

## Runtime and identity

The official `mesh-llm` executable sits next to the app executable, retaining
its adjacent native-runtimes bundle. It runs as you, against your own
`~/.mesh-llm` — the same profile the plain `mesh-llm` CLI and Buzz use, because
Buzz deliberately keeps one owner identity per machine however Mesh is started
(`desktop/src-tauri/src/mesh_llm/identity.rs:1-8`). There is no app-owned HOME,
no second keystore, no copied trust store and no symlinks. Models are not
downloaded twice because there is only one cache.

Public and Private are flags on the same node, not two identities: `--auto` for
Public, and `--owner-required --trust-policy allowlist` plus one `--trust-owner`
per admitted person for Private. Switching modes keeps your identity, so an
invitation you sent still works afterwards. The roster is declared on the
command line, exactly as Buzz declares its own through the SDK
(`desktop/src-tauri/src/mesh_llm/mod.rs:399-406`), so the tray never edits your
trust store. The engine merges those arguments with the store in memory and
writes nothing back (`mesh-llm-host-runtime/src/runtime/startup_models.rs:141`),
so the allowlist is the union of your machine's trusted owners and the tray's
roster. Forgetting someone in the tray drops the tray's grant at the next start;
if you or Buzz also trusted them in `~/.mesh-llm/trusted-owners.json`, that
machine-level decision stands and is yours to remove with `mesh-llm auth`.

The tray writes two files, ever: `~/.mesh-app/launcher.json` (ports, mode, who
you admitted and what is outstanding) and `~/.mesh-app/mesh.log`. On a machine
with no engine config at all it writes one `~/.mesh-llm/config.toml` on first
run — thinking off, so tray chat answers instead of reasoning into its whole
token budget — and then never touches that file again, whatever you put in it.
If it names `[[models]]` the tray passes no `--model` and the file decides. Occupied ports are not adopted or
stopped, and only children this app started are ever terminated.

Known limitation of one identity: if Buzz's embedded Mesh and this tray are both
sharing at the same time, one node key is live in two processes. Multiple engine
instances per profile are supported — each takes its own
`~/.mesh-llm/runtime/<pid>` with a lock — but that identity being on the mesh
twice at once is not something we have tested. Run one at a time for now.

**Start Over** deletes the launcher state, so the tray trusts nobody and holds
no outstanding invitations. It does not touch your machine's Mesh identity,
which the CLI and Buzz share: resetting that is `rm -rf ~/.mesh-llm`, and it
resets them too.

Private model selection is two Gemma picks, classified on the machine's rated
memory rather than what is free at launch: Gemma 4 E4B below 128 GiB, the 26B
MoE above it, and an error below 8 GiB. Both answer without visible
chain-of-thought, which is why they are the picks — the tray no longer writes
engine defaults to say so. It is a total-memory heuristic, not a free-VRAM
assurance, and first start can download weights.

## Developer checks

```sh
just verify # fmt, check, full tests/doctests, all-targets Clippy -D warnings
just build  # release tray executable, no distribution/updater
just clean
# Prints the owner identity in a profile directory, creating one if that
# directory is new. May prompt for keychain permission:
just profile-probe /absolute/profile/root
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
