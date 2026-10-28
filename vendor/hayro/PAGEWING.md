# Pagewing renderer patch

Source: crates.io `hayro` 0.8.0, https://github.com/LaurenzV/hayro.
Upstream MIT and Apache-2.0 licenses are retained alongside the source.

Pagewing vendors only this renderer crate because its renderer and outline cache
are private and upstream `RenderSettings` has no glyph weight option.
The interpreter, font parsing, and other dependencies remain registry packages.

Local changes are confined to `src/lib.rs` and `src/glyph.rs`:

- Add a default-off `RenderSettings::text_embolden` in the range 0–1.
- Expand filled outline glyphs by a round border (up to 0.025 em per edge).
  Merge the border with the original fill using matching nonzero winding,
  preserving glyph opacity and paint without an additional compositing pass.
- Include weight in outline cache keys so caches can be reused across settings.
- Leave stroke-only/invisible text, Type 3 glyphs, text clipping outlines, images,
  and ordinary vector graphics unchanged.

Configuration and render regression coverage lives in Pagewing's `src/config.rs`,
`src/document.rs`, and `src/worker.rs`. Keep the default-zero path pixel-identical
when updating this copy; remove the patch when upstream offers an equivalent API.
