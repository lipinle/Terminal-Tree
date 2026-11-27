import os
import shutil
import signal
import subprocess
from urllib.parse import quote
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

    def run_crash(self,code):
        prefix="import os, signal, sys; sys.path.insert(0,sys.argv[1]); import tree; src,dst=sys.argv[2:4]\n"
        result=subprocess.run([sys.executable,'-c',prefix+code,str(Path(tree.__file__).parent),
                               str(self.src),str(self.dst)],timeout=15)
        self.assertEqual(result.returncode,-signal.SIGKILL)

    def test_sigkill_before_publication_retains_source_and_journal(self):
        self.run_crash("""
def interrupted_cp(args,**kwargs):
    with open(args[-1],'wb') as f: f.write(b'partial')
    os.kill(os.getpid(),signal.SIGKILL)
tree.subprocess.run=interrupted_cp
tree.copy_item(src,dst,tree.identity(src))
""")
        self.assertEqual(self.src.read_text(),'original')
        self.assertFalse(self.dst.exists())
        records=tree.recovery_records()
        self.assertEqual(len(records),1)
        self.assertEqual(records[0]['operation'],'copy')
        self.assertEqual((Path(records[0]['stage'])/'payload').read_bytes(),b'partial')

    def test_sigkill_after_rename_keeps_pending_record(self):
        self.run_crash("""
with tree.operation_log('move',src,dst):
    tree.rename_noreplace(src,dst)
    os.kill(os.getpid(),signal.SIGKILL)
""")
        self.assertFalse(self.src.exists())
        self.assertEqual(self.dst.read_text(),'original')
        self.assertEqual(tree.recovery_records()[0]['operation'],'move')

    def test_reviewed_destination_identity_is_not_recaptured(self):
        folder=self.root/'folder'; folder.mkdir()
        expected=tree.dir_identity(folder)
        folder.rename(self.root/'old-folder'); folder.mkdir()
        for op in (tree.copy_item,tree.move_item):
            with self.assertRaises(ValueError):
                op(str(self.src),str(folder/'file'),tree.identity(self.src),expected)
        with self.assertRaises(ValueError):
            tree.create_item(str(folder/'new'),False,expected)
        self.assertEqual(list(folder.iterdir()),[])
        self.assertEqual(self.src.read_text(),'original')

    def mock_trash(self):
        trash=self.root/'data'/'Trash'; tf=trash/'files'; info=trash/'info'
        tf.mkdir(parents=True); info.mkdir()
        p=patch.multiple(tree,TRASH=str(trash),TF=str(tf)); p.start(); self.addCleanup(p.stop)
        return tf,info

    def test_restore_refuses_existing_destination(self):
        tf,info=self.mock_trash()
        item=tf/'item'; item.write_text('trashed')
        metadata=info/'item.trashinfo'
        metadata.write_text('[Trash Info]\nPath='+quote(str(self.dst))+'\n')
        self.dst.write_text('keep')
        with self.assertRaises(FileExistsError):
            tree.restore(str(item),tree.identity(item))
        self.assertEqual(item.read_text(),'trashed')
        self.assertEqual(self.dst.read_text(),'keep')
        self.assertTrue(metadata.exists())

    def test_restore_preserves_data_and_removes_metadata(self):
        tf,info=self.mock_trash()
        item=tf/'item'; item.write_text('trashed')
        metadata=info/'item.trashinfo'
        metadata.write_text('[Trash Info]\nPath='+quote(str(self.dst))+'\n')
        record=tree.restore_path(str(item))
        self.assertEqual(tree.restore(str(item),tree.identity(item),record),'Restored')
        self.assertEqual(self.dst.read_text(),'trashed')
        self.assertFalse(item.exists())
        self.assertFalse(metadata.exists())
        self.assertEqual(tree.recovery_records(),[])

    def test_purge_verified_directory_does_not_follow_symlink(self):
        tf,info=self.mock_trash()
        item=tf/'item'; item.mkdir(); (item/'nested').mkdir()
        (item/'nested'/'data').write_text('trash')
        (item/'link').symlink_to(self.src)
        metadata=info/'item.trashinfo'; metadata.write_text('metadata')
        tree.purge_item(str(item),tree.identity(item),tree.tree_signature(str(item)))
        self.assertFalse(item.exists())
        self.assertFalse(metadata.exists())
        self.assertEqual(self.src.read_text(),'original')

    def test_purge_detects_changed_nested_entry_during_deletion(self):
        tf,info=self.mock_trash()
        item=tf/'item'; item.mkdir()
        (item/'a').write_text('trash A'); (item/'b').write_text('trash B')
        original=os.unlink
        def change_next(name,*args,**kwargs):
            result=original(name,*args,**kwargs)
            if name=='a': (item/'b').write_text('new content to preserve')
            return result
        with patch.object(tree.os,'unlink',side_effect=change_next):
            with self.assertRaises(ValueError):
                tree.purge_item(str(item),tree.identity(item),tree.tree_signature(str(item)))
        self.assertEqual((item/'b').read_text(),'new content to preserve')
        self.assertEqual(tree.recovery_records()[0]['operation'],'permanent-delete')

    @unittest.skipUnless(shutil.which('gio'),'GIO not installed')
    def test_real_gio_trash_and_restore(self):
        tf,info=self.mock_trash()
        with patch.dict(os.environ,{'XDG_DATA_HOME':str(tf.parent.parent),'HOME':str(self.root)}):
            tree.gio_trash(str(self.src),tree.identity(self.src))
        self.assertFalse(self.src.exists())
        items=list(tf.iterdir())
        self.assertEqual(len(items),1)
        self.assertEqual(tree.restore(str(items[0]),tree.identity(items[0])),'Restored')
        self.assertEqual(self.src.read_text(),'original')
        self.assertEqual(list(info.iterdir()),[])


if __name__ == '__main__':
    unittest.main()
