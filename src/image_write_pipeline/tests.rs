//! Tests for discovering and emitting archive images incrementally.

use super::*;
use crate::conversion::{ConversionRequest, ConversionTarget};
use crate::test_support::{FailAfterReader, temp_test_dir};
use std::fs;
use std::io::{self, Cursor, Read};
use std::path::PathBuf;

const MINIMAL_PNG: &[u8] = b"\x89PNG\r\n\x1A\n\x00\x00\x00\rIHDR";

struct AssertOutputBeforeTailReader {
    cursor: Cursor<Vec<u8>>,
    expected_output: PathBuf,
    checked: bool,
}

impl AssertOutputBeforeTailReader {
    /// Creates a reader that verifies earlier emission after its evidence prefix.
    fn new(data: Vec<u8>, expected_output: PathBuf) -> Self {
        Self {
            cursor: Cursor::new(data),
            expected_output,
            checked: false,
        }
    }
}

impl Read for AssertOutputBeforeTailReader {
    /// Verifies earlier outputs before reading beyond this source's evidence prefix.
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.cursor.position() >= 1027 && !self.checked {
            assert!(
                self.expected_output.exists(),
                "earlier prepared images should be emitted before the third payload tail"
            );
            self.checked = true;
        }
        self.cursor.read(buffer)
    }
}

/// Writes buffered test fixtures through the scoped production reader seam.
fn write_sources(
    pipeline: &ImageWritePipeline,
    request: ImageWriteRequest<'_>,
    sources: Vec<(ArchiveImageSource, Vec<u8>)>,
) -> ImageWriteOutcome {
    pipeline.write_from(request, |visitor| {
        for (source, data) in sources {
            visitor.visit(source, &mut Cursor::new(data))?;
        }
        Ok(())
    })
}

/// Creates one named buffered source for a pipeline interface test.
fn named_source(data: impl Into<Vec<u8>>, source_name: &str) -> (ArchiveImageSource, Vec<u8>) {
    (ArchiveImageSource::named(source_name), data.into())
}

/// Creates one MIME-labelled buffered source for a pipeline interface test.
fn mime_source(
    data: impl Into<Vec<u8>>,
    source_name: &str,
    mime: &str,
) -> (ArchiveImageSource, Vec<u8>) {
    (
        ArchiveImageSource::named(source_name).with_mime(mime),
        data.into(),
    )
}

/// Proves a defaulted cover is emitted under the format it was defaulted to.
///
/// That the payload defaults to JPEG at all, and that the whole payload is read,
/// are Archive image discovery's facts and are asserted as values there. What
/// only this seam shows is the rest of the journey: the defaulting warning
/// survives the fold into the cover outcome, and the bytes land under the
/// defaulted format's extension rather than the `.png` in the source name.
#[test]
fn required_cover_defaults_unidentified_evidence_to_jpeg_and_emits_it() {
    let temp_dir = temp_test_dir("pipeline", "required-cover-default-jpeg");
    let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
        HashSet::from([ImageFormat::Jpg]),
        None,
        None,
    ));
    let original = vec![0x5a; 4096];
    let mut reader = Cursor::new(original.clone());

    let outcome = pipeline
        .write_required_cover(
            RequiredCoverWriteRequest::new(&temp_dir, "sample"),
            |visitor| {
                visitor.visit(
                    ArchiveImageSource::required_cover("OPS/cover.png", "application/octet-stream"),
                    &mut reader,
                )
            },
        )
        .expect("required cover write should succeed");

    let RequiredCoverWriteOutcome::Completed(result) = outcome else {
        panic!("a readable required cover should complete the cover decision");
    };
    assert_eq!(result.counts.extracted, 1);
    assert_eq!(result.counts.gifs_routed, 0);
    assert_eq!(result.counts.converted, 0);
    assert_eq!(result.counts.skipped, 0);
    assert!(!result.has_normal_image_output());
    assert_eq!(
        result.warnings,
        vec![ImageWriteWarning::CoverDefaultToJpeg {
            mime: "application/octet-stream".to_string(),
        }]
    );
    assert_eq!(fs::read(temp_dir.join("sample.jpg")).unwrap(), original);
    assert!(!temp_dir.join("sample.png").exists());

    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}

