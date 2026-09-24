# Mesh for desktop

Run [Mesh](https://github.com/Mesh-LLM/mesh-llm) from your menu bar or system tray.
The app runs the Mesh engine for you, so you can use distributed AI and share
compute without managing a command-line server.

- **Public or private:** use the public Mesh or create a private Mesh with people you trust.
- **Invite with a code:** copy an invite and send it, or paste one to join.
- **Chat:** open the Mesh console in your browser from the tray menu.

## Get started

For a macOS app candidate, open its DMG, drag **Mesh.app** into **Applications**,
and launch it. Click the jellyfish in the menu bar to choose Public or Private,
invite someone, or open Chat. Models may need to download on first use.

App distribution is still in preview. Windows tray builds are available as
[CI artifacts](https://github.com/Mesh-LLM/mesh-app/actions/workflows/ci.yml);
Windows UI testing is pending and Linux support is experimental.

**Share private invites only with people you trust.** Codes can be forwarded,
expire after 24 hours for new members, and grant access to the whole private
Mesh. There is no Mesh-wide revoke button.

The app shares your existing Mesh identity and model cache with the CLI and
Buzz. Run only one Mesh app/CLI/Buzz engine at a time for now.

## Development

```sh
just build   # build console assets and the release tray executable
just verify  # fmt, check, full tests and Clippy
just clean
```

See [PRODUCT.md](PRODUCT.md) and [DESIGN.md](DESIGN.md) for product and invite
semantics. The [macOS release workflow](.github/workflows/release-macos.yml)
builds the pinned engine's matching Metal runtime and packages, signs and
notarizes the app. Packaging does not require a separately published engine
release.
