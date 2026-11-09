# Synthetic DICOM interoperability fixture

Generated for this repository; contains no real patient information.
Explicit VR Little Endian Secondary Capture, one 16×16 unsigned 8-bit frame.
Pixel values are 0 through 255 in row order. PatientID is
`OCTOFHIR-DICOM-TEST`, issuer `HOSPITAL`. Study date is deliberately 1999.
UIDs use a test-only fixture namespace and must not enter clinical archives.
The file is uploaded only to isolated test PACS; OctoFHIR stores its FHIR
manifest and references, never these DICOM bytes.

The gateway matrix additionally includes `synthetic-multiframe.dcm` (two 16×16
uncompressed grayscale frames, forward/reversed 0..255) and
`synthetic-jpeg-baseline.dcm` (one synthetic 64×64 JPEG Baseline grayscale frame).
Their Study UIDs end in `.999.2` and `.999.3`; they use the same artificial patient.
They exercise framing, compressed transfer syntax and gateway streaming, not
clinical modality or diagnostic validation.

`synthetic-cyrillic-a.dcm` and `synthetic-cyrillic-b.dcm` contain two distinct
artificial patient identifiers and UTF-8 names (Иванов^Иван and Петров^Пётр).
Their Study UIDs end in `.999.4` and `.999.5`. Real-PACS tests compare exact/fuzzy
QIDO results with direct origin queries and request InstitutionName through the
gateway, independently of its local attribute dictionary.

Regenerate the additional files in a private Python environment with
`pydicom==3.0.1` and `Pillow==12.0.0`, then run
`python scripts/generate-dicom-gateway-fixtures.py`. These are developer-only
dependencies. The normal server and portable client do not require them.