/// Pins that a completed discovery is a *final* cover decision.
///
/// Discovery cannot produce this fact: it reports that the cover's format was
/// filtered, and the mapping from that report to "stop looking for a cover"
/// belongs to the pipeline. The pair below — completed is final, a failed
/// acquisition is retryable — is the whole contract the EPUB cover retry loop is
/// built on, which is why it stays here after the surrounding assertions moved
/// down to Archive image discovery's own tests.
#[test]
fn required_cover_completing_without_emission_is_a_final_outcome() {
    let temp_dir = temp_test_dir("pipeline", "required-cover-filter");
    let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
        HashSet::from([ImageFormat::Png]),
        None,
        None,
    ));
    let mut reader = Cursor::new(vec![0xff; 4096]);
    reader.get_mut()[..3].copy_from_slice(b"\xFF\xD8\xFF");

    let outcome = pipeline
        .write_required_cover(
            RequiredCoverWriteRequest::new(&temp_dir, "sample"),
            |visitor| {
                visitor.visit(
                    ArchiveImageSource::required_cover("OPS/cover.jpg", "image/jpeg"),
                    &mut reader,
                )
            },
        )
        .expect("format filtering should be a normal cover outcome");

    let RequiredCoverWriteOutcome::Completed(result) = outcome else {
        panic!("a filtered cover should complete rather than retry");
    };
    assert_eq!(result.counts.extracted, 0);
    assert!(!result.has_normal_image_output());
}

/// Pins that a failed acquisition leaves the cover decision open.
///
/// The counterpart to the test above: the retry loop only tries another
/// candidate because acquisition failure maps to this arm, so the arm is
/// asserted rather than inferred from the warning it happens to carry.
#[test]
fn required_cover_acquisition_failure_permits_another_candidate() {
    let temp_dir = temp_test_dir("pipeline", "required-cover-tail-failure");
    let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
        HashSet::from([ImageFormat::Jpg]),
        None,
        None,
    ));
    let mut reader = FailAfterReader::new(vec![0; 2048], 1100);

    let outcome = pipeline
        .write_required_cover(
            RequiredCoverWriteRequest::new(&temp_dir, "sample"),
            |visitor| {
                visitor.visit(
                    ArchiveImageSource::required_cover("OPS/cover.bin", "application/octet-stream"),
                    &mut reader,
                )
            },
        )
        .expect("a cover acquisition failure should be a typed outcome");

    let RequiredCoverWriteOutcome::Retry(result) = outcome else {
        panic!("a tail read failure should permit another cover candidate");
    };
    assert_eq!(result.counts.extracted, 0);
    assert!(!result.has_normal_image_output());
}

#[test]
fn required_cover_conversion_skip_is_final_and_writes_nothing() {
    let temp_dir = temp_test_dir("pipeline", "required-cover-conversion-skip");
    let pipeline = pipeline_with_conversion(
        HashSet::from([ImageFormat::Svg]),
        ConversionTarget::Png,
        None,
    );
    let mut reader = Cursor::new(b"<svg/>".to_vec());

    let outcome = pipeline
        .write_required_cover(
            RequiredCoverWriteRequest::new(&temp_dir, "sample"),
            |visitor| {
                visitor.visit(
                    ArchiveImageSource::required_cover("OPS/cover.svg", "image/svg+xml"),
                    &mut reader,
                )
            },
        )
        .expect("unsupported cover conversion should be a normal outcome");

    let RequiredCoverWriteOutcome::Completed(result) = outcome else {
        panic!("conversion skip should complete rather than retry");
    };
    assert_eq!(result.counts.extracted, 0);
    assert_eq!(result.counts.skipped, 0);
    assert!(!result.has_normal_image_output());
    assert_eq!(
        result.warnings,
        vec![ImageWriteWarning::CoverConversionSkipped {
            format: ImageFormat::Svg,
        }]
    );
}

#[test]
fn required_cover_conversion_failure_is_final_and_writes_nothing() {
    let temp_dir = temp_test_dir("pipeline", "required-cover-conversion-failure");
    let pipeline = pipeline_with_conversion(
        HashSet::from([ImageFormat::Png]),
        ConversionTarget::Jpg,
        None,
    );
    let mut reader = Cursor::new(b"\x89PNG\r\n\x1A\ninvalid".to_vec());

    let outcome = pipeline
        .write_required_cover(
            RequiredCoverWriteRequest::new(&temp_dir, "sample"),
            |visitor| {
                visitor.visit(
                    ArchiveImageSource::required_cover("OPS/cover.png", "image/png"),
                    &mut reader,
                )
            },
        )
        .expect("cover conversion failure should be a normal outcome");

    let RequiredCoverWriteOutcome::Completed(result) = outcome else {
        panic!("conversion failure should complete rather than retry");
    };
    assert_eq!(result.counts.extracted, 0);
    assert_eq!(result.counts.skipped, 0);
    assert!(!result.has_normal_image_output());
    assert!(matches!(
        &result.warnings[..],
        [ImageWriteWarning::CoverConversionFailed { detail }]
            if detail == "Failed to decode image"
    ));
}

