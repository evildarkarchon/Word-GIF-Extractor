# Reopen EPUB archives for incremental resource reads

EPUB extraction uses `EpubDoc` to resolve manifest metadata and cover identity, then opens the same file separately through the direct ZIP adapter when reading resource payloads. This keeps EPUB traversal facts in the EPUB adapter while allowing Archive image discovery to inspect bounded evidence and fully load only accepted resources; eager `EpubDoc::get_resource` buffering and maintaining an EPUB crate fork were rejected.
