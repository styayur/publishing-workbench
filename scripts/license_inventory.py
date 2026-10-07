"""Create resolved dependency metadata and collect upstream license texts for releases."""
import json,subprocess,zipfile
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
OUT=ROOT/'test-results/release';OUT.mkdir(parents=True,exist_ok=True)
cargo=json.loads(subprocess.check_output(['cargo','metadata','--locked','--format-version','1','--manifest-path',str(ROOT/'src-tauri/Cargo.toml')],text=True))
entries=[];directories=[]
for p in cargo['packages']:
 if p['name']=='publishing-workbench':continue
 entries.append({'ecosystem':'cargo','name':p['name'],'version':p['version'],'license':p['license'],'repository':p['repository']});directories.append(('cargo/'+p['name']+'-'+p['version'],Path(p['manifest_path']).parent))
for pkg in (ROOT/'node_modules').glob('**/package.json'):
 try:p=json.loads(pkg.read_text(encoding='utf-8'))
 except (ValueError,UnicodeError):continue
 if not p.get('name') or not p.get('version'):continue
 entries.append({'ecosystem':'npm','name':p['name'],'version':p['version'],'license':p.get('license'),'repository':p.get('repository')});directories.append(('npm/'+p['name']+'-'+p['version'],pkg.parent))
(OUT/'third-party-licenses.json').write_text(json.dumps(entries,indent=2,ensure_ascii=False)+'\n',encoding='utf-8')
with zipfile.ZipFile(OUT/'dependency-license-texts.zip','w',zipfile.ZIP_DEFLATED,strict_timestamps=False) as z:
 for prefix,d in directories:
  for f in d.iterdir():
   if f.is_file() and f.name.upper().startswith(('LICENSE','LICENCE','COPYING','NOTICE','COPYRIGHT','UNLICENSE')):z.write(f,prefix+'/'+f.name)
print(f'Collected {len(entries)} package metadata entries')