#[test]
fn required_gif_cover_routes_without_conversion() {
    let temp_dir = temp_test_dir("pipeline", "required-cover-gif-routing");
    let gif_dir = temp_dir.join("gifs");
    let output_dir = temp_dir.join("images");
    let pipeline = pipeline_with_conversion(
        HashSet::from([ImageFormat::Gif]),
        ConversionTarget::Png,
        Some(gif_dir.clone()),
    );
    let mut reader = Cursor::new(b"GIF89a".to_vec());

    let outcome = pipeline
        .write_required_cover(
            RequiredCoverWriteRequest::new(&output_dir, "sample"),
            |visitor| {
                visitor.visit(
                    ArchiveImageSource::required_cover("OPS/cover.gif", "image/gif"),
                    &mut reader,
                )
            },
        )
        .expect("routed required GIF should be emitted");

    let RequiredCoverWriteOutcome::Completed(result) = outcome else {
        panic!("a routed cover should complete");
    };
    assert_eq!(result.counts.extracted, 1);
    assert_eq!(result.counts.gifs_routed, 1);
    assert_eq!(result.counts.converted, 0);
    assert!(result.warnings.is_empty());
    assert_eq!(fs::read(gif_dir.join("sample.gif")).unwrap(), b"GIF89a");
    assert!(!output_dir.exists());

    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}

/// Proves an emitted file is named from the format its payload was identified as.
///
/// One representative payload rather than one per signature. That every
/// signature is recognized is asserted over values in Archive image discovery's
/// tests, and that every format has the canonical extension it does is asserted
/// in the Image format module's, so a second row here would walk both through a
/// filesystem to learn nothing new. WEBP is chosen because it is nowhere else in
/// this module, and the source is named `.bin` so the emitted name can only have
/// come from the identified format.
///
/// The cost is stated in the ticket and worth repeating: eleven of the twelve
/// formats no longer have a single test walking payload to filename end to end.
/// That composition is sound but it is now a composition, not an observation.
#[test]
fn emitted_file_is_named_from_the_identified_format() {
    let temp_dir = temp_test_dir("pipeline", "format-derived-output-name");
    let pipeline =
        ImageWritePipeline::new(ImageWritePolicy::new(ImageFormat::all_set(), None, None));
    let payload = b"RIFF\x00\x00\x00\x00WEBP payload".to_vec();

    let result = write_sources(
        &pipeline,
        ImageWriteRequest::normal_images(&temp_dir, "sample"),
        vec![named_source(payload.clone(), "word/media/image.bin")],
    )
    .expect("magic-identified image should be emitted");

    assert_eq!(result.counts.extracted, 1);
    assert_eq!(result.counts.gifs_routed, 0);
    assert_eq!(result.counts.converted, 0);
    assert_eq!(result.counts.skipped, 0);
    assert!(result.has_normal_image_output());
    assert!(result.warnings.is_empty());
    assert_eq!(fs::read(temp_dir.join("sample.webp")).unwrap(), payload);

    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}

#[test]
fn magic_evidence_outranks_conflicting_extension_and_mime() {
    let temp_dir = temp_test_dir("pipeline", "magic-precedence");
    let pipeline =
        ImageWritePipeline::new(ImageWritePolicy::new(ImageFormat::all_set(), None, None));

    let result = write_sources(
        &pipeline,
        ImageWriteRequest::normal_images(&temp_dir, "sample"),
        vec![mime_source(
            MINIMAL_PNG,
            "word/media/image.jpg",
            "image/gif",
        )],
    )
    .expect("magic-identified image should be emitted");

    assert_eq!(result.counts.extracted, 1);
    assert!(result.warnings.is_empty());
    assert_eq!(fs::read(temp_dir.join("sample.png")).unwrap(), MINIMAL_PNG);
    assert!(!temp_dir.join("sample.jpg").exists());
    assert!(!temp_dir.join("sample.gif").exists());

    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}

