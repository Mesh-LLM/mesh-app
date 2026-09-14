# Mesh Tray — standalone private pooling development checkpoint

Native jellyfish tray using the unchanged official prebuilt Mesh **v0.76.1**.
This checkpoint implements the invitation state machine; it is **not yet the
ready-to-test replacement preview**. The existing preview and identity are untouched.

## Using Members

1. Choose Private. The app serves a locally selected model; joins also keep serving.
2. Members → Invite a member. Send the invitation to your friend with native sharing.
3. They open it via Members → Open invitation or reply, choose **Accept & reply**,
   and send back the reply. Their connection and grants are still unchanged.
4. Check with your friend outside Mesh that the reply is theirs. The optional
   code can help; it is not a mandatory ceremony. The inviter chooses **Allow** or Decline. A name alone is not proof of identity.
5. Once restarted/ready, Members → Share reply or approval sends the final approval.
   The recipient opens it to join. Existing members open that same approval to
   admit the new member transitively, without another pairwise approval.

**File delivery is manual in this checkpoint.** Automatic receipt propagation,
link/QR transport, cold/warm Finder file association and friendly names remain work.
The old request/reply adapter remains under Members → Legacy request exchange for
existing saved state; it is not the new invitation journey.

Invitations expire after 30 minutes and are single-decision on the inviter. Both
Decline and Allow consume the local pending invitation. Acceptances alone cannot
admit anybody; signed final approvals bind the exact invitation and recipient.
Issued invitations, pending acceptance, outgoing reply/approval and consumed grant
IDs persist across restarts. Only the latest outgoing membership file is retained;
finish delivery before starting another acceptance/approval. Cancel pending exchanges
clears pending membership work, not established member grants.

## Runtime and identity

`MESH_LLM_BIN` points to the verified official executable, retaining its adjacent
native-runtimes bundle. `--profile-dir` is no longer passed. The child gets an
app-owned HOME and XDG/runtime paths; inherited MESH_LLM overrides are stripped.
The GUI keeps real OS HOME. Private identity remains at the established absolute
`<app>/home/.mesh-llm/owner-keystore.json` path with its stable keychain account.

On macOS `security default-keychain -d user` discovers the OS-selected keychain;
only `<app-home>/Library/Keychains/login.keychain-db` is linked to that file. No
secrets or keychain ACLs are copied/changed. Existing different routing fails
without overwriting. This is storage separation, **not an OS security sandbox**.
OS prompts are still possible with unsigned/changing developer executables: this
was observed during probes and is an unresolved UX gate. Further interactive
probes stopped on Mic's request. Missing/corrupt established identities are never
silently regenerated. Windows remains launch-gated; Linux is not native-verified.

Public now uses `<app>/public-home/.mesh-llm`; an established old development
`public/key` blocks automatic migration rather than silently replacing that identity.
Tests/demos must use a fresh `MESH_TRAY_DATA_DIR`, not the retained preview profile.
Ports are overridable with MESH_LLM_CONSOLE_PORT / MESH_LLM_API_PORT. Occupied ports
are not adopted or stopped. Only retained children are terminated.

Private model selection: Qwen2.5 3B Q4_K_M for 8–23 GiB RAM; Qwen3.5 9B Q4_K_M for
24+ GiB. Below 8 GiB the automatic selection errors. This is a total-memory
heuristic, not free GPU memory assurance. Joins retain serving and original seeds;
first start can download weights. `ready_idle` is not inference readiness.

## Developer checks

```sh
just verify # fmt, check, full tests/doctests, all-targets Clippy -D warnings
just build  # release tray executable, no distribution/updater
just clean
# Interactive probes below may prompt for keychain permission. Do not run while
# the user is working; fresh roots only, never existing identities:
just profile-probe /absolute/fresh/root
just released-pool-probe /absolute/official/mesh-llm /absolute/fresh/pool-root
```

The identity-only crate remains pinned to immutable upstream d4ffbbacd9c8486e0b80c44452876ffa17785964,
not a sibling checkout; no embedded Mesh SDK/runtime build. The wire identity
format was exercised with official 0.76.1.

## Verified / not verified

- Full checks: 46 library + 15 executable + 2 compile-fail doctests passed; release
  build passed, no warning suppression added.
- Fresh encrypted owner unlocked in official runtime under app HOME; `/api/status`
  reported version 0.76.1 and verified owner. No plaintext/env/argv secret handoff.
- Three official `serve` nodes, distinct owners, two peers each after onward
  membership and restarts. This used the earlier bearer-consent draft; it proves
  runtime connectivity, NOT the corrected identity-confirmation journey.
- Corrected journey and decline/replay/tamper/expiry/seed preservation pass unit
  tests. Updated three-node probe compiles but is not rerun after the prompt report.
- Native tray process launched. Screen capture unavailable (`could not create image
  from display`). No click/share-service/screenshots/real chat demonstrated.
- No replacement preview, distribution, updater, public release or independent
  review completion claimed. Review requested; outcome pending.

Keep the established preview unchanged until the no-interruption credential UX,
corrected live journey, file activation/delivery, inference and independent review
are complete. Temporary probe credentials/profile artifacts are retained for
scoped cleanup without triggering further OS dialogs; do not remove user models
or established identities as part of build cleanup.

## Local candidate packaging

See [HUMAN_TESTING.md](HUMAN_TESTING.md) for the reproducible checksum-verified
macOS arm64 bundle and manual test procedure. The bundle is not launched during
verification; native credential access remains an explicit unresolved gate.
