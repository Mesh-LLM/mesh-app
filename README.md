# Mesh for desktop - expermental/demo app 

Run [Mesh](https://github.com/Mesh-LLM/mesh-llm) from your menu bar or system tray.
No command line, no model setup.

## Note this is early access
Some features are still experimental or examples

<!-- screenshot -->

- **Models set up automatically:** Mesh picks a model that fits your machine and downloads it on first use.
- **Chat:** open the Mesh chat in your browser from the tray menu.

## Public or private

- **Public** joins the open Mesh. Anyone can find it, and there's nothing to share.
- **Private** is your own Mesh. Others join with an invite: choose **Invite → Copy an invite**
  and send the code, and they paste it with **Invite → Join with an invite**.
  An invite works for 24 hours and gives access to the whole private Mesh.
  It can be forwarded and can't be revoked, so share it only with people you trust.

## Get started

On macOS, open the DMG, drag **Mesh.app** into **Applications**, and launch it.
Click the jellyfish in the menu bar. The app is in preview. Windows builds are
[CI artifacts](https://github.com/Mesh-LLM/mesh-app/actions/workflows/ci.yml), and Linux is experimental.

## Development

```sh
just build
just verify
just clean
```

See [PRODUCT.md](PRODUCT.md) and [DESIGN.md](DESIGN.md) for details.

## Wallet plugin setup

Mesh handles pricing, spending limits and settlement; an installed `wallet.v1`
plugin handles the Lightning wallet. The app enables the engine's payments
support but does not bundle or compile in Lexe.

For a reproducible setup, use the Mesh CLI built from this app's engine revision
`89db585319fc04d7d50530900e466502273f1b68`. No numbered Mesh release containing
that engine change is claimed here. The v0.77.0 CLI already has `plugins install`,
but running its older engine is not equivalent to this app's external-wallet
engine. Install the native plugin as the same OS user who runs the app, then
quit and reopen the app:

```sh
mesh-llm plugins install Mesh-LLM/lexe-wallet
```

The app and CLI use the same `~/.mesh-llm/plugins` store (unless
`MESH_LLM_PLUGIN_DIR` overrides it). Installed, enabled providers are discovered
by the engine; no executable path or extra config stanza is needed. Keep just
one wallet provider enabled unless you explicitly configure provider selection.
Use **Payments → Add funds** to create a Lightning invoice and pay it from
another wallet. Separately enable **Pay** and set a daily limit when you want
paid inference. Installation and funding do not authorize spending.

For CLI control of the app's default management port:

```sh
mesh-llm wallet --port 3232 fund-wallet --amount-sats 1000
mesh-llm wallet --port 3232 policy --mode automatic --daily-budget-sats 100
mesh-llm wallet --port 3232 policy --mode free-only
```

**Existing wallet warning:** [Lexe v0.1.0](https://github.com/Mesh-LLM/lexe-wallet/releases/tag/v0.1.0)
is for fresh/unpinned profiles. An existing `wallet-lexe` provider pin is not an
automatic upgrade to `lexe-wallet`. This app does not migrate, delete, or rewrite
wallet pins, recovery seeds, or payment history. Preserve the old wallet and use
a separately validated migration procedure before switching a funded profile.
Do not remove a pin to suppress an identity mismatch. The engine error may
suggest `mesh-llm wallet unpin`; that is **not a migration procedure** and must
not be followed merely to make an old funded wallet open with this new provider.
A missing/disabled or
mismatched provider yields a wallet error, not a fallback wallet. Free Mesh use
does not require a wallet plugin. Plugin executables are trusted local code,
not sandboxed payment tools.