/// Proves a payload larger than the evidence window reaches disk whole.
///
/// The reader-position assertion this test used to carry is Archive image
/// discovery's two-phase-read fact and is asserted over values there. What
/// survives here is the on-disk consequence, and the varying tail is what makes
/// it evidence: a payload rebuilt from the wrong offset cannot compare equal by
/// accident. Incremental *ordering* is a different fact, pinned separately by
/// `earlier_images_are_emitted_before_third_payload_is_fully_read`.
#[test]
fn accepted_source_reuses_its_evidence_prefix_and_emits_the_complete_payload() {
    let temp_dir = temp_test_dir("pipeline", "normal-images");
    let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
        HashSet::from([ImageFormat::Png]),
        None,
        None,
    ));
    let mut original = vec![0; 4096];
    original[..MINIMAL_PNG.len()].copy_from_slice(MINIMAL_PNG);
    for (index, byte) in original[MINIMAL_PNG.len()..].iter_mut().enumerate() {
        *byte = (index % 251) as u8;
    }
    let mut reader = Cursor::new(original.clone());

    let result = pipeline
        .write_from(
            ImageWriteRequest::normal_images(&temp_dir, "sample"),
            |visitor| {
                visitor.visit(
                    ArchiveImageSource::named("word/media/image.bin"),
                    &mut reader,
                )
            },
        )
        .expect("normal image write should succeed");

    assert_eq!(result.counts.extracted, 1);
    assert!(result.has_normal_image_output());
    assert!(result.warnings.is_empty());
    assert_eq!(fs::read(temp_dir.join("sample.png")).unwrap(), original);

    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}

/// Proves a discovery outcome that emits nothing contributes nothing to the run.
///
/// *Why* each of these sources is non-emitting — a format outside the allow-set,
/// a path the safety rule rejects — is decided in Archive image discovery and
/// Image write purpose, and asserted as values there. This is the part only the
/// fold can show: such an outcome moves no count and claims no normal output.
///
/// It is asserted from the result rather than from the absence of the output
/// directory. The directory is absent before the pipeline runs (see
/// `temp_test_dir`), so checking for it would pass even if nothing ran at all;
/// counts that stayed at zero cannot.
#[test]
fn non_emitting_discovery_outcomes_move_no_counts() {
    let temp_dir = temp_test_dir("pipeline", "non-emitting-outcomes");
    let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
        HashSet::from([ImageFormat::Png]),
        None,
        None,
    ));

    let result = write_sources(
        &pipeline,
        ImageWriteRequest::normal_images(&temp_dir, "sample"),
        vec![
            // Identified, then filtered out by the allow-set.
            named_source(b"GIF89a payload".as_slice(), "word/media/animation.gif"),
            // Rejected before its reader is touched, despite valid PNG evidence.
            named_source(MINIMAL_PNG, "../media/image.png"),
        ],
    )
    .expect("non-emitting sources should be a normal pipeline outcome");

    assert_eq!(result.counts.extracted, 0);
    assert_eq!(result.counts.gifs_routed, 0);
    assert_eq!(result.counts.converted, 0);
    assert_eq!(result.counts.skipped, 0);
    assert!(!result.has_normal_image_output());
    assert!(result.warnings.is_empty());
}

#[test]
fn failed_normal_emission_does_not_report_normal_output() {
    let temp_dir = temp_test_dir("pipeline", "failed-normal-emission-purpose");
    let blocked_output = temp_dir.join("not-a-directory");
    fs::create_dir_all(&temp_dir).expect("temporary test directory should be creatable");
    fs::write(&blocked_output, b"occupied").expect("blocking file should be creatable");
    let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
        HashSet::from([ImageFormat::Png]),
        None,
        None,
    ));

    let failure = write_sources(
        &pipeline,
        ImageWriteRequest::normal_images(&blocked_output, "sample"),
        vec![named_source(MINIMAL_PNG, "word/media/image.png")],
    )
    .expect_err("failed Image file emission should abort the pipeline");

    assert_eq!(failure.partial.counts.extracted, 0);
    assert!(!failure.partial.has_normal_image_output());
    assert!(!blocked_output.join("sample.png").exists());

    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}

/// Pins extension-fallback warning order when later payload acquisition fails.
#[test]
fn extension_fallback_warning_precedes_tail_failure_and_later_source_emits() {
    let temp_dir = temp_test_dir("pipeline", "tail-read-failure");
    let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
        HashSet::from([ImageFormat::Png]),
        None,
        None,
    ));
    let failing_payload = vec![0; 2048];
    let mut failing_source = FailAfterReader::new(failing_payload, 1100);
    let mut valid_source = Cursor::new(MINIMAL_PNG);

    let result = pipeline
        .write_from(
            ImageWriteRequest::normal_images(&temp_dir, "sample"),
            |visitor| {
                visitor.visit(
                    ArchiveImageSource::named("word/media/broken.png").with_mime("image/jpeg"),
                    &mut failing_source,
                )?;
                visitor.visit(
                    ArchiveImageSource::named("word/media/valid.png"),
                    &mut valid_source,
                )?;
                Ok(())
            },
        )
        .expect("a later readable source should still be emitted");

    assert_eq!(result.counts.extracted, 1);
    assert!(matches!(
        &result.warnings[..],
        [
            ImageWriteWarning::ExtensionFallback {
                source_name: extension_source_name,
                format: ImageFormat::Png,
            },
            ImageWriteWarning::ArchiveImageAcquisitionFailed {
                source_name: acquisition_source_name,
                detail,
            }
        ] if extension_source_name == "word/media/broken.png"
            && acquisition_source_name == "word/media/broken.png"
            && detail == "injected archive resource failure"
    ));
    assert_eq!(fs::read(temp_dir.join("sample.png")).unwrap(), MINIMAL_PNG);
    assert!(!temp_dir.join("sample_1.png").exists());

    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}

