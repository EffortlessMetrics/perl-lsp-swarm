#!/usr/bin/env python3
"""Exercise the installed secret scanner with synthetic markers in disposable Git repos."""
import os, subprocess, tempfile, pathlib, re, sys, shutil

root_dir = pathlib.Path(__file__).resolve().parents[2]
source_path = pathlib.Path(sys.argv[1]) if len(sys.argv) > 1 else root_dir / "crates/perl-ci-hygiene/src/git_hooks.rs"
hook_path = pathlib.Path(sys.argv[2]) if len(sys.argv) > 2 else root_dir / "hooks/pre-push"
real_grep = shutil.which("grep")
assert real_grep is not None, "grep is required for fixture controls"
source=source_path.read_text()
def part(name): return re.search(r'const '+name+r': &str = r#"(.*?)"#;',source,re.S).group(1)
hook=''.join(part(n) for n in ('PRE_COMMIT_HEAD','SECRET_SCAN_SH','PRE_COMMIT_TAIL'))
env=os.environ.copy()
for key in subprocess.check_output(['git','rev-parse','--local-env-vars'],text=True).splitlines(): env.pop(key,None)
# Isolate only fixture subprocesses from caller configuration and templates.
for key in list(env):
 if key == "GIT_CONFIG" or key.startswith("GIT_CONFIG_") or key == "GIT_TEMPLATE_DIR": env.pop(key)
env["GIT_CONFIG_NOSYSTEM"] = "1"
env["GIT_CONFIG_GLOBAL"] = os.devnull
failed=False
cases=[('token','token',None,1),('benign','benign',None,0),('generic','generic',None,1),('excluded','excluded',None,0),('single-error','token','single',1),('generic-first-error','generic','first',1),('generic-filter-error','generic','filter',1),('generic-count-error','generic','count',1)]
with tempfile.TemporaryDirectory(prefix='scan18467-') as d:
 root=pathlib.Path(d); bin=root/'bin'; bin.mkdir()
 (bin/'cargo').write_text('#!/bin/sh\nexit 0\n'); (bin/'cargo').chmod(0o755)
 for label,content,mode,expected in cases:
  repo=root/label; repo.mkdir()
  def run(*args,env=env): return subprocess.run(args,cwd=repo,env=env,text=True,stdout=subprocess.PIPE,stderr=subprocess.STDOUT)
  assert run('git','init','-q').returncode==0
  run('git','config','user.name','Fixture Test'); run('git','config','user.email','fixture@example.invalid')
  hp=repo/'.git/hooks/pre-commit'; hp.write_text(hook); hp.chmod(0o755)
  data={'token':'value = '+'ghp_'+'FAKEFAKEFAKEFAKEFAKEFAKEFAKEFAKE0000','benign':'ordinary text','generic':'api_key = "'+'1234567890abcdefghij'+'"','excluded':'api_key = computed_identifier_name;'}
  (repo/'fixture.txt').write_text(data[content]+'\n'); assert run('git','add','fixture.txt').returncode==0
  shadow=root/('shadow-'+label); shadow.mkdir()
  if mode:
   # Match scanner invocation shape without affecting other commands.
   conditions={'single':'[ "$1" = -a ] && [ "$3" = -c ]','first':'[ "$1" = -a ] && [ "$3" = -- ] && case "$4" in *Aa*) true;; *) false;; esac','filter':'[ "$1" = -a ] && [ "$3" = -v ]','count':'[ "$1" = -a ] && [ "$2" = -c ]'}
   p=shadow/'grep'; p.write_text('#!/bin/bash\nif '+conditions[mode]+'; then exit 2; fi\nexec "'+real_grep+'" "$@"\n'); p.chmod(0o755)
  e=env.copy(); e['PATH']=str(shadow)+os.pathsep+str(bin)+os.pathsep+env['PATH']
  result=run('git','commit','-m','fixture',env=e); landed=run('git','rev-parse','--verify','HEAD').returncode==0
  ok=(result.returncode==0)==(expected==0) and (landed==(expected==0)) and (not mode or 'could not evaluate' in result.stdout)
  print(label,'exit',result.returncode,'landed',landed,'unevaluated','could not evaluate' in result.stdout,'PASS' if ok else 'FAIL')
  if not ok: failed=True; print(result.stdout)

