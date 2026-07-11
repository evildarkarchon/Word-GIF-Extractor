# Retain EPUB declarations for the extraction run

Document selection acquires complete payload-free EPUB declarations once and retains a successful result as the authoritative declaration facts for the Extraction run; Document extraction retries acquisition only when selection could not obtain them. This favors a consistent run snapshot and avoids repeated EPUB parsing over observing declaration changes made between selection and extraction, while ADR-0001 continues to govern independent direct-ZIP payload reads.
