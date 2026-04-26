## ADDED Requirements

### Requirement: Detect image format by magic value
The system SHALL inspect the binary header of each extracted file to determine its image format before applying extension-based filters.

#### Scenario: PNG file with correct extension
- **WHEN** the extractor encounters a file starting with the bytes `89 50 4E 47 0D 0A 1A 0A`
- **THEN** the system SHALL identify the format as `png`

#### Scenario: JPEG file with incorrect extension
- **WHEN** the extractor encounters a file starting with the bytes `FF D8 FF`
- **THEN** the system SHALL identify the format as `jpg` regardless of the file extension

#### Scenario: GIF file with missing extension
- **WHEN** the extractor encounters a file starting with the bytes `47 49 46 38` (`GIF8`)
- **THEN** the system SHALL identify the format as `gif`

#### Scenario: WebP file with generic extension
- **WHEN** the extractor encounters a file with the RIFF header starting with `52 49 46 46` and containing `57 45 42 50` (`WEBP`) at bytes 8-11
- **THEN** the system SHALL identify the format as `webp`

#### Scenario: BMP file with incorrect extension
- **WHEN** the extractor encounters a file starting with the bytes `42 4D` (`BM`)
- **THEN** the system SHALL identify the format as `bmp`

#### Scenario: TIFF file with little-endian header
- **WHEN** the extractor encounters a file starting with the bytes `49 49 2A 00` (`II*`)
- **THEN** the system SHALL identify the format as `tiff`

#### Scenario: TIFF file with big-endian header
- **WHEN** the extractor encounters a file starting with the bytes `4D 4D 00 2A` (`MM\0*`)
- **THEN** the system SHALL identify the format as `tiff`

#### Scenario: ICO file with incorrect extension
- **WHEN** the extractor encounters a file starting with the bytes `00 00 01 00`
- **THEN** the system SHALL identify the format as `ico`

### Requirement: Fall back to file extension when magic value is unrecognized
The system SHALL use the original file extension as the format identifier if the binary header does not match any known magic value.

#### Scenario: Unknown magic value with valid extension
- **WHEN** the extractor encounters a file with an unrecognized header but the extension `.png`
- **THEN** the system SHALL identify the format as `png` and emit a warning that magic detection failed

#### Scenario: Unknown magic value with invalid extension
- **WHEN** the extractor encounters a file with an unrecognized header and extension `.bin`
- **THEN** the system SHALL skip the file unless `.bin` is explicitly included in the supported formats list

### Requirement: Support text-based SVG detection
The system SHALL detect SVG files by inspecting the first bytes for XML or SVG markers when binary magic detection does not apply.

#### Scenario: SVG file with correct extension
- **WHEN** the extractor encounters a file starting with `<?xml` or `<svg`
- **THEN** the system SHALL identify the format as `svg`

### Requirement: Support WMF and EMF detection
The system SHALL detect WMF and EMF files by their respective magic values where available.

#### Scenario: WMF file with placeable metafile header
- **WHEN** the extractor encounters a file starting with the bytes `D7 CD C6 9A`
- **THEN** the system SHALL identify the format as `wmf`

#### Scenario: EMF file with correct header
- **WHEN** the extractor encounters a file starting with the bytes `01 00 00 00` and containing `EMF` in the first 44 bytes
- **THEN** the system SHALL identify the format as `emf`

### Requirement: Apply format filter after magic detection
The system SHALL apply the user-specified format filter (`-f`) against the detected format rather than the original file extension.

#### Scenario: User filters for PNG and file is mislabeled as BIN
- **WHEN** the user runs with `-f png` and the extractor finds a PNG file labeled `.bin`
- **THEN** the system SHALL extract the file because magic detection identifies it as `png`
