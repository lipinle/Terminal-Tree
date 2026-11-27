import os
import sys
import tempfile
import unittest
from unittest.mock import patch
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'src'))
import tree


class SafetyTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(dir='/tmp')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        env=patch.dict(os.environ,{'XDG_STATE_HOME':str(self.root/'state')})
        env.start(); self.addCleanup(env.stop)
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

    def test_corruption_after_cp_never_published(self):
        original=tree.subprocess.run
        def corrupt(args,**kwargs):
            result=original(args,**kwargs)
            with open(args[-1],'wb') as f: f.write(b'corrupt!')
            return result
        with patch.object(tree.subprocess,'run',side_effect=corrupt):
            with self.assertRaises(ValueError):
                tree.copy_item(str(self.src),str(self.dst),tree.identity(self.src))
        self.assertEqual(self.src.read_text(),'original')
        self.assertFalse(self.dst.exists())
        self.assertFalse(list(self.root.glob('.terminal-tree-copy-*')))
        self.assertEqual(tree.recovery_records()[0]['operation'],'copy')

    def test_copy_preserves_hardlinks_and_symlink(self):
        source=self.root/'directory'; source.mkdir()
        (source/'a').write_bytes(b'data')
        os.link(source/'a',source/'b')
        (source/'link').symlink_to('a')
        tree.copy_item(str(source),str(self.dst),tree.identity(source))
        self.assertEqual((self.dst/'a').stat().st_ino,(self.dst/'b').stat().st_ino)
        self.assertEqual(os.readlink(self.dst/'link'),'a')
        self.assertEqual(tree.content_manifest(source),tree.content_manifest(self.dst))

    def test_collision_during_copy_never_overwrites(self):
        original=tree.subprocess.run
        def collision(args,**kwargs):
            result=original(args,**kwargs)
            self.dst.write_text('concurrent')
            return result
        with patch.object(tree.subprocess,'run',side_effect=collision):
            with self.assertRaises(FileExistsError):
                tree.copy_item(str(self.src),str(self.dst),tree.identity(self.src))
        self.assertEqual(self.dst.read_text(),'concurrent')
        self.assertEqual(self.src.read_text(),'original')

    def test_journal_failure_refuses_move(self):
        with patch.object(tree,'journal_directory',side_effect=OSError('disk full')):
            with self.assertRaises(OSError):
                tree.move_item(str(self.src),str(self.dst),tree.identity(self.src))
        self.assertTrue(self.src.exists())
        self.assertFalse(self.dst.exists())

    def test_pending_record_after_interrupt(self):
        with self.assertRaises(KeyboardInterrupt):
            with tree.operation_log('copy',str(self.src),str(self.dst)):
                raise KeyboardInterrupt()
        self.assertEqual(tree.recovery_records()[0]['source'],str(self.src))
        self.assertFalse(self.dst.exists())

    def test_completed_record_not_pending(self):
        tree.move_item(str(self.src),str(self.dst),tree.identity(self.src))
        self.assertEqual(tree.recovery_records(),[])
        self.assertEqual(len(list(Path(tree.state_directory()).glob('*.done.json'))),1)

    def test_destination_replaced_refuses_rename(self):
        directory=self.root/'destination'; directory.mkdir()
        ident=tree.dir_identity(directory)
        directory.rename(self.root/'old-destination'); directory.mkdir()
        with self.assertRaises(ValueError):
            tree.rename_noreplace(str(self.src),str(directory/'file'),tree.identity(self.src),ident)
        self.assertTrue(self.src.exists())
        self.assertFalse((directory/'file').exists())

    def test_create_file_never_overwrites(self):
        self.dst.write_text('keep')
        with self.assertRaises(FileExistsError):
            tree.create_item(str(self.dst),False)
        self.assertEqual(self.dst.read_text(),'keep')

    def test_managed_state_protected(self):
        state=Path(tree.journal_directory())
        with self.assertRaises(ValueError): tree.guard_source(str(state))
        with self.assertRaises(ValueError): tree.guard_destination(str(state))

    def test_purge_outside_trash_refused(self):
        with self.assertRaises(ValueError):
            tree.purge_item(str(self.src),tree.identity(self.src),tree.tree_signature(str(self.src)))
        self.assertTrue(self.src.exists())

    def test_verify_metadata_difference(self):
        self.dst.write_bytes(self.src.read_bytes())
        os.chmod(self.src,0o600); os.chmod(self.dst,0o644)
        self.assertNotEqual(tree.content_manifest(self.src),tree.content_manifest(self.dst))


if __name__ == '__main__':
    unittest.main()
