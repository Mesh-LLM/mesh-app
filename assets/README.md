# Mesh jellyfish

`mesh-jellyfish.png` is copied unchanged from Mesh-LLM/mesh-llm
`website/src/assets/images/jelly-logo-color.png` at commit 11f4f9cca.
`mesh-jellyfish.rgba` is its 32×32 RGBA raster (Pillow LANCZOS), embedded
without an image-decoder dependency. macOS renders the alpha as a native
template; other platforms retain the original colors.

`Mesh.icns` is the macOS application/Finder icon generated from the same colour
PNG using macOS `sips` (16, 32, 128, 256, 512 pixel sizes, each with a 2× variant)
and `iconutil -c icns`. It is copied into Contents/Resources before signing and
referenced by CFBundleIconFile. The source is 180×180; larger representations are
upscaled, not new higher-resolution artwork. The menu-bar template is unchanged.
