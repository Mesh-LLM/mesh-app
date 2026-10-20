# Mesh desktop launcher

Run [Mesh](https://github.com/Mesh-LLM/mesh-llm) from your menu bar or system tray.
No command line, no model setup.

## Note this is early access
Some features are still experimental or examples

<img width="533" height="909" alt="image" src="https://github.com/user-attachments/assets/9e81cdd9-22fb-4674-9123-217e110d2f85" />


- **Models set up automatically:** Mesh picks a model that fits your machine and downloads it on first use.
- **Chat:** open the Mesh chat in your browser with **Chat** in the panel or the tray menu.
  The smaller **Console** link opens the Mesh console.
- **Settings live in one panel:** the tray menu only shows status, **Settings…**, **Chat** and **Quit**.

## Public or private

- **Public** joins the open Mesh. Anyone can find it, and there's nothing to share.
- **Private** is your own Mesh. Others join with an invite: open **Settings…**, copy an
  invite and send the code; they paste it under **Join with an invite** in their panel.
  An invite works for 24 hours and gives access to the whole private Mesh.
  It can be forwarded and can't be revoked, so share it only with people you trust.

## Get started

**[Download the latest macOS build (Apple Silicon)](https://github.com/Mesh-LLM/mesh-app/releases/download/dev-latest/Mesh-macos-arm64.dmg)**

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