# Actual checked-in hook, invoked by git push to a disposable bare remote.
# History-only cases add and remove the file before the push, so the endpoint
# diff contains no additions; only the per-commit log leg can discover them.
push_hook = hook_path.read_text()
scan = re.search(r'# SECRET_SCAN_SH_BEGIN\n.*?# SECRET_SCAN_SH_END', source, re.S).group()
assert scan in push_hook, 'generated and tracked scanner bytes differ'
with tempfile.TemporaryDirectory(prefix='scan18467-push-') as d:
 root=pathlib.Path(d)
 for leg in ('range','history'):
  for kind in ('benign','secret','error','first','filter','count','git-list','git-log-list','git-diff','git-log'):
   name=leg+'-'+kind; repo=root/name; repo.mkdir(); remote=root/(name+'.git')
   def run(*args,env=env): return subprocess.run(args,cwd=repo,env=env,text=True,stdout=subprocess.PIPE,stderr=subprocess.STDOUT)
   def ok(*args):
    result=run(*args)
    assert result.returncode == 0, (args,result.stdout)
    return result.stdout.strip()
   ok('git','init','-q','-b','main'); ok('git','config','user.name','Fixture Test'); ok('git','config','user.email','fixture@example.invalid')
   ok('git','init','-q','--bare',str(remote)); ok('git','remote','add','origin',str(remote))
   (repo/'base.txt').write_text('initial\n'); ok('git','add','base.txt'); ok('git','commit','-qm','base'); ok('git','push','-q','origin','main')
   base=ok('git','rev-parse','HEAD')
   target=repo/'fixture.txt'; target.write_text('ordinary text\n' if kind=='benign' else ('api_key = "'+'1234567890abcdefghij'+'"\n' if kind in ('first','filter','count') else 'value = '+'ghp_'+'FAKEFAKEFAKEFAKEFAKEFAKEFAKEFAKE0000'+'\n'))
   ok('git','add','fixture.txt'); ok('git','commit','-qm','add fixture')
   if leg=='history':
    ok('git','rm','-q','fixture.txt'); ok('git','commit','-qm','remove fixture')
    assert not ok('git','diff','--name-only',base,'HEAD'), 'history fixture must have an empty endpoint diff'
   hp=repo/'.git/hooks/pre-push'; hp.write_text(push_hook); hp.chmod(0o755)
   shadow=root/('shadow-'+name); shadow.mkdir()
   # Only the scanner's count grep fails. All unrelated hook greps use real grep.
   if kind in ('error','first','filter','count'):
    conditions={'error':'[ "$1" = -a ] && [ "$3" = -c ]','first':'[ "$1" = -a ] && [ "$3" = -- ] && case "$4" in *Aa*) true;; *) false;; esac','filter':'[ "$1" = -a ] && [ "$3" = -v ]','count':'[ "$1" = -a ] && [ "$2" = -c ]'}
    p=shadow/'grep'; p.write_text('#!/bin/bash\nif '+conditions[kind]+'; then exit 2; fi\nexec "'+real_grep+'" "$@"\n'); p.chmod(0o755)
   if kind.startswith('git-'):
    conditions={'git-list':'[ "$1" = -c ] && [ "$3" = diff ]', 'git-log-list':'[ "$1" = -c ] && [ "$3" = log ]', 'git-diff':'[ "$1" = --literal-pathspecs ] && [ "$4" = diff ]', 'git-log':'[ "$1" = --literal-pathspecs ] && [ "$4" = log ]'}
    p=shadow/'git'; p.write_text('#!/bin/bash\nif '+conditions[kind]+'; then exit 2; fi\nexec "'+shutil.which('git')+'" "$@"\n'); p.chmod(0o755)
    hp.write_text(push_hook.replace('\n', '\nexport PATH="'+str(shadow)+':$PATH"\n', 1))
   # Fixture-only stub of the post-scan gate; never changes the hook itself.
   p=shadow/'just'; p.write_text('#!/bin/sh\nexit 0\n'); p.chmod(0o755)
   e=env.copy(); e['PATH']=str(shadow)+os.pathsep+env['PATH']
   result=run('git','push','origin','main',env=e)
   remote_sha=ok('git','--git-dir',str(remote),'rev-parse','refs/heads/main')
   allowed=kind=='benign'; expected_report='Secret scan refused this push' if kind=='secret' else 'could not evaluate'
   passed=(result.returncode==0)==allowed and (remote_sha != base)==allowed and (allowed or expected_report in result.stdout)
   print(name,'exit',result.returncode,'remote_advanced',remote_sha!=base,'PASS' if passed else 'FAIL')
   if not passed: failed=True; print(result.stdout)
if failed: sys.exit(1)