#[test]
fn bom_prefixed_svg_at_end_of_evidence_window_is_discovered() {
    let temp_dir = temp_test_dir("pipeline", "svg-evidence-window");
    let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
        HashSet::from([ImageFormat::Svg]),
        None,
        None,
    ));
    let mut svg = b"\xEF\xBB\xBF".to_vec();
    svg.extend(std::iter::repeat_n(b' ', 1019));
    svg.extend_from_slice(b"<svg>");
    assert_eq!(svg.len(), 1027);

    let result = write_sources(
        &pipeline,
        ImageWriteRequest::normal_images(&temp_dir, "sample"),
        vec![named_source(svg.clone(), "word/media/vector.bin")],
    )
    .expect("the full SVG evidence window should be inspected");

    assert_eq!(result.counts.extracted, 1);
    assert!(result.warnings.is_empty());
    assert_eq!(fs::read(temp_dir.join("sample.svg")).unwrap(), svg);

    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}

#[test]
fn multiple_sources_keep_discovery_warnings_before_conversion_warnings() {
    let temp_dir = temp_test_dir("pipeline", "phase-ordered-warnings");
    let pipeline = pipeline_with_conversion(
        HashSet::from([ImageFormat::Svg, ImageFormat::Png]),
        ConversionTarget::Png,
        None,
    );

    let result = write_sources(
        &pipeline,
        ImageWriteRequest::normal_images(&temp_dir, "sample"),
        vec![
            named_source(b"not really svg".as_slice(), "media/first.svg"),
            named_source(b"not really png".as_slice(), "media/second.png"),
        ],
    )
    .expect("both accepted sources should be emitted");

    assert_eq!(result.counts.extracted, 2);
    assert_eq!(result.counts.skipped, 1);
    assert_eq!(
        result.warnings,
        vec![
            ImageWriteWarning::ExtensionFallback {
                source_name: "media/first.svg".to_string(),
                format: ImageFormat::Svg,
            },
            ImageWriteWarning::ExtensionFallback {
                source_name: "media/second.png".to_string(),
                format: ImageFormat::Png,
            },
            ImageWriteWarning::ConversionSkipped {
                base_name: "sample".to_string(),
                format: ImageFormat::Svg,
            },
        ]
    );
    assert_eq!(
        fs::read(temp_dir.join("sample_1.svg")).unwrap(),
        b"not really svg"
    );
    assert_eq!(
        fs::read(temp_dir.join("sample_2.png")).unwrap(),
        b"not really png"
    );

    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}

#[test]
fn earlier_images_are_emitted_before_third_payload_is_fully_read() {
    let temp_dir = temp_test_dir("pipeline", "two-image-lookahead");
    let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
        HashSet::from([ImageFormat::Png]),
        None,
        None,
    ));
    let mut first = Cursor::new(MINIMAL_PNG);
    let mut second = Cursor::new(MINIMAL_PNG);
    let mut third_payload = vec![0; 2048];
    third_payload[..8].copy_from_slice(b"\x89PNG\r\n\x1A\n");
    let mut third = AssertOutputBeforeTailReader::new(third_payload, temp_dir.join("sample_1.png"));

    let result = pipeline
        .write_from(
            ImageWriteRequest::normal_images(&temp_dir, "sample"),
            |visitor| {
                visitor.visit(ArchiveImageSource::named("first.png"), &mut first)?;
                visitor.visit(ArchiveImageSource::named("second.png"), &mut second)?;
                visitor.visit(ArchiveImageSource::named("third.png"), &mut third)?;
                Ok(())
            },
        )
        .expect("three images should be emitted incrementally");

    assert!(third.checked);
    assert_eq!(result.counts.extracted, 3);
    assert!(temp_dir.join("sample_1.png").exists());
    assert!(temp_dir.join("sample_2.png").exists());
    assert!(temp_dir.join("sample_3.png").exists());

    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}

