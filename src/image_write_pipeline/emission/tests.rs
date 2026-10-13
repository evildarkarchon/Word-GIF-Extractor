//! Tests for collision-safe Image file emission.

use super::*;
use crate::emitted_image_tally::EmittedImageTally;
use crate::image_format::ImageFormat;
use crate::image_write_pipeline::{ImageWriteResult, normal_image_recorder};
use crate::test_support::{MINIMAL_PNG, temp_test_dir};

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

    let result = complete_reserved_file(&output_path, file, |mut file| {
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

    let error = complete_reserved_file(&output_path, file, move |mut file| {
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
