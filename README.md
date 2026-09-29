# Mesh for desktop

Run [Mesh](https://github.com/Mesh-LLM/mesh-llm) from your menu bar or system tray.
No command line, no model setup.

<!-- screenshot -->

- **Models set up automatically:** Mesh picks a model that fits your machine and downloads it on first use.
- **Built-in wallet:** pay for models over Lightning, earn from the one you serve, and add funds with a QR code.
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
