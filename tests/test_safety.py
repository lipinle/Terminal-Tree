import os
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'src'))
import tree


class SafetyTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(dir='/tmp')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.src = self.root / 'source\nfile.txt'
        self.dst = self.root / 'destination.txt'
        self.src.write_text('original')

    def test_copy_preserves_original(self):
        tree.copy_item(str(self.src), str(self.dst), tree.identity(self.src))
        self.assertEqual(self.src.read_text(), self.dst.read_text())

    def test_no_overwrite(self):
        self.dst.write_text('keep')
        for op in (tree.copy_item, tree.move_item):
            with self.assertRaises(FileExistsError):
                op(str(self.src), str(self.dst), tree.identity(self.src))
        self.assertEqual(self.dst.read_text(), 'keep')
        self.assertEqual(self.src.read_text(), 'original')

    def test_stale_selection(self):
        signature = tree.identity(self.src)
        self.src.write_text('changed data')
        with self.assertRaises(ValueError):
            tree.copy_item(str(self.src), str(self.dst), signature)
        self.assertFalse(self.dst.exists())

    def test_move_no_overwrite(self):
        tree.move_item(str(self.src), str(self.dst), tree.identity(self.src))
        self.assertFalse(self.src.exists())
        self.assertEqual(self.dst.read_text(), 'original')

    def test_symlink_preserved(self):
        link = self.root / 'link'
        link.symlink_to(self.src)
        tree.copy_item(str(link), str(self.dst), tree.identity(link))
        self.assertTrue(self.dst.is_symlink())
        self.assertEqual(os.readlink(self.dst), str(self.src))

    def test_root_protected(self):
        with self.assertRaises(ValueError):
            tree.guard_source('/')
        with self.assertRaises(ValueError):
            tree.guard_destination('/')


if __name__ == '__main__':
    unittest.main()
