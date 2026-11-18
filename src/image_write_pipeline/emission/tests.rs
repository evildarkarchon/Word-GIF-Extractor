//! Tests for collision-safe Image file emission.

use std::io::{Cursor, Read};

use super::*;
use crate::emitted_image_tally::EmittedImageTally;
use crate::image_format::ImageFormat;
use crate::image_size_limit::ImageSizeLimit;
use crate::image_write_pipeline::discovery::{ArchiveImageDiscoveryOutcome, identify_image};
use crate::image_write_pipeline::{ArchiveImageSource, ImageWriteResult, normal_image_recorder};
use crate::test_support::{FailAfterReader, MINIMAL_PNG, file_names_in, temp_test_dir};

/// Builds one prepared image by hand, as preparation would have handed it over.
fn prepared<'policy>(
    data: &[u8],
    format: ImageFormat,
    role: EmittedImageRole<'policy>,
) -> PreparedImage<'policy> {
    PreparedImage {
        data: data.to_vec(),
        format,
        role,
    }
}

/// Stages every prepared image through one Image file emission, then finishes it.
///
/// Each reported role is recorded as a normal image under the pipeline's role
/// mapping, the way the normal-image visitor records it, so the result carries the
/// tally a pipeline invocation would have reported for the same images.
fn emit_all<'policy>(
    placement: &OutputPlacement,
    images: Vec<PreparedImage<'policy>>,
) -> Result<ImageWriteResult> {
    let mut tally = EmittedImageTally::default();
    let mut record = normal_image_recorder(&mut tally);
    let mut emission = ImageFileEmission::new(placement);
    for image in images {
        emission.stage(image, &mut record)?;
    }
    emission.finish(record)?;
    Ok(ImageWriteResult::new(tally, Vec::new()))
}

/// Verifies a lone image is named after the base name alone, with no ordinal.
#[test]
fn lone_staged_image_is_written_under_the_base_name_once_emission_finishes() {
    let temp_dir = temp_test_dir("emission", "lone-image");

    let result = emit_all(
        &OutputPlacement::new(&temp_dir, "sample"),
        vec![prepared(
            MINIMAL_PNG,
            ImageFormat::Png,
            EmittedImageRole::Preserved,
        )],
    )
    .expect("a lone staged image should be written");

    assert_eq!(result.tally.emitted(), 1);
    assert_eq!(fs::read(temp_dir.join("sample.png")).unwrap(), MINIMAL_PNG);
    assert!(!temp_dir.join("sample_1.png").exists());
}

/// Verifies a destination directory appears only once a file is written to it.
///
/// Only a routed GIF is staged, so its GIF destination is created and written to
/// while the placement's output directory, a sibling under the same parent, is
/// never needed. The sibling's absence means something only because the GIF
/// destination beside it is known to have been created.
#[test]
fn destination_directory_does_not_exist_until_a_file_is_written_to_it() {
    let temp_dir = temp_test_dir("emission", "lazy-destination");
    let gif_dir = temp_dir.join("gifs");
    let output_dir = temp_dir.join("images");
    let gif = b"GIF89a routed payload".to_vec();

    let result = emit_all(
        &OutputPlacement::new(&output_dir, "sample"),
        vec![prepared(
            &gif,
            ImageFormat::Gif,
            EmittedImageRole::RoutedGif(&gif_dir),
        )],
    )
    .expect("a lone routed GIF should be written");

    assert_eq!(result.tally.gifs_routed(), 1);
    assert_eq!(fs::read(gif_dir.join("sample.gif")).unwrap(), gif);
    assert!(!output_dir.exists());
}

/// Verifies a held-back image creates no directory until finishing names and writes it.
///
/// Staging one image cannot name it yet, so nothing may touch the filesystem until
/// emission finishes. Each destination's absence before finishing means something
/// only because finishing is then shown to create and write the GIF destination.
#[test]
fn held_back_image_creates_no_directory_until_emission_finishes() {
    let temp_dir = temp_test_dir("emission", "held-back-no-directory");
    let gif_dir = temp_dir.join("gifs");
    let output_dir = temp_dir.join("images");
    let gif = b"GIF89a routed payload".to_vec();
    let placement = OutputPlacement::new(&output_dir, "sample");
    let mut recorded = Vec::new();

    let mut emission = ImageFileEmission::new(&placement);
    emission
        .stage(
            prepared(
                &gif,
                ImageFormat::Gif,
                EmittedImageRole::RoutedGif(&gif_dir),
            ),
            |role| recorded.push(role),
        )
        .expect("staging a first image should hold it back");

    assert!(recorded.is_empty());
    assert!(!gif_dir.exists());
    assert!(!output_dir.exists());

    emission
        .finish(|role| recorded.push(role))
        .expect("finishing should write the held-back image");

    assert!(matches!(recorded[..], [EmittedImageRole::RoutedGif(_)]));
    assert_eq!(fs::read(gif_dir.join("sample.gif")).unwrap(), gif);
    assert!(!output_dir.exists());
}

