# Local payments preparation

Status: draft; local-only. App base: `63303ba1e42ac1fc7ea6259693106186a662d5ab`.
Engine inspected: `gu/wallet-plugin` at `6764a7f132abbc613cf8e2a95ced39904cf5a47e`.
Upstream PR: https://github.com/Mesh-LLM/mesh-llm/pull/1926 (still open at inspection).

## Product scope

Add a Payments entry to the native tray. Prefer native controls to preserve DESIGN.md's no-webview/no-extra-settings-website constraint. Existing README/DESIGN still describe some pre-embedded ownership; actual lifecycle.rs owns the embedded SDK. Do not copy old child-process assumptions into this feature.

Separate two controls, not one ambiguous master switch:

- **Pay for inference**: off by default; enabling requires an explicit positive daily spending limit. Disable maps to free_only, not wallet deletion. Funding never enables spending.
- **Charge for serving**: per-model input/output prices and minimum invoice; disabling removes that model's pricing. Clarify that turning charging off offers that model free, rather than stopping serving.
- **Wallet**: balance and funding remain accessible while paid inference is off. Distinguish wallet unavailable from zero balance.
- **Add funds**: amount in sats, create a Lightning invoice, copy invoice and show QR plus expiry. Explain: “Pay this Lightning invoice from a wallet or exchange that supports Bitcoin Lightning withdrawals.” Coinbase is an example only if that account/product supports Lightning; do not imply an on-chain Bitcoin address or universal exchange support. Creating an invoice is not proof of receipt; refresh balance after payment.

Use integer msat in the API, explicit sat units in UI, checked conversion and validation. No floating-point money.

## Verified API mapping at engine revision above

POST loopback `/api/wallet`, using ControlCommand tagged JSON (`command`).

| UI action | Command |
|---|---|
| Read balance | balance |
| Read spending policy + usage | policy, value: null |
| Free only | policy, value: {mode: free_only, daily_budget_msat: ...} |
| Enable spending | policy, value: {mode: automatic, daily_budget_msat: positive integer} |
| Read seller prices | pricing |
| Set/remove model price | set_pricing, model, value: Pricing or null |
| Generate funding invoice | fund, amount_msat |

Sources: crates/mesh-llm-payments/src/{control.rs,ledger.rs,pricing.rs,intent.rs}; crates/mesh-llm-host-runtime/src/api/routes/wallet.rs.

Daily budget is engine enforced during transactional approval, with existing reservations deducted. Day boundaries are UTC epoch-day boundaries (`now_ms / 86_400_000`), not a rolling 24h allowance or local midnight. Policy status returns spent_today_msat, reserved_msat and remaining_daily_budget_msat. Avoid promising instantaneous cancellation of already authorized/in-flight settlements when switching off; verify that behavior independently.

Important corrections to earlier discussion: current Policy holds mode + daily budget, not stored per-model buyer rate caps. intent.rs derives eligibility from that policy, with unrestricted input/output rate caps and the daily budget as total cap; request intent can tighten it. Do not invent editable persistent rate caps. expected_pid is optional at the server boundary, but the app must always send its retained runtime PID; expected_directory can additionally constrain the ledger. A mismatch yields 409: invalidate snapshot, refresh and require user reconfirmation, never automatically replay a write.

## Dependency/backend seam

Current app pins three engine deps to d2fc780f; inspected plugin revision contains that commit. Update all three immutable pins and Cargo.lock together when implementing; enable SDK payments. SDK payments provides no wallet backend. Establish the supported wallet.v1 plugin launch/package path separately before claiming balance/funding work. Do not silently add direct Lexe linkage or assume a CLI helper exists in the embedded bundle.

## Implementation sequence

1. Independent review received from Thinker (`RESEARCH/TRAY_PAYMENT_UX_PLAN.md`, PR #1926 at 2dad8653). It agrees on the three sections and confirms lazy provisioning on balance reads. Compare the existing companion `benthecarman/mesh-app` branch `lightning-wallet` before implementing, to reuse suitable work rather than duplicate it. Verify its compatibility with the newer plugin branch; do not assume it. Match invoice receipt by payment hash, not generic balance growth. Initially restrict paid serving to supported single-node text models. Old authorizations can settle after a UTC rollover or disabling/lowering the budget, so the UI must describe an authorization allowance, not an unconditional cap on all debits settling today. No withdraw/send UI in the first slice.
2. Typed app payments command/view model and bounded, redirect-free loopback client; mocked HTTP tests only. Preserve last-known values as stale, never optimistic success.
3. Native Payments entry and forms: read-only policy/balance, explicit save for spending cap and seller prices, invoice display/copy/QR. All HTTP work off main/UI thread.
4. Pin dependency/backend integration and build in isolated task output. No automatic wallet creation from background polling without first-use review.
5. Full package tests, formatter and all-target Clippy; fake backend for unavailable/timeout/409/invalid amount/expired invoice/restart/stale responses. Then separately authorized disposable runtime test. No Keychain prompts, real funding or spending in this preparation.

No push, PR, release, paid cloud resources, live wallet mutation or installed-app replacement authorized by this local preparation task.

## Controller boundary and test progression (Mic's direction)

The tray is a thin controller over Mesh, not a payment implementation. Prefer
SDK operations; use the existing local operator API where the SDK has no typed
operation. No app-owned ledger, seed handling, settlement or budget engine.
Provider packaging is a Mesh integration concern; do not build a new wallet
manager into this app. Companion native presentation may be reusable, but its
manual/approve/reject controller is obsolete against the inspected backend.

Use the channel's latest live evidence, not the older backup trial:
`RESEARCH/WALLET_PLUGIN_LIVE_2026_09_22.md` records d65dff065, existing funded
profiles, wallet-lexe pins, 4-sat total debit, restart reconciliation and final
free-only policy. A backup is recovery material, not a fresh independent wallet.
Never overwrite current ledger/pins from an old snapshot or run concurrent copies
of one funded wallet. Check profile ownership and current state before any reuse.

Validation progression:
1. Fake loopback contract tests; no profile or wallet access.
2. Native controls over mocked Mesh responses; no live wallet creation.
3. Supported Mesh runtime configured to the preserved test profile, after checking
   its existing wallet/provider identity and exclusive ownership. Start free-only.
   Do not rename providers to sidestep an existing pin.
4. Explicit funding-invoice test; invoice generation is not funds received. Current
   host API lacks targeted lookup/status projection: show unknown rather than
   infer absence from a bounded transaction list. No automatic invoice recreation.
5. Separately agreed tiny paid-inference cap; fail-fast policy update and readback
   before requests. Match payer/provider hashes and reconcile reservations, then
   return to free-only and stop only owned test processes.

Local first slice: src/payments.rs and src/payments/tests.rs implement request
serialization, PID pinning, bounded loopback HTTP without redirects/write retries,
exact whole-sat inputs and authoritative response types. Not yet connected to the
tray menu, no dependency pin changed and no live integration claimed.
