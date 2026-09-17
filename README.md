# Mesh Tray

A native menu-bar tray that runs a private Mesh for you and the people you invite.
No webview. It launches the official prebuilt Mesh executable unchanged — the tray
owns the menus, the invite and the child process, not the runtime.

## Inviting someone

One code. You mint it, they paste it, they are in.

1. **Invites → Copy an invite.** The code is on your clipboard; send it however
   you like — Messages, Mail, a note read out loud.
2. **They choose Invites → Join with an invite** and paste it. Mesh restarts and
   they are in.
3. **Nothing comes back.** No reply to open, nothing to confirm, no list to keep.

Joining with another invite replaces the saved invite set; it does not add a
second mesh. Ordinary Quit → reopen retains the selected mesh. Ports, engine
settings and downloaded models are preserved when selecting another invite.

Everyone in the Mesh can use everyone's machines, including people you never
handed a code to yourself: they can forward yours on, and whoever joins is
trusted by everybody already in. The unit of trust is the Mesh, not the person.

### What the code is and what it costs

- It is a signed bearer token: your node's address plus the signed Mesh policy,
  good for 24 hours. Holding it within the day is membership.
- **It is not bound to anybody.** Forward it to five people and all five can
  join. That is what makes the Mesh grow without you in the middle.
- **Only the node that created the Mesh can mint one.** Someone who joined can
  pass on the code they were given, and when it lapses their Invite says to ask
  you for a new one. So you have to be running for anyone new to arrive.
- **You cannot take it back.** There is no Mesh-wide eviction: each person can
  refuse someone on their own machine with `mesh-llm auth`, and the clean kick is
  re-forming the Mesh, which changes its ID and means everyone re-pastes.
- Asking again gives a fresh 24 hours for the **same** Mesh, because the Mesh ID
  is the hash of the policy and the policy has not changed.
## Runtime and identity

The official `mesh-llm` executable sits next to the app executable, retaining
its adjacent native-runtimes bundle. It runs as you, against your own
`~/.mesh-llm` — the same profile the plain `mesh-llm` CLI and Buzz use, because
Buzz deliberately keeps one owner identity per machine however Mesh is started
(`desktop/src-tauri/src/mesh_llm/identity.rs:1-8`). There is no app-owned HOME,
no second keystore, no copied trust store and no symlinks. Models are not
downloaded twice because there is only one cache.

Public and Private are flags on the same node, not two identities: `--auto` for
Public, and `--owner-required --trust-policy require-owned` for Private, plus
either `--min-node-version` when this node creates the Mesh or `--join <code>`
when it joins one. Switching modes keeps your identity — it is the Mesh, not the
identity, that going Public gives up.

`require-owned` is what makes it a Mesh rather than a star: the tray keeps no
allowlist and passes no `--trust-owner`, so every peer with a valid owner
attestation and the same signed Mesh policy is trusted mutually
(`mesh-llm-host-runtime/src/mesh/ownership.rs:355-416`). The version floor is the
one requirement the tray sets, and it is not really about versions: any
requirement makes the Mesh requirement-aware, which is what turns its invite
into the signed 24-hour bearer token only the originator can mint
(`mesh/node_identity.rs:106-185`). Without it the Mesh is unrestricted and the
code degrades to an unsigned address token with no expiry. The floor is a
deliberate constant in `settings.rs`, not the bundled version, because the Mesh
ID is the hash of the policy: changing it forms a different Mesh and everyone
re-pastes.

The tray writes two files, ever: `~/.mesh-app/launcher.json` (ports, mode and
the invites this node joined with) and `~/.mesh-app/mesh.log`. On a machine
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

There is no separate "start over", because switching Mesh is starting over:
going Public forgets the invite that put you in the Mesh, and starting a Private
Mesh starts your own with nobody in it. It is forgetting, not revoking — people
holding a valid code to a Mesh you created can still use it. Your machine's Mesh identity is
not part of that and does not change — it is shared with the CLI and Buzz, and
resetting it is `rm -rf ~/.mesh-llm`, which resets them too.

Private model selection uses total installed memory in whole GiB: Gemma 4 E4B
(Q4_K_M) at 8–64 GiB, Gemma 4 26B-A4B (UD-Q4_K_M) at 65–127 GiB,
and Qwen3.8 27B (UD-Q4_K_M) at 128 GiB and above. Below 8 GiB it reports
insufficient memory. Configured startup models override these recommendations.
This is not a free-memory or dedicated-GPU-VRAM fit guarantee; context cache,
runtime buffers and other applications also need memory. First start can download
weights. The Qwen tier uses the quant selected in the attended 128-GiB Mac trial.

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

The trust model itself is proven on the unmodified 0.76.2 runtime: four
identities across two Macs, all joined with one originator's code, every peer
mutually verified, and cross-host inference served both ways
(`RESEARCH/MESH_BEARER_INVITE_REQUIRE_OWNED_TEST_20260915.md` in the Buzz nest).
That was a LAN test, so it proves trust and routing, not NAT traversal between
houses. The flags the tray launches, the invite copy and the join restart are
covered by unit tests.

Not verified: the journey clicked end to end in the menu bar by a human, and
what happens on restart when a code has since expired. Windows is launch-gated;
Linux is not natively verified.

[HUMAN_TESTING.md](HUMAN_TESTING.md) has the reproducible checksum-verified
macOS arm64 bundle and the manual test procedure.

### Testing an unreleased engine fix

The official preview packager still defaults to the pinned 0.76.2 archive.
For a locally composed source product, explicitly record the full engine commit
and both input checksums instead (Python 3.12+):

```sh
python3 scripts/package-preview.py target/release/mesh-tray \
  /absolute/source-product.tar.gz /absolute/new-source-candidate \
  --source-commit FULL_40_CHARACTER_ENGINE_COMMIT \
  --archive-sha256 ARCHIVE_SHA256 --host-sha256 HOST_SHA256
```

Build the host and matching native runtime from that same engine checkout using
its documented `just` product-build commands. Hash the archive and its contained
`mesh-bundle/mesh-llm`, not a different host or an installed app. The pins are
caller-supplied provenance, not release authentication. Packaging executes neither
binary, preserves the adjacent runtime, and labels the handoff as a source trial.
Do not change `MIN_NODE_VERSION` to match a newer binary: it defines the Mesh ID.

Engine PR [#1896](https://github.com/Mesh-LLM/mesh-llm/pull/1896) supplies durable
joiner membership after invite expiry. Until tested with that fix, do not promise
that the pinned 0.76.2 preview survives an expired invite on restart. For a source
candidate, verify joiner restart after expiry, owner restart, two joiners restarting
with the owner offline, and rejection of a fresh node with the expired code.
The tray must retain the original invite as the engine's mesh-selection hint;
it must not discard it merely because its admission window has elapsed.