#[test]
fn concurrent_image_emissions_preserve_every_payload() {
    const WRITER_COUNT: usize = 32;

    let temp_dir = temp_test_dir("pipeline", "concurrent-emissions");
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(WRITER_COUNT));
    let mut expected_payloads = Vec::new();
    let mut writers = Vec::new();

    for writer_index in 0..WRITER_COUNT {
        let mut payload = MINIMAL_PNG.to_vec();
        payload.push(writer_index as u8);
        expected_payloads.push(payload.clone());

        let output_dir = temp_dir.clone();
        let barrier = barrier.clone();
        writers.push(std::thread::spawn(move || {
            let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
                HashSet::from([ImageFormat::Png]),
                None,
                None,
            ));

            barrier.wait();
            write_sources(
                &pipeline,
                ImageWriteRequest::normal_images(&output_dir, "shared"),
                vec![named_source(payload, "word/media/image.bin")],
            )
            .expect("concurrent image emission should succeed");
        }));
    }

    for writer in writers {
        writer.join().expect("concurrent writer should not panic");
    }

    let emitted_files: Vec<(String, Vec<u8>)> = fs::read_dir(&temp_dir)
        .expect("concurrent output directory should exist")
        .map(|entry| {
            let path = entry.expect("output entry should be readable").path();
            let name = path
                .file_name()
                .expect("emitted image should have a filename")
                .to_string_lossy()
                .to_string();
            let payload = fs::read(path).expect("emitted image should be readable");
            (name, payload)
        })
        .collect();
    let (mut emitted_names, mut emitted_payloads): (Vec<_>, Vec<_>) =
        emitted_files.into_iter().unzip();
    let mut expected_names = vec!["shared.png".to_string()];
    expected_names.extend((1..WRITER_COUNT).map(|index| format!("shared_{index}.png")));

    expected_names.sort();
    emitted_names.sort();
    expected_payloads.sort();
    emitted_payloads.sort();

    assert_eq!(emitted_names, expected_names);
    assert_eq!(emitted_payloads, expected_payloads);

    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}

#[test]
fn existing_output_is_preserved_and_uses_compatible_collision_suffix() {
    let temp_dir = temp_test_dir("pipeline", "existing-output");
    fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");
    fs::write(temp_dir.join("shared.png"), b"existing")
        .expect("existing output should be writable");
    let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
        HashSet::from([ImageFormat::Png]),
        None,
        None,
    ));

    let result = write_sources(
        &pipeline,
        ImageWriteRequest::normal_images(&temp_dir, "shared"),
        vec![named_source(MINIMAL_PNG, "word/media/image.bin")],
    )
    .expect("colliding image emission should succeed");

    assert_eq!(result.counts.extracted, 1);
    assert_eq!(
        fs::read(temp_dir.join("shared.png")).expect("existing output should remain readable"),
        b"existing"
    );
    assert_eq!(
        fs::read(temp_dir.join("shared_1.png"))
            .expect("collision-suffixed image should be readable"),
        MINIMAL_PNG
    );

    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}

/// Proves extension evidence outranks MIME and returns the exact fallback warning.
#[test]
fn eligible_extension_outranks_mime_and_emits_fallback_warning() {
    let temp_dir = temp_test_dir("pipeline", "extension-fallback");
    let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
        HashSet::from([ImageFormat::Png]),
        None,
        None,
    ));

    let result = write_sources(
        &pipeline,
        ImageWriteRequest::normal_images(&temp_dir, "sample"),
        vec![mime_source(
            b"not actually a png".as_slice(),
            "word/media/image.png",
            "image/jpeg",
        )],
    )
    .expect("extension fallback image should be written");

    assert_eq!(result.counts.extracted, 1);
    assert_eq!(
        result.warnings,
        vec![ImageWriteWarning::ExtensionFallback {
            source_name: "word/media/image.png".to_string(),
            format: ImageFormat::Png,
        }]
    );
    assert_eq!(
        fs::read(temp_dir.join("sample.png")).unwrap(),
        b"not actually a png"
    );
    assert!(!temp_dir.join("sample.jpg").exists());

    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}

