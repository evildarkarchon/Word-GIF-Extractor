# Cap each archive image at an Image size limit, and stream images written in their original bytes

The 2026-11-14 audit (M6) showed that Archive image discovery bounds identification to 1,027 bytes, but then reads an accepted source to its end into one buffer before anything is written, and conversion decodes a second copy. Nothing checked an entry's size. A 522 KB DOCX whose one entry was a PNG signature followed by 512 MiB of zeros wrote a 512 MiB file, and held all of it in memory to do so. The release profile sets `panic = "abort"`, so if that allocation fails, the whole run ends, not just the one document. A batch over downloaded books is exactly where such an archive turns up. Two things are decided here: a ceiling on every source, and writing without a buffer wherever the bytes are emitted unchanged.

## The Image size limit

Every archive image source is held to one per-run Image size limit, part of the Image write policy. The default is 256 MiB, well above any realistic embedded picture, scanned TIFFs included. `--max-image-size <SIZE>` sets it, with a byte count or a `K`, `M` or `G` suffix (binary, so `1M` is 1,048,576 bytes; `KiB`, `MiB` and `GiB` are accepted too). A value of zero, a decimal suffix such as `MB`, or a count that overflows is rejected when the command line is parsed, as ADR-0027 does for format names.

The limit is checked against the bytes actually read, never against the uncompressed size a ZIP header declares. A declared size is a claim the archive makes about itself, and a malformed archive is the case this guards against. A source that runs past the limit is a Document extraction warning naming the source and the limit, and nothing is written for it. That is the same outcome as an unreadable source, for the same reason: the document still has its other images. For a required EPUB cover, an oversize candidate writes no cover, so EPUB cover extraction hands over to the next candidate exactly as it does after a read failure (ADR-0023). The limit belongs to Archive image discovery, because acquiring a source is discovery's job. The decoder's own allocation ceiling, the `image` crate's default 512 MiB, already bounds what conversion decodes, and the Image size limit does not replace it.

## Streaming

An image whose bytes are emitted unchanged is copied from its reader to its output file in chunks instead of being buffered. Whether the bytes are unchanged is known from the Image format alone, before the payload is read. That covers:

- every image when no Conversion policy is configured;
- a GIF routed to its own destination;
- a source that already matches the conversion target and is preserved;
- a source whose format cannot be converted at all, such as SVG, EMF or WMF.

This is wider than the audit's "when no conversion is requested". The rule is the same in each case, and the unconvertible metafiles are among the largest images an Office document holds. An image that does have to be converted is still buffered, up to the limit, because the decoder needs it whole. A conversion that then falls back emits the buffered original, as ADR-0023 decides.

The naming lookahead from ADR-0022 constrains what can stream. Image file emission cannot name the first image it is given until it knows whether a second follows, and an archive reader cannot outlive the entry it belongs to. So the first image is still held back in memory, but now never more than the limit. Every later image streams. When a streamed image arrives while the first is held back, Image file emission writes the arrival under its numbered name first, and writes the held-back image afterwards. The order matters. A source can fail partway through, from a read error or by running past the limit, after its output file has been claimed. When that happens, the partial file is removed and the image never arrived: the held-back image keeps its place and still takes the singular name if nothing else follows. The ordinal is not used up, so numbering has no gaps. This is today's behaviour for a source that fails to read, and it is the reason the arrival is written before the held-back image. Writing the held-back image first would commit it to a numbered name before anyone knew the second would complete. A partial file that cannot be removed fails the document, because the output directory would otherwise keep a truncated image no warning accounts for.

If the arrival fails on the output side instead (directory, create, write or flush), the held-back image is still written under its numbered name before the error is returned. That keeps the files on disk the same as when the held-back image was written first: under ADR-0022 a failing second image left the first written and recorded, and it still does.

A buffered payload grows by doubling, as a `Vec` does, but its growth is capped one byte past the limit, and one byte is probed before each doubling. Growing a buffer copies it, so the old and new allocations are briefly both live. Uncapped doubling near the limit held twice the limit, and the probe keeps a payload that ends exactly at the buffer's capacity from being copied into a larger buffer it does not need.

## Measured

The audit did not measure peak memory. This change was measured with release builds of `main` and of this change on Windows, using the audit's fixture: one 512 MiB entry, deflated to 521,999 bytes. Peak working set:

| Run | `main` | This change |
| --- | --- | --- |
| The entry as a document's only image, default limit | 1,030 MiB, wrote 512 MiB | 262 MiB, skipped with a warning |
| The entry as a document's only image, `--max-image-size 1G` | — | 1,030 MiB, wrote 512 MiB |
| The entry after a small image, `--max-image-size 1G` | 1,030 MiB | 5.3 MiB, both written |
| The entry after a small image, default limit | 1,030 MiB | 5.8 MiB, the small image written as `name.png` |

A lone image is held back and so buffered, which is why raising the limit above the entry brings its cost back. Every later image costs one 64 KiB chunk however large it is.

## What this amends

This amends ADR-0022, which stays binding. Image file emission gains one operation, staging a streamed image, beside staging a prepared one. Staging a streamed image reports whether the source was read through, so that the visitor records a source that ran short as a warning, not an image. The recording callback is still called once per completed file, in completion order. When a streamed arrival meets a held-back image, the arrival completes first, so it is recorded first. Everything else in ADR-0022 stands: emission owns naming, ordinals, the destination by Emitted image role, directory creation before each file, and collision-free claiming. The bytes of a streamed image come through a payload that Archive image discovery hands over, which enforces the limit and reports a read failure. Emission copies the payload and does not interpret it, so the glossary's exclusion of Archive image discovery from Image file emission still holds.

Covers are not streamed. A cover attempt stages one image and finishes, so it always holds that image back, and buffering it up to the limit costs nothing that streaming would save. The required-cover scope, its fold and ADR-0021's decisions are unchanged.

## Alternatives

Five alternatives were rejected:

- Checking only the declared size. It costs nothing to read, but a malformed archive can declare anything. It could be added as an early skip later, but the check on bytes read stays the one that decides.
- Writing the held-back image first, then streaming the arrival. Simpler, but a document with one good image and one that fails partway through would write `name_1.png` instead of `name.png`.
- Buffering the second image as well, and streaming only from the third. That keeps the old order but doubles the peak memory, and a two-image document, the most common multi-image case, would gain nothing.
- Streaming the first image to a temporary name and renaming it once its name is known. A rename on Windows replaces an existing file, so it would need a claim and a link of its own to stay collision-safe. That is a second claiming scheme beside the one ADR-0022 gives emission.
- No limit by default. The audit's point is that one malformed archive should not end a batch, and an opt-in ceiling does not protect the run nobody thought to protect.

## Scope

Out of scope:
- Parallel decoding or encoding.
- Reading the EPUB adapter's declared sizes.
- Changing any naming rule, the collision scheme or the routed-GIF ordinal rule.

`GLOSSARY.md` gains an Image size limit entry. Its Image write policy entry names the limit among the run's choices, and its Archive image discovery entry names enforcing it. Nothing here becomes public: ADR-0003 still governs the library interface, and the new type is crate-private.
