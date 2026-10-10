"""Current live-owner artifact bridge controls; synthetic executables, no Cargo."""
import copy
import importlib.util
import json
import os
from pathlib import Path
import sys
import unittest
from unittest.mock import patch
import test_cargo_admitted_helper_route as support


class HelperArtifactControls(unittest.TestCase):
    def setUp(self):
        fixture = support.HelperRouteTests()
        fixture.setUp()
        self.addCleanup(fixture.doCleanups)
        self.f = fixture
        spec = importlib.util.spec_from_file_location('helper_artifact_controls', fixture.actual/'scripts/ci/helper_artifact_prepare.py')
        self.a = importlib.util.module_from_spec(spec);spec.loader.exec_module(self.a)
        self.enterContext(patch.object(self.a,'owner',fixture.a))
        import perllsp_workspace_prepare as product
        self.enterContext(patch.object(product,'owner',fixture.a))
        self.env = fixture.n.env
        self.records = []
        for key,target in self.a.expected(fixture.mapping).items():
            package,name,kind,test = key
            binary=fixture.n.paths['target']/'debug'
            if test:binary=fixture.n.paths['build']/'debug/deps'/(name.replace('-','_')+'-123abc')
            else:binary=binary/name
            binary.parent.mkdir(parents=True,exist_ok=True)
            binary.write_text('#!/bin/sh\nexit 0\n');binary.chmod(0o755)
            root=fixture.n.worktree
            self.records.append({'reason':'compiler-artifact','manifest_path':str(root/('xtask' if package=='xtask' else 'crates/'+package)/'Cargo.toml'),
                'target':{'name':name,'kind':list(kind),'src_path':str(root/target['src_path'])},
                'profile':{'test':test},'executable':str(binary),'filenames':[str(binary)]})
        self.location=fixture.n.paths['temp']/('helper-artifacts-'+str(os.getpid())+'.json')
    def log(self,records=None,terminal=True):
        return '\n'.join(json.dumps(record) for record in (self.records if records is None else records))+('\n'+json.dumps({'reason':'build-finished','success':terminal}) if terminal is not None else '')+'\n'
    def capture(self):return self.a.capture(self.log(),self.env)
    def test_complete_artifacts_and_compile_time_candidate_bind_original_live_owner(self):
        child=self.capture();candidate=self.f.n.paths['target']/'debug/xtask'
        self.assertEqual(self.a.validate(child,str(candidate)),candidate)
        receipt,_=self.f.a.bounded_json(str(self.location),limit=1024*1024)
        self.assertEqual(len(receipt['artifacts']),309)
        self.assertEqual(sum(row['key'][3] for row in receipt['artifacts']),277)
        self.assertEqual(receipt['binding']['descriptor']['marker_identity'],self.f.n.receipt['marker_identity'])
    def test_missing_harness_normal_bin_duplicate_failed_and_malformed_terminal_refuse(self):
        normal=next(row for row in self.records if row['target']['name']=='xtask' and row['profile']['test'] is False)
        logs=[self.log(self.records[1:]),self.log([row for row in self.records if row is not normal]),
              self.log([*self.records,self.records[0]]),self.log(terminal=False),self.log(terminal=None),self.log()+'{}\n','{broken\n',self.log().replace('"success": true','"success": 1')]
        for log in logs:
            self.location.unlink(missing_ok=True)
            with self.subTest(log=log[:80]),self.assertRaises(self.f.a.Denied):self.a.capture(log,self.env)
    def test_wrong_source_private_root_and_test_profile_refuse(self):
        for change in ('src_path','executable','test'):
            rows=copy.deepcopy(self.records);row=next(r for r in rows if r['target']['name']=='xtask' and r['profile']['test'] is False)
            if change=='src_path':row['target']['src_path']='/other/main.rs'
            elif change=='executable':row['executable']='/other/xtask';row['filenames']=[row['executable']]
            else:row['profile']['test']=True
            with self.subTest(change=change),self.assertRaises(self.f.a.Denied):self.a.capture(self.log(rows),self.env)
    def test_changed_bytes_missing_artifact_and_foreign_candidate_refuse(self):
        child=self.capture();candidate=self.f.n.paths['target']/'debug/xtask'
        with self.assertRaises(self.f.a.Denied):self.a.validate(child,str(self.f.n.paths['target']/'release/xtask'))
        candidate.write_text('#!/bin/sh\nexit 1\n')
        with self.assertRaises(self.f.a.Denied):self.a.validate(child,str(candidate))
        candidate.unlink()
        with self.assertRaises(self.f.a.Denied):self.a.validate(child)
    def test_released_or_replaced_original_owner_refuses(self):
        child=self.capture()
        marker=Path(self.f.n.receipt['lease_marker']);marker.rename(marker.with_name('foreign-owner'))
        with self.assertRaises(self.f.a.Denied):self.a.validate(child)
    def test_missing_handoff_and_changed_interpreter_refuse_no_fallback(self):
        with self.assertRaises(self.f.a.Denied):self.a.validate(self.env)
        child=self.capture();child[self.a.PYTHON]='/other/python'
        with self.assertRaises(self.f.a.Denied):self.a.validate(child)
    def test_harness_in_final_target_instead_of_bound_build_root_refuses(self):
        rows=copy.deepcopy(self.records)
        row=next(r for r in rows if r['profile']['test'] is True)
        wrong=self.f.n.paths['target']/'debug/deps'/Path(row['executable']).name
        wrong.parent.mkdir(parents=True,exist_ok=True);wrong.write_text('#!/bin/sh\nexit 0\n');wrong.chmod(0o755)
        row['executable']=str(wrong);row['filenames']=[str(wrong)]
        with self.assertRaisesRegex(self.f.a.Denied,'private executable/profile/root'):
            self.a.capture(self.log(rows),self.env)