/// Verifies finishing an emission that staged nothing writes nothing at all.
///
/// The temporary directory is handed out uncreated, so its absence afterwards shows
/// finishing created no output directory and therefore wrote no file into it.
#[test]
fn finishing_with_nothing_staged_writes_nothing_and_creates_no_directory() {
    let temp_dir = temp_test_dir("emission", "nothing-staged");
    let placement = OutputPlacement::new(&temp_dir, "sample");
    let mut recorded = Vec::new();

    ImageFileEmission::new(&placement)
        .finish(|role| recorded.push(role))
        .expect("finishing an empty emission should succeed");

    assert!(recorded.is_empty());
    assert!(!temp_dir.exists());
}

/// Verifies a held-back image written ahead of a failing second image is recorded.
///
/// The second image is a routed GIF whose GIF destination is occupied by a regular
/// file, so creating that directory fails deterministically on every platform. The
/// held-back first image is written before the second is attempted, and its role
/// must already be recorded when the error comes back, or the caller's facts would
/// miss a file that is on disk.
#[test]
fn held_back_image_is_recorded_when_the_second_image_then_fails() {
    let temp_dir = temp_test_dir("emission", "second-image-fails");
    let gif_dir = temp_dir.join("gifs");
    let output_dir = temp_dir.join("images");
    fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");
    fs::write(&gif_dir, b"not a directory").expect("blocking file should be writable");
    let gif = b"GIF89a routed payload".to_vec();
    let placement = OutputPlacement::new(&output_dir, "sample");
    let mut recorded = Vec::new();

    let mut emission = ImageFileEmission::new(&placement);
    emission
        .stage(
            prepared(MINIMAL_PNG, ImageFormat::Png, EmittedImageRole::Preserved),
            |role| recorded.push(role),
        )
        .expect("staging a first image should hold it back");
    let error = emission
        .stage(
            prepared(
                &gif,
                ImageFormat::Gif,
                EmittedImageRole::RoutedGif(&gif_dir),
            ),
            |role| recorded.push(role),
        )
        .expect_err("a second image whose directory cannot be created should fail");

    assert!(format!("{error:#}").contains("Failed to create output directory"));
    assert!(matches!(recorded[..], [EmittedImageRole::Preserved]));
    assert_eq!(
        fs::read(output_dir.join("sample_1.png")).unwrap(),
        MINIMAL_PNG
    );
}

/// Verifies numbering, once begun, continues past the two images that started it.
///
/// The second image is what decides numbering, so the first two are named together;
/// a third must continue from that count rather than fall back to deciding again.
#[test]
fn numbering_continues_past_the_images_that_started_it() {
    let temp_dir = temp_test_dir("emission", "numbering-continues");
    let payloads = (1..=3u8)
        .map(|index| {
            let mut payload = MINIMAL_PNG.to_vec();
            payload.push(index);
            payload
        })
        .collect::<Vec<_>>();

    emit_all(
        &OutputPlacement::new(&temp_dir, "sample"),
        payloads
            .iter()
            .map(|payload| prepared(payload, ImageFormat::Png, EmittedImageRole::Preserved))
            .collect(),
    )
    .expect("three accepted images should be written");

    for (index, payload) in payloads.iter().enumerate() {
        let name = format!("sample_{}.png", index + 1);
        assert_eq!(&fs::read(temp_dir.join(&name)).unwrap(), payload, "{name}");
    }
    assert!(!temp_dir.join("sample.png").exists());
}

