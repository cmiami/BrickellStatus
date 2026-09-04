import importlib.util
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

spec = importlib.util.spec_from_file_location(
    "bundle_firmware",
    Path(__file__).resolve().parents[1] / "firmware/panel/scripts/bundle_firmware.py",
)
bundle = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bundle)


class FirmwareBundleTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.build = self.root / ".pio/build/panel"
        self.build.mkdir(parents=True)
        self.output = self.root / "output"
        for _, name in bundle.LAYOUT:
            (self.build / name).write_bytes(b"synthetic image")
        for target, value in (
            ("FIRMWARE_ROOT", self.root), ("OUTPUT_ROOT", self.output),
        ):
            patcher = patch.object(bundle, target, value)
            patcher.start()
            self.addCleanup(patcher.stop)
        patcher = patch.object(bundle, "boot_app0", return_value=self.build / "boot_app0.bin")
        patcher.start()
        self.addCleanup(patcher.stop)

    def test_current_image_is_copied_with_the_complete_flash_layout(self):
        (self.build / "firmware.bin").write_bytes(b"synthetic\0abc1234\0image")
        images = bundle.collect("panel", "abc1234")
        self.assertEqual([(image["offset"], image["file"]) for image in images], bundle.LAYOUT)
        for _, name in bundle.LAYOUT:
            self.assertEqual((self.output / "panel" / name).read_bytes(), (self.build / name).read_bytes())

    def test_old_and_dirty_images_are_not_relabelled_as_current(self):
        for value in (b"old1234\0", b"abc1234-dirty-other\0"):
            (self.build / "firmware.bin").write_bytes(value)
            self.assertIsNone(bundle.collect("panel", "abc1234"))
            self.assertFalse(self.output.exists())

    def test_unknown_source_identity_cannot_certify_a_cached_image(self):
        (self.build / "firmware.bin").write_bytes(b"unknown\0")
        self.assertIsNone(bundle.collect("panel", None))
        self.assertIsNone(bundle.collect("panel", "unknown"))
        self.assertFalse(self.output.exists())

    def test_missing_segment_leaves_no_partial_payload(self):
        (self.build / "firmware.bin").write_bytes(b"abc1234\0")
        (self.build / "partitions.bin").unlink()
        self.assertIsNone(bundle.collect("panel", "abc1234"))
        self.assertFalse(self.output.exists())
