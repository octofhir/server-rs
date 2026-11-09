#!/usr/bin/env python3
"""Generate synthetic multiframe and JPEG SC fixtures; never use clinical input.

Developer-only dependencies: pydicom==3.0.1 and Pillow==12.0.0.
The production server and portable client do not depend on these packages.
"""
from argparse import ArgumentParser
from copy import deepcopy
from hashlib import sha256
from io import BytesIO
from pathlib import Path

from PIL import Image
import pydicom
from pydicom.encaps import encapsulate, generate_frames
from pydicom.tag import Tag
from pydicom.uid import (
    ExplicitVRLittleEndian,
    JPEGBaseline8Bit,
    MultiFrameGrayscaleByteSecondaryCaptureImageStorage,
)


def main():
    directory = Path(__file__).resolve().parents[1] / "crates/octofhir-server/tests/fixtures/dicom"
    parser = ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, default=directory / "synthetic-secondary-capture.dcm")
    parser.add_argument("--output", type=Path, default=directory)
    args = parser.parse_args()
    source = pydicom.dcmread(args.source)
    if source.PatientID != "OCTOFHIR-DICOM-TEST" or source.IssuerOfPatientID != "HOSPITAL":
        raise SystemExit("Input must be the repository's synthetic fixture")
    if source.PixelData != bytes(range(256)):
        raise SystemExit("Unexpected source pixel matrix")
    args.output.mkdir(parents=True, exist_ok=True)

    def identity(dataset, suffix):
        dataset.StudyInstanceUID = f"1.2.826.0.1.3680043.10.999.{suffix}"
        dataset.SeriesInstanceUID = f"{dataset.StudyInstanceUID}.1"
        dataset.SOPInstanceUID = f"{dataset.SeriesInstanceUID}.1"
        dataset.file_meta.MediaStorageSOPInstanceUID = dataset.SOPInstanceUID
        dataset.BurnedInAnnotation = "NO"
        dataset.PresentationLUTShape = "IDENTITY"
        dataset.InstanceNumber = 1
        dataset.SeriesNumber = 1

    multi = deepcopy(source)
    identity(multi, 2)
    multi.SOPClassUID = MultiFrameGrayscaleByteSecondaryCaptureImageStorage
    multi.file_meta.MediaStorageSOPClassUID = multi.SOPClassUID
    multi.file_meta.TransferSyntaxUID = ExplicitVRLittleEndian
    multi.NumberOfFrames = 2
    multi.FrameTime = "1000"
    multi.FrameIncrementPointer = Tag(0x0018, 0x1063)
    multi.PixelData = bytes(range(256)) + bytes(reversed(range(256)))
    multi["PixelData"].VR = "OB"
    multi.save_as(args.output / "synthetic-multiframe.dcm", enforce_file_format=True)

    compressed = deepcopy(source)
    identity(compressed, 3)
    compressed.Rows = compressed.Columns = 64
    pixels = bytes((x + y * 3) % 256 for y in range(64) for x in range(64))
    buffer = BytesIO()
    Image.frombytes("L", (64, 64), pixels).save(buffer, format="JPEG", quality=95, optimize=False)
    jpeg = buffer.getvalue()
    compressed.file_meta.TransferSyntaxUID = JPEGBaseline8Bit
    compressed.PixelData = encapsulate([jpeg])
    compressed["PixelData"].VR = "OB"
    compressed["PixelData"].is_undefined_length = True
    compressed.LossyImageCompression = "01"
    compressed.LossyImageCompressionMethod = "ISO_10918_1"
    compressed.LossyImageCompressionRatio = f"{len(pixels) / len(jpeg):.6f}"
    compressed.save_as(args.output / "synthetic-jpeg-baseline.dcm", enforce_file_format=True)

    for suffix, label, name in [(4, "A", "Иванов^Иван"), (5, "B", "Петров^Пётр")]:
        unicode_patient = deepcopy(source)
        identity(unicode_patient, suffix)
        unicode_patient.SpecificCharacterSet = "ISO_IR 192"
        unicode_patient.PatientID = f"OCTOFHIR-DICOM-CYRILLIC-{label}"
        unicode_patient.PatientName = name
        unicode_patient.InstitutionName = "OctoFHIR synthetic institution"
        unicode_patient.save_as(args.output / f"synthetic-cyrillic-{label.lower()}.dcm", enforce_file_format=True)

    reread = pydicom.dcmread(args.output / "synthetic-multiframe.dcm")
    assert reread.NumberOfFrames == 2 and reread.PixelData == multi.PixelData
    reread = pydicom.dcmread(args.output / "synthetic-jpeg-baseline.dcm")
    frames = list(generate_frames(reread.PixelData, number_of_frames=1))
    assert len(frames) == 1 and frames[0] in (jpeg, jpeg + b"\0")
    assert Image.open(BytesIO(frames[0])).size == (64, 64)
    for name in ["synthetic-multiframe.dcm", "synthetic-jpeg-baseline.dcm", "synthetic-cyrillic-a.dcm", "synthetic-cyrillic-b.dcm"]:
        print(name, sha256((args.output / name).read_bytes()).hexdigest())


if __name__ == "__main__":
    main()