/// Verifies numbering follows document position even when a GIF is routed elsewhere.
///
/// One ordinal counts every image the document emits, so the routed GIF is the
/// second image of the document in a directory where it is the only file. The
/// README states the rule; this pins it, since nothing else covered a routed GIF in
/// a document that also emits a normal image.
#[test]
fn numbering_follows_document_position_across_routed_gifs() {
    let temp_dir = temp_test_dir("emission", "numbering-across-routed-gifs");
    let gif_dir = temp_dir.join("gifs");
    let output_dir = temp_dir.join("images");
    let png = MINIMAL_PNG.to_vec();
    let gif = b"GIF89a routed payload".to_vec();

    let result = emit_all(
        &OutputPlacement::new(&output_dir, "sample"),
        vec![
            prepared(&png, ImageFormat::Png, EmittedImageRole::Preserved),
            prepared(
                &gif,
                ImageFormat::Gif,
                EmittedImageRole::RoutedGif(&gif_dir),
            ),
        ],
    )
    .expect("a normal image and a routed GIF should both be written");

    assert_eq!(result.tally.emitted(), 2);
    assert_eq!(result.tally.gifs_routed(), 1);
    assert_eq!(fs::read(output_dir.join("sample_1.png")).unwrap(), png);
    assert_eq!(fs::read(gif_dir.join("sample_2.gif")).unwrap(), gif);
    assert!(!gif_dir.join("sample_1.gif").exists());
}