#[test]
fn unreadable_normal_sources_apply_source_eligibility_before_warning() {
    let temp_dir = temp_test_dir("pipeline", "unreadable-source-eligibility");
    let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
        HashSet::from([ImageFormat::Png]),
        None,
        None,
    ));

    let result = pipeline
        .write_from(
            ImageWriteRequest::normal_images(&temp_dir, "sample"),
            |visitor| {
                visitor.unreadable(
                    ArchiveImageSource::named("../unsafe.png"),
                    "unsafe source should remain silent",
                );
                visitor.unreadable(
                    ArchiveImageSource::named("word/media/safe.png"),
                    "safe source could not be opened",
                );
                Ok(())
            },
        )
        .expect("unreadable sources should remain non-fatal");

    assert_eq!(result.counts.extracted, 0);
    assert!(!result.has_normal_image_output());
    assert_eq!(
        result.warnings,
        vec![ImageWriteWarning::ArchiveImageAcquisitionFailed {
            source_name: "word/media/safe.png".to_string(),
            detail: "safe source could not be opened".to_string(),
        }]
    );
}

#[test]
fn normal_conversion_skip_writes_original_and_preserves_warning_order() {
    let temp_dir = temp_test_dir("pipeline", "normal-conversion-skip");
    let pipeline = pipeline_with_conversion(
        HashSet::from([ImageFormat::Svg]),
        ConversionTarget::Png,
        None,
    );

    let result = write_sources(
        &pipeline,
        ImageWriteRequest::normal_images(&temp_dir, "sample"),
        vec![named_source(
            b"not really svg".as_slice(),
            "media/image.svg",
        )],
    )
    .expect("normal conversion skip should preserve original bytes");

    assert_eq!(result.counts.extracted, 1);
    assert_eq!(result.counts.skipped, 1);
    assert_eq!(
        result.warnings,
        vec![
            ImageWriteWarning::ExtensionFallback {
                source_name: "media/image.svg".to_string(),
                format: ImageFormat::Svg,
            },
            ImageWriteWarning::ConversionSkipped {
                base_name: "sample".to_string(),
                format: ImageFormat::Svg,
            },
        ]
    );
    assert!(temp_dir.join("sample.svg").exists());

    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}

#[test]
fn normal_conversion_failure_writes_original_and_counts_skip() {
    let temp_dir = temp_test_dir("pipeline", "normal-conversion-failure");
    let original = b"\x89PNG\r\n\x1A\nnot a valid png".to_vec();
    let pipeline = pipeline_with_conversion(
        HashSet::from([ImageFormat::Png]),
        ConversionTarget::Jpg,
        None,
    );

    let result = write_sources(
        &pipeline,
        ImageWriteRequest::normal_images(&temp_dir, "sample"),
        vec![named_source(original.clone(), "image.png")],
    )
    .expect("normal conversion failure should preserve original bytes");

    assert_eq!(result.counts.extracted, 1);
    assert_eq!(result.counts.converted, 0);
    assert_eq!(result.counts.skipped, 1);
    assert!(matches!(
        &result.warnings[..],
        [ImageWriteWarning::ConversionFailed { base_name, detail }]
            if base_name == "sample" && detail == "Failed to decode image"
    ));
    assert_eq!(fs::read(temp_dir.join("sample.png")).unwrap(), original);

    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}

/// Pins the invariant that an unconfigured Image write policy produces no
/// conversion or GIF-routing counts.
///
/// A GIF is included deliberately: with no GIF destination it must land in the
/// normal output directory and leave `gifs_routed` at zero, which is the half of
/// the invariant a PNG-only fixture cannot observe.
#[test]
fn unconfigured_policy_produces_no_conversion_or_routing_counts() {
    let temp_dir = temp_test_dir("pipeline", "unconfigured-policy-counts");
    let png = MINIMAL_PNG.to_vec();
    let gif = b"GIF89a unrouted payload".to_vec();
    let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
        HashSet::from([ImageFormat::Png, ImageFormat::Gif]),
        None,
        None,
    ));

    let result = write_sources(
        &pipeline,
        ImageWriteRequest::normal_images(&temp_dir, "sample"),
        vec![
            named_source(png.clone(), "media/image1.png"),
            named_source(gif.clone(), "media/image2.gif"),
        ],
    )
    .expect("accepted sources should be written without conversion or routing");

    assert_eq!(result.counts.extracted, 2);
    assert_eq!(result.counts.converted, 0);
    assert_eq!(result.counts.skipped, 0);
    assert_eq!(result.counts.gifs_routed, 0);
    assert!(result.has_normal_image_output());
    assert!(result.warnings.is_empty());
    assert_eq!(fs::read(temp_dir.join("sample_1.png")).unwrap(), png);
    assert_eq!(fs::read(temp_dir.join("sample_2.gif")).unwrap(), gif);

    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}

