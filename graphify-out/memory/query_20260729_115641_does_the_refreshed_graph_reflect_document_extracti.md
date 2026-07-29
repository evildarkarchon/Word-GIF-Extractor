---
type: "query"
date: "2026-07-29T11:56:41.055690+00:00"
question: "Does the refreshed graph reflect Document extraction as the warning-wording owner?"
contributor: "graphify"
outcome: "useful"
source_nodes: ["DocumentExtractionWarning", "ImageWriteWarning"]
---

# Q: Does the refreshed graph reflect Document extraction as the warning-wording owner?

## Answer

Expanded from the verification query via graph vocabulary: [document, extraction, warning, wording, image, pipeline, formatter, classified, facts]. The refreshed graph places from_image_write_warning and get_message under DocumentExtractionWarning in src/document_extraction.rs; ImageWriteWarning retains typed facts and archive_image_acquisition_failed but has no message formatter.

## Outcome

- Signal: useful

## Source Nodes

- DocumentExtractionWarning
- ImageWriteWarning