/// Verifies concurrent emissions into one directory never overwrite each other.
///
/// Every writer emits a lone image under the same base name at once, so they all
/// race for the same singular name. Atomic reservation must give each its own file:
/// one keeps the base name and the rest take collision suffixes, with no payload lost.
#[test]
fn concurrent_image_emissions_preserve_every_payload() {
    const WRITER_COUNT: usize = 32;

    let temp_dir = temp_test_dir("emission", "concurrent-emissions");
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(WRITER_COUNT));
    let mut expected_payloads = Vec::new();
    let mut writers = Vec::new();

    for writer_index in 0..WRITER_COUNT {
        let mut payload = MINIMAL_PNG.to_vec();
        payload.push(writer_index as u8);
        expected_payloads.push(payload.clone());

        let output_dir = temp_dir.to_path_buf();
        let barrier = barrier.clone();
        writers.push(std::thread::spawn(move || {
            barrier.wait();
            emit_all(
                &OutputPlacement::new(&output_dir, "shared"),
                vec![prepared(
                    &payload,
                    ImageFormat::Png,
                    EmittedImageRole::Preserved,
                )],
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
}

/// Verifies an existing file is left untouched and the new image takes a suffix.
///
/// A lone image would be named `shared.png`; with that name taken, it must land under
/// the established `<stem>_<n>` collision form (`shared_1.png`) rather than replace
/// the file already there.
#[test]
fn existing_output_is_preserved_and_uses_compatible_collision_suffix() {
    let temp_dir = temp_test_dir("emission", "existing-output");
    fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");
    fs::write(temp_dir.join("shared.png"), b"existing")
        .expect("existing output should be writable");

    let result = emit_all(
        &OutputPlacement::new(&temp_dir, "shared"),
        vec![prepared(
            MINIMAL_PNG,
            ImageFormat::Png,
            EmittedImageRole::Preserved,
        )],
    )
    .expect("colliding image emission should succeed");

    assert_eq!(result.tally.emitted(), 1);
    assert_eq!(
        fs::read(temp_dir.join("shared.png")).expect("existing output should remain readable"),
        b"existing"
    );
    assert_eq!(
        fs::read(temp_dir.join("shared_1.png"))
            .expect("collision-suffixed image should be readable"),
        MINIMAL_PNG
    );
}

/// Verifies a base name past the filesystem's name cap is written, not refused.
///
/// The audit (L5) found that one long EPUB title failed the whole document with a
/// create error. Every image here is written under one shortened stem that keeps
/// its ordinal and extension. A second run then collides with the first, which
/// checks that collision suffixes fit too.
#[test]
fn overlong_base_name_is_shortened_to_a_name_the_filesystem_creates() {
    let temp_dir = temp_test_dir("emission", "overlong-base-name");
    let base = "Author - ".to_string() + &"A Very Long Subtitle ".repeat(15);
    let placement = OutputPlacement::new(&temp_dir, base.as_str());
    let images = || {
        vec![
            prepared(MINIMAL_PNG, ImageFormat::Png, EmittedImageRole::Preserved),
            prepared(MINIMAL_PNG, ImageFormat::Png, EmittedImageRole::Preserved),
        ]
    };

    emit_all(&placement, images()).expect("an overlong base name should be fitted");
    emit_all(&placement, images()).expect("colliding overlong names should be fitted");

    let names = file_names_in(&temp_dir);
    let stem = names[0]
        .strip_suffix("_1.png")
        .expect("the first image should keep its ordinal");
    assert!(base.starts_with(stem), "{stem:?} should begin {base:?}");
    assert_eq!(
        names,
        ["_1.png", "_1_1.png", "_2.png", "_2_1.png"].map(|suffix| format!("{stem}{suffix}"))
    );
}

#[test]
fn write_failure_removes_the_reserved_partial_file() {
    let temp_dir = temp_test_dir("emission", "partial-cleanup");
    fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");
    let output_path = temp_dir.join("sample.png");
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&output_path)
        .expect("test output should be reservable");

    let result = complete_reserved_file::<()>(&output_path, file, |mut file| {
        file.write_all(b"partial")
            .expect("partial test bytes should be writable");
        Err(FileCompletionError::write(io::Error::other(
            "injected write failure",
        )))
    });

    assert!(result.is_err());
    assert!(!output_path.exists());
}

#[test]
fn cleanup_failure_is_reported_with_the_original_write_failure() {
    let temp_dir = temp_test_dir("emission", "cleanup-failure");
    fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");
    let output_path = temp_dir.join("sample.png");
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&output_path)
        .expect("test output should be reservable");
    let replacement_path = output_path.clone();

    let error = complete_reserved_file::<()>(&output_path, file, move |mut file| {
        file.write_all(b"partial")
            .expect("partial test bytes should be writable");
        drop(file);
        fs::remove_file(&replacement_path).expect("reserved test file should be removable");
        fs::create_dir(&replacement_path)
            .expect("replacement directory should make file cleanup fail");
        fs::write(replacement_path.join("child"), b"occupied")
            .expect("replacement directory should be non-empty");
        Err(FileCompletionError::write(io::Error::other(
            "injected write failure",
        )))
    })
    .expect_err("injected write and cleanup failures should be returned");
    let error_report = format!("{error:#}");

    assert!(error_report.contains("injected write failure"));
    assert!(error_report.contains("failed to remove partial output file"));
    assert!(error_report.contains(&output_path.display().to_string()));
}

/// Returns a PNG-identified payload of `len` bytes whose tail varies with `seed`,
/// so two payloads written under swapped names cannot compare equal by accident.
fn png_payload(len: usize, seed: u8) -> Vec<u8> {
    let mut payload = vec![0; len];
    payload[..MINIMAL_PNG.len()].copy_from_slice(MINIMAL_PNG);
    for (index, byte) in payload[MINIMAL_PNG.len()..].iter_mut().enumerate() {
        *byte = ((index % 251) as u8).wrapping_add(seed);
    }
    payload
}

/// Identifies `reader` through Archive image discovery and returns it ready to stream.
///
/// The payload can only be built by discovery, so the test goes through it exactly
/// as the normal-image visitor does, with `limit` as the Image size limit.
fn streamed<'policy, 'reader>(
    reader: &'reader mut dyn Read,
    role: EmittedImageRole<'policy>,
    limit: ImageSizeLimit,
) -> StreamedImage<'policy, 'reader> {
    let source = ArchiveImageSource::named("word/media/streamed.bin");
    let discovered = identify_image(&source, reader, &ImageFormat::all_set(), limit);
    let ArchiveImageDiscoveryOutcome::Accepted(pending) = discovered.outcome else {
        panic!("a streamed test source should be accepted");
    };
    StreamedImage {
        payload: pending.payload,
        format: pending.format,
        role,
    }
}

/// Records the largest size `watched` reached on disk while this reader still had
/// bytes left to give.
///
/// A streamed file grows while its source is still being read; a buffered one would
/// only appear once the source was finished. Watching from inside the reader is what
/// tells the two apart. The final end-of-file read is not watched, because by then
/// every byte has been handed over and both strategies would show a whole file.
struct DiskWatchingReader {
    cursor: Cursor<Vec<u8>>,
    watched: PathBuf,
    largest_seen: u64,
}

impl Read for DiskWatchingReader {
    /// Notes the watched file's current size before a read that still returns bytes.
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let unread = self.cursor.position() < self.cursor.get_ref().len() as u64;
        if unread && let Ok(metadata) = fs::metadata(&self.watched) {
            self.largest_seen = self.largest_seen.max(metadata.len());
        }
        self.cursor.read(buffer)
    }
}

/// Verifies a lone streamed image is buffered, held back, and named singularly.
#[test]
fn lone_streamed_image_is_held_back_and_written_under_the_base_name() {
    let temp_dir = temp_test_dir("emission", "lone-streamed");
    let placement = OutputPlacement::new(&temp_dir, "sample");
    let payload = png_payload(150_000, 1);
    let mut reader = Cursor::new(payload.clone());
    let mut recorded = Vec::new();

    let mut emission = ImageFileEmission::new(&placement);
    let staged = emission
        .stage_streamed(
            streamed(
                &mut reader,
                EmittedImageRole::Preserved,
                ImageSizeLimit::DEFAULT,
            ),
            |role| recorded.push(role),
        )
        .expect("staging a first streamed image should hold it back");
    assert!(matches!(staged, StreamStaging::Staged));
    assert!(recorded.is_empty());
    assert!(!temp_dir.exists());

    emission
        .finish(|role| recorded.push(role))
        .expect("finishing should write the held-back image");

    assert!(matches!(recorded[..], [EmittedImageRole::Preserved]));
    assert_eq!(fs::read(temp_dir.join("sample.png")).unwrap(), payload);
    assert_eq!(file_names_in(&temp_dir), ["sample.png"]);
}

/// Verifies streamed images are numbered in arrival order with every byte intact.
///
/// The second arrival is written before the held-back first one, so the ordinals,
/// not the completion order, are what put each payload under its own name.
#[test]
fn streamed_images_are_numbered_in_arrival_order_byte_for_byte() {
    let temp_dir = temp_test_dir("emission", "streamed-numbering");
    let placement = OutputPlacement::new(&temp_dir, "sample");
    let payloads = [
        png_payload(2_000, 1),
        png_payload(150_000, 2),
        png_payload(150_000, 3),
    ];
    let mut tally = EmittedImageTally::default();
    let mut record = normal_image_recorder(&mut tally);

    let mut emission = ImageFileEmission::new(&placement);
    for payload in &payloads {
        let mut reader = Cursor::new(payload.clone());
        let staged = emission
            .stage_streamed(
                streamed(
                    &mut reader,
                    EmittedImageRole::Preserved,
                    ImageSizeLimit::DEFAULT,
                ),
                &mut record,
            )
            .expect("a readable streamed image should stage");
        assert!(matches!(staged, StreamStaging::Staged));
    }
    emission
        .finish(record)
        .expect("finishing after numbering should write nothing more");

    assert_eq!(tally.emitted(), 3);
    for (ordinal, payload) in payloads.iter().enumerate() {
        let name = format!("sample_{}.png", ordinal + 1);
        assert_eq!(&fs::read(temp_dir.join(&name)).unwrap(), payload, "{name}");
    }
    assert_eq!(
        file_names_in(&temp_dir),
        ["sample_1.png", "sample_2.png", "sample_3.png"]
    );
}

/// Verifies a streamed image's file fills while its source is still being read.
#[test]
fn streamed_image_is_written_while_its_source_is_still_being_read() {
    let temp_dir = temp_test_dir("emission", "streamed-while-reading");
    let placement = OutputPlacement::new(&temp_dir, "sample");
    let second = png_payload(300_000, 2);
    let mut first_reader = Cursor::new(png_payload(2_000, 1));
    let mut second_reader = DiskWatchingReader {
        cursor: Cursor::new(second.clone()),
        watched: temp_dir.join("sample_2.png"),
        largest_seen: 0,
    };

    let mut emission = ImageFileEmission::new(&placement);
    emission
        .stage_streamed(
            streamed(
                &mut first_reader,
                EmittedImageRole::Preserved,
                ImageSizeLimit::DEFAULT,
            ),
            |_| {},
        )
        .expect("the first image should be held back");
    emission
        .stage_streamed(
            streamed(
                &mut second_reader,
                EmittedImageRole::Preserved,
                ImageSizeLimit::DEFAULT,
            ),
            |_| {},
        )
        .expect("the second image should stream");

    assert!(
        second_reader.largest_seen > 0,
        "the second file should have held bytes before its source was finished"
    );
    assert!(second_reader.largest_seen < second.len() as u64);
    assert_eq!(fs::read(temp_dir.join("sample_2.png")).unwrap(), second);
}

/// Verifies a second image whose source fails partway leaves the first singular.
///
/// The arrival's file is claimed and partly written before the read fails, so this
/// pins both halves of ADR-0030's order: the partial file is removed, and the
/// held-back image was never committed to a numbered name.
#[test]
fn streamed_second_image_failing_to_read_leaves_the_first_singular() {
    let temp_dir = temp_test_dir("emission", "streamed-second-unreadable");
    let placement = OutputPlacement::new(&temp_dir, "sample");
    let first = png_payload(2_000, 1);
    let mut first_reader = Cursor::new(first.clone());
    let mut failing_reader = FailAfterReader::new(png_payload(300_000, 2), 150_000);
    let mut recorded = Vec::new();

    let mut emission = ImageFileEmission::new(&placement);
    emission
        .stage_streamed(
            streamed(
                &mut first_reader,
                EmittedImageRole::Preserved,
                ImageSizeLimit::DEFAULT,
            ),
            |role| recorded.push(role),
        )
        .expect("the first image should be held back");
    let staged = emission
        .stage_streamed(
            streamed(
                &mut failing_reader,
                EmittedImageRole::Preserved,
                ImageSizeLimit::DEFAULT,
            ),
            |role| recorded.push(role),
        )
        .expect("a source that fails to read is a shortfall, not an error");
    assert!(matches!(
        staged,
        StreamStaging::SourceFellShort(PayloadShortfall::Unreadable(_))
    ));
    assert!(recorded.is_empty());
    assert!(!temp_dir.join("sample_2.png").exists());

    emission
        .finish(|role| recorded.push(role))
        .expect("finishing should write the still-held-back image");

    assert!(matches!(recorded[..], [EmittedImageRole::Preserved]));
    assert_eq!(fs::read(temp_dir.join("sample.png")).unwrap(), first);
    assert_eq!(file_names_in(&temp_dir), ["sample.png"]);
}

/// Verifies a second image past the Image size limit leaves the first singular.
#[test]
fn streamed_second_image_past_the_limit_leaves_the_first_singular() {
    let temp_dir = temp_test_dir("emission", "streamed-second-oversize");
    let placement = OutputPlacement::new(&temp_dir, "sample");
    let limit: ImageSizeLimit = "100K".parse().expect("test limit should parse");
    let first = png_payload(2_000, 1);
    let mut first_reader = Cursor::new(first.clone());
    let mut oversize_reader = Cursor::new(png_payload(300_000, 2));

    let mut emission = ImageFileEmission::new(&placement);
    emission
        .stage_streamed(
            streamed(&mut first_reader, EmittedImageRole::Preserved, limit),
            |_| {},
        )
        .expect("the first image should be held back");
    let staged = emission
        .stage_streamed(
            streamed(&mut oversize_reader, EmittedImageRole::Preserved, limit),
            |_| {},
        )
        .expect("a source past the limit is a shortfall, not an error");
    assert!(matches!(
        staged,
        StreamStaging::SourceFellShort(PayloadShortfall::ExceedsLimit(seen)) if seen == limit
    ));
    emission
        .finish(|_| {})
        .expect("finishing should write the still-held-back image");

    assert_eq!(fs::read(temp_dir.join("sample.png")).unwrap(), first);
    assert_eq!(file_names_in(&temp_dir), ["sample.png"]);
}

/// Verifies a streamed source that runs short once numbering began uses no ordinal.
#[test]
fn streamed_source_running_short_while_numbering_leaves_no_gap() {
    let temp_dir = temp_test_dir("emission", "streamed-no-gap");
    let placement = OutputPlacement::new(&temp_dir, "sample");
    let first = png_payload(2_000, 1);
    let second = png_payload(150_000, 2);
    let fourth = png_payload(150_000, 4);
    let mut tally = EmittedImageTally::default();
    let mut record = normal_image_recorder(&mut tally);

    let mut emission = ImageFileEmission::new(&placement);
    for payload in [&first, &second] {
        let mut reader = Cursor::new(payload.clone());
        emission
            .stage_streamed(
                streamed(
                    &mut reader,
                    EmittedImageRole::Preserved,
                    ImageSizeLimit::DEFAULT,
                ),
                &mut record,
            )
            .expect("a readable streamed image should stage");
    }
    let mut failing_reader = FailAfterReader::new(png_payload(300_000, 3), 150_000);
    let staged = emission
        .stage_streamed(
            streamed(
                &mut failing_reader,
                EmittedImageRole::Preserved,
                ImageSizeLimit::DEFAULT,
            ),
            &mut record,
        )
        .expect("a source that fails to read is a shortfall, not an error");
    assert!(matches!(staged, StreamStaging::SourceFellShort(_)));
    let mut fourth_reader = Cursor::new(fourth.clone());
    emission
        .stage_streamed(
            streamed(
                &mut fourth_reader,
                EmittedImageRole::Preserved,
                ImageSizeLimit::DEFAULT,
            ),
            &mut record,
        )
        .expect("a readable streamed image should stage");
    emission.finish(record).expect("finishing should succeed");

    assert_eq!(tally.emitted(), 3);
    assert_eq!(fs::read(temp_dir.join("sample_3.png")).unwrap(), fourth);
    assert_eq!(
        file_names_in(&temp_dir),
        ["sample_1.png", "sample_2.png", "sample_3.png"]
    );
}

/// Verifies a streamed arrival failing on output still leaves the held-back image written.
///
/// The streamed counterpart of the prepared-image test above: the arrival is a
/// routed GIF whose destination is occupied by a regular file, so creating it fails.
/// Under ADR-0022's order the held-back image was written before the arrival failed,
/// and the streamed order must leave the same file on disk, recorded.
#[test]
fn streamed_arrival_failing_on_output_still_writes_the_held_back_image() {
    let temp_dir = temp_test_dir("emission", "streamed-arrival-fails");
    let gif_dir = temp_dir.join("gifs");
    let output_dir = temp_dir.join("images");
    fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");
    fs::write(&gif_dir, b"not a directory").expect("blocking file should be writable");
    let placement = OutputPlacement::new(&output_dir, "sample");
    let first = png_payload(2_000, 1);
    let mut first_reader = Cursor::new(first.clone());
    let mut gif_reader = Cursor::new(b"GIF89a streamed routed payload".to_vec());
    let mut recorded = Vec::new();

    let mut emission = ImageFileEmission::new(&placement);
    emission
        .stage_streamed(
            streamed(
                &mut first_reader,
                EmittedImageRole::Preserved,
                ImageSizeLimit::DEFAULT,
            ),
            |role| recorded.push(role),
        )
        .expect("the first image should be held back");
    let error = emission
        .stage_streamed(
            streamed(
                &mut gif_reader,
                EmittedImageRole::RoutedGif(&gif_dir),
                ImageSizeLimit::DEFAULT,
            ),
            |role| recorded.push(role),
        )
        .expect_err("an arrival whose directory cannot be created should fail");

    assert!(format!("{error:#}").contains("Failed to create output directory"));
    assert!(matches!(recorded[..], [EmittedImageRole::Preserved]));
    assert_eq!(fs::read(output_dir.join("sample_1.png")).unwrap(), first);
}

/// Verifies a streamed and a prepared image share one numbering, in arrival order.
///
/// The visitor stages converted images as prepared and kept ones as streamed, so a
/// document mixing both must still number by position.
#[test]
fn streamed_and_prepared_images_share_one_numbering() {
    let temp_dir = temp_test_dir("emission", "mixed-staging");
    let placement = OutputPlacement::new(&temp_dir, "sample");
    let first = png_payload(2_000, 1);
    let converted = b"\xFF\xD8\xFF converted payload".to_vec();
    let third = png_payload(150_000, 3);
    let mut first_reader = Cursor::new(first.clone());
    let mut third_reader = Cursor::new(third.clone());

    let mut emission = ImageFileEmission::new(&placement);
    emission
        .stage_streamed(
            streamed(
                &mut first_reader,
                EmittedImageRole::Preserved,
                ImageSizeLimit::DEFAULT,
            ),
            |_| {},
        )
        .expect("the first image should be held back");
    emission
        .stage(
            prepared(&converted, ImageFormat::Jpg, EmittedImageRole::Converted),
            |_| {},
        )
        .expect("a prepared second image should write both");
    emission
        .stage_streamed(
            streamed(
                &mut third_reader,
                EmittedImageRole::Preserved,
                ImageSizeLimit::DEFAULT,
            ),
            |_| {},
        )
        .expect("a streamed third image should write");
    emission.finish(|_| {}).expect("finishing should succeed");

    assert_eq!(fs::read(temp_dir.join("sample_1.png")).unwrap(), first);
    assert_eq!(fs::read(temp_dir.join("sample_2.jpg")).unwrap(), converted);
    assert_eq!(fs::read(temp_dir.join("sample_3.png")).unwrap(), third);
}
