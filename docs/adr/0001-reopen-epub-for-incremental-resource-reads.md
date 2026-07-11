# Reopen EPUB archives for incremental resource reads

EPUB declaration acquisition uses `EpubDoc` to resolve manifest metadata and cover identity, while EPUB extraction independently opens the same file through the direct ZIP adapter when reading resource payloads. This keeps payload-free declaration facts separate from incremental payload access while allowing Archive image discovery to inspect bounded evidence and fully load only accepted resources; eager `EpubDoc::get_resource` buffering and maintaining an EPUB crate fork were rejected.
