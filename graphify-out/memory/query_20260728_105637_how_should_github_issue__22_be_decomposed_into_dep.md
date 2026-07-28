---
type: "query"
date: "2026-07-28T10:56:37.769920+00:00"
question: "How should GitHub issue #22 be decomposed into dependency-aware vertical tracer-bullet tickets?"
contributor: "graphify"
outcome: "useful"
source_nodes: ["EpubResourceArchive", "ArchiveResourceIdentity", "extract_all_images()", "extract_required_cover()"]
---

# Q: How should GitHub issue #22 be decomposed into dependency-aware vertical tracer-bullet tickets?

## Answer

Expanded from original query via graph vocabulary: [epub, resource, archive, declaration, identity, payload, reader, cover, fallback, image, extraction, write]. Approved and published as two vertical tickets: #23 adopts branded EPUB archive custody for normal-image extraction; #24 moves cover retry and normal fallback onto that session and retires descriptor custody. #24 is natively blocked by #23, and both are sub-issues of #22.

## Outcome

- Signal: useful

## Source Nodes

- EpubResourceArchive
- ArchiveResourceIdentity
- extract_all_images()
- extract_required_cover()