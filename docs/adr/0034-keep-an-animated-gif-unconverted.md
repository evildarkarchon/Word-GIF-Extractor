# Keep an animated GIF unconverted

The 2026-11-14 audit (L7) found that `--convert` turned an animated GIF into its first frame. Every conversion target is encoded from one still image, decoded with `image::load_from_memory`, which reads a GIF's first frame only. A module comment in `src/conversion.rs` noted it. Users were never told, and the README said nothing. For a tool that began as a GIF extractor, a run that drops every frame but the first without a word is a poor default.

## Decision

**A GIF with more than one frame is not converted.** The Conversion policy reports it as a new outcome, `AnimatedSource`. The Image write pipeline resolves that outcome as it resolves an unsupported source under ADR-0023. It writes the original bytes under the GIF format in the conversion-skipped role, beside one warning fact, `AnimatedConversionSkipped`, naming the source image. The warning reads "Skipping conversion for <image> (animated GIF; converting would keep only its first frame)". The image counts among the summary's skipped conversions. As with any conversion fallback, an animated GIF cover is still a written cover.

**Animation is decided from the bytes, after the format-only decision.** `outcome_without_decoding` stays format-only, so the streaming decision ADR-0030 depends on is unchanged. No conversion target matches GIF, so every GIF under conversion is already buffered for decoding, and the check costs no extra read. The check decodes at most two frames, with the default allocation limits the normal decode also applies. A GIF is animated only when its second frame decodes. A still GIF missing its trailer, or a GIF whose second frame is damaged, converts its first frame as before. A GIF whose first frame cannot be read fails in the normal decode with its own error.

**Routing is unchanged.** With `--gif-output`, every GIF is routed in its original bytes before conversion is considered, as before. This decision reaches only a GIF that conversion would otherwise take.

Two alternatives were rejected:
- **Convert it and warn.** The output would still be a lossy reduction that nobody asked for, and the warning would only say so afterwards. Keeping the original loses nothing, and someone who wants a still can convert the kept GIF.
- **Encode an animated target.** Only WebP has an animated form among the targets, so JPEG and PNG would still need this fallback. An animated WebP would need a second encoding path, and conversion output would depend on the target in a second way.

## Consequences

Animated PNG and animated WebP sources are out of scope. They are rare inside documents, the audit named GIF, and their decoders answer the question through different APIs. The outcome carries the Image format, so a later decision can widen the check without new vocabulary. ADR-0023 is extended, not replaced: its fallback now has a third cause beside an unsupported source and a failed conversion. The README's Conversion section states the rule, and the glossary's Conversion policy entry names it. Nothing here becomes public, and ADR-0003 still governs the library interface.