#[test]
fn matching_conversion_target_preserves_original_without_conversion_count() {
    let temp_dir = temp_test_dir("pipeline", "matching-conversion-target");
    let original = b"accepted through extension fallback".to_vec();
    let pipeline = pipeline_with_conversion(
        HashSet::from([ImageFormat::Png]),
        ConversionTarget::Png,
        None,
    );

    let result = write_sources(
        &pipeline,
        ImageWriteRequest::normal_images(&temp_dir, "sample"),
        vec![named_source(original.clone(), "image.png")],
    )
    .expect("matching target should preserve source bytes");

    assert_eq!(result.counts.extracted, 1);
    assert_eq!(result.counts.converted, 0);
    assert_eq!(result.counts.skipped, 0);
    assert_eq!(
        result.warnings,
        vec![ImageWriteWarning::ExtensionFallback {
            source_name: "image.png".to_string(),
            format: ImageFormat::Png,
        }]
    );
    assert_eq!(fs::read(temp_dir.join("sample.png")).unwrap(), original);

    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}

/// Verifies routed GIF emission retains exact bytes and complete count semantics.
#[test]
fn routed_gif_bypasses_conversion() {
    let temp_dir = temp_test_dir("pipeline", "gif-routing");
    let gif_dir = temp_dir.join("gifs");
    let output_dir = temp_dir.join("images");
    let original = b"GIF89a routed payload".to_vec();
    let pipeline = pipeline_with_conversion(
        HashSet::from([ImageFormat::Gif]),
        ConversionTarget::Png,
        Some(gif_dir.clone()),
    );

    let result = write_sources(
        &pipeline,
        ImageWriteRequest::normal_images(&output_dir, "sample"),
        vec![named_source(original.clone(), "media/image.gif")],
    )
    .expect("routed GIF should be written without conversion");

    assert_eq!(result.counts.extracted, 1);
    assert_eq!(result.counts.gifs_routed, 1);
    assert_eq!(result.counts.converted, 0);
    assert_eq!(result.counts.skipped, 0);
    assert!(result.has_normal_image_output());
    assert!(result.warnings.is_empty());
    assert_eq!(fs::read(gif_dir.join("sample.gif")).unwrap(), original);
    assert!(!output_dir.exists());

    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}

#[test]
fn mime_is_used_only_after_magic_and_extension_evidence_fail() {
    let temp_dir = temp_test_dir("pipeline", "mime-source");
    let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
        HashSet::from([ImageFormat::Png]),
        None,
        None,
    ));

    let result = write_sources(
        &pipeline,
        ImageWriteRequest::normal_images(&temp_dir, "sample"),
        vec![mime_source(
            b"unknown bytes".as_slice(),
            "media/image.bin",
            "image/png",
        )],
    )
    .expect("MIME-identified image should be written");

    assert_eq!(result.counts.extracted, 1);
    assert!(result.warnings.is_empty());
    assert_eq!(
        fs::read(temp_dir.join("sample.png")).unwrap(),
        b"unknown bytes"
    );

    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}

#[test]
fn constructed_results_carry_their_facts_through_the_fold() {
    let earlier_warning = ImageWriteWarning::UnsupportedCoverFormat {
        format: ImageFormat::Emf,
    };
    let later_warning = ImageWriteWarning::CoverConversionFailed {
        detail: "encoder rejected the payload".to_string(),
    };
    let mut earlier = ImageWriteResult::new(
        ImageWriteCounts {
            extracted: 1,
            skipped: 3,
            ..ImageWriteCounts::default()
        },
        vec![earlier_warning.clone()],
        NormalImageOutput::Absent,
    );

    earlier.append(ImageWriteResult::new(
        ImageWriteCounts {
            extracted: 1,
            gifs_routed: 1,
            converted: 2,
            skipped: 0,
        },
        vec![later_warning.clone()],
        NormalImageOutput::Present,
    ));

    assert_eq!(earlier.counts.extracted, 2);
    assert_eq!(earlier.counts.gifs_routed, 1);
    assert_eq!(earlier.counts.converted, 2);
    assert_eq!(earlier.counts.skipped, 3);
    assert_eq!(earlier.warnings, vec![earlier_warning, later_warning]);
    assert!(earlier.has_normal_image_output());
}

/// Builds a pipeline with a default Conversion policy for interface tests.
fn pipeline_with_conversion(
    allowed_formats: HashSet<ImageFormat>,
    target: ConversionTarget,
    gif_output: Option<PathBuf>,
) -> ImageWritePipeline {
    let conversion = ConversionPolicy::try_from(ConversionRequest {
        target,
        quality: None,
        lossless: false,
    })
    .expect("test conversion request should be valid");

    ImageWritePipeline::new(ImageWritePolicy::new(
        allowed_formats,
        Some(conversion),
        gif_output,
    ))
}
