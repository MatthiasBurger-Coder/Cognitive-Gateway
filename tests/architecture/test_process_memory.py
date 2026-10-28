"""Resource evidence must remain measured and retain platform-specific units."""
import importlib.util
from pathlib import Path
import sys
from types import SimpleNamespace
import unittest
from unittest.mock import patch, Mock

SPEC = importlib.util.spec_from_file_location(
    'process_memory', Path(__file__).resolve().parents[2] / 'tests/scripts/process_memory.py')
memory = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(memory)


class ProcessMemoryTests(unittest.TestCase):
    def test_native_measurement_is_positive_and_does_not_decrease(self):
        before = memory.peak_rss_bytes()
        allocation = bytearray(4 * 1024 * 1024)
        self.assertGreater(before, 0)
        self.assertGreaterEqual(memory.peak_rss_bytes(), before)
        self.assertEqual(len(allocation), 4 * 1024 * 1024)

    def test_posix_units(self):
        resource = SimpleNamespace(RUSAGE_SELF=0, getrusage=lambda _: SimpleNamespace(ru_maxrss=123))
        for platform, expected in [('linux', 123 * 1024), ('darwin', 123)]:
            with self.subTest(platform=platform), patch.object(sys, 'platform', platform), \
                 patch.dict(sys.modules, {'resource': resource}):
                self.assertEqual(memory.peak_rss_bytes(), expected)

    @unittest.skipUnless(sys.platform == 'win32', 'Native WinDLL error behavior requires Windows')
    def test_windows_measurement_failure_is_not_fabricated_evidence(self):
        import ctypes
        library = Mock()
        library.GetProcessMemoryInfo.return_value = 0
        with patch.object(ctypes, 'WinDLL', return_value=library), \
             patch.object(ctypes, 'get_last_error', return_value=5):
            with self.assertRaises(OSError):
                memory.peak_rss_bytes()
