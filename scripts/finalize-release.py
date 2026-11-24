#!/usr/bin/env python3
"""Revision-8 Root finalizer: prove same-host delivery, then publish the same bytes on GitHub.

Revision-8 merges the two halves that review task-114 found unjoined. The bundle that is staged
and the bundle that the delivery chain reads are now the same bytes: one helper (the revision-8
helper) both writes the manifest and verifies it, and the frozen task-98 adapter is used through
a revision-8 subclass that supplies the one argument the helper needs - the release source root.
The adapter's staging, no-overwrite rename, re-verification, `flock` pointer compare-and-swap and
both `go=False` defaults stay the frozen ones. Default HOLD.
"""
import argparse,hashlib,json,os,platform,re,shutil,stat,subprocess,sys,tempfile
from pathlib import Path
from urllib.parse import quote,urlsplit,urljoin,unquote
from urllib.request import Request,build_opener,HTTPRedirectHandler,ProxyHandler
from urllib.error import HTTPError

ORIGIN='https://console.saisi.online/installers/releases'
DEVICE='4c718d6e44c83388c7e07bd0d811fa0b7cf22e2fced85d35d0021dd26b11f687'
MAX_BYTES=128*1024*1024
E=Path('/home/zss/summrise-product/.superpowers/sdd/continuation-cad0a3a9/full-product-ui-refactor/a3-redesign/continuation-e7a56696')
TRUSTED_HELPER=E/'release-bundle-prep/revision-8/helper/stage-release-bundle.py'
TRUSTED_HELPER_SHA='6931397262940d2b86dc348c16cc25c2462b484858e1dea3d73f16ab30590282'
TRUSTED_CLI=E/'trusted-cli-copy-1/summrise-release'
TRUSTED_CLI_SHA='c9d5a844a258ff19bf1d27ddfef0e9f7429368ebdc71429e11b0c0b70dafb951'
TRUSTED_CLI_BYTES=3019024
ADAPTER_MODULE=E/'release-bundle-prep/revision-8/adapter/mirror_adapter.py'
ADAPTER_MODULE_SHA='8d1014eec130856a1cec361b87f1844f3f8411f7f2163e8b525ce202acae4ed5'
FROZEN_ADAPTER_SHA='b1e0fa1ac3bafda474cd2712b36eb65d5dea7a1ec1f2270909688da38ba4dc97'
# The manifest keeps the six revision-5 metadata keys unchanged and adds one metadata key,
# `tooling`, beside them: the CLI bytes that signed the bundle and the release sources that
# built that CLI. `artifacts` and `files` stay as they were. The tooling section is a new key
# rather than a fold into an existing one because every existing key already carries a fixed
# meaning (schema, sourceSha, version, channel, mirror, minVersion) that other readers compare
# byte for byte. Both the finalizer and the adapter read the nine-key contract through the same
# pinned helper, and the finalizer re-checks it independently.
INVENTORY_METADATA_KEYS=('schema','sourceSha','version','channel','mirror','minVersion','tooling')
INVENTORY_KEYS=set(INVENTORY_METADATA_KEYS)|{'artifacts','files'}
class Refusal(Exception):pass
def need(value,message):
 if not value:raise Refusal(message)
def sha(raw):return hashlib.sha256(raw).hexdigest()
def pin(path):
 need(path.is_file() and not path.is_symlink(),'non-regular input');b=path.read_bytes();return {'sha256':sha(b),'bytes':len(b)}
def names(c):
 stems=[f'summrise-{r}-{c["version"]}-{c["channel"]}' for r in ('launch','device','console','shell')]+[f'SummriseSetup-{c["version"]}-{c["channel"]}']
 return set([s+'.exe' for s in stems]+[s+'.sidecar.json' for s in stems]+[f'release-{c["version"]}-{c["channel"]}.json',f'release-{c["version"]}-{c["channel"]}.sidecar.json','inventory.json','inventory.sidecar.json','SHA256SUMS','READY'])
def asset_map(rows):
 need(isinstance(rows,list),'invalid assets');out={};ids=set()
 for row in rows:
  ident,name,size,state=(row.get(k) for k in ('id','name','size','state'))
  need(type(ident) is int and ident>0 and ident not in ids,'duplicate/invalid asset ID')
  need(isinstance(name,str) and re.fullmatch(r'[A-Za-z0-9_.-]+',name) and name not in out and name not in ('.','..'),'duplicate/invalid asset name')
  need(type(size) is int and 0<size<=MAX_BYTES and state=='uploaded','asset bytes/state invalid')
  ids.add(ident);out[name]=(ident,size,state)
 return out

def validate_config(c):
 need(c.get('schema')==1,'config schema')
 need(re.fullmatch(r'[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+',c.get('repository','')),'repository')
 need(re.fullmatch(r'[0-9a-f]{40}',c.get('source_sha','')),'source SHA')
 need(re.fullmatch(r'(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)',c.get('version','')),'version')
 need(c.get('channel') in ('stable','canary') and c.get('tag')=='v'+c['version'],'channel/tag')
 for key in ('release_id','installer_run_id','ci_run_id'):need(type(c.get(key)) is int and c[key]>0,'invalid identity '+key)
 need(c.get('artifact_origin')==ORIGIN and c.get('artifact_base')==ORIGIN+'/v'+c['version']+'/'+c['channel'],'permanent artifact origin/base')
 need(c.get('proof_base')=='https://github.com/'+c['repository']+'/releases/download/'+c['tag'],'GitHub proof base')
 for key in ('launcher_public_key','release_public_key','inventory_sha256'):need(re.fullmatch(r'[0-9a-f]{64}',c.get(key,'')),'invalid trusted '+key)
 need(set(asset_map(c.get('expected_assets')))==names(c),'complete expected asset set')
 need(not any(k.lower() in ('token','password','secret','authorization') for k in c),'credentials must stay in memory')
 need(bool(re.fullmatch(r'(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)',c.get('min_version') or '')),'minimum version')
 helper=c.get('helper') or {};signer=c.get('signer') or {}
 need(Path(str(helper.get('path'))).resolve()==TRUSTED_HELPER and helper.get('pin',{}).get('sha256')==TRUSTED_HELPER_SHA,'only the reviewed revision-8 helper may be used')
 need(Path(str(signer.get('path'))).resolve()==TRUSTED_CLI and signer.get('pin',{}).get('sha256')==TRUSTED_CLI_SHA and signer.get('pin',{}).get('bytes')==TRUSTED_CLI_BYTES,'only the read-only trusted native CLI copy may be used')
 source_root=Path(str(c.get('release_source_root') or ''))
 need(source_root.is_absolute() and source_root.is_dir() and not source_root.is_symlink(),'release source root missing')
 signing=c.get('release_cli') or {}
 need(re.fullmatch(r'[0-9a-f]{64}',str(signing.get('sha256') or '')),'the signing CLI pin is missing')
 need(type(signing.get('bytes')) is int and signing['bytes']>0,'the signing CLI byte length is missing')

def validate_authority(c,s,draft):
 need(s['main'].get('sha')==s['tag'].get('sha')==c['source_sha'],'main/tag/source SHA differs')
 for label,ident,branch,path,events in [('ci',c['ci_run_id'],'main','.github/workflows/ci.yml',('push',)),('installer',c['installer_run_id'],c['tag'],'.github/workflows/installer.yml',('push','workflow_dispatch'))]:
  row=s[label]
  need(row.get('id')==ident and row.get('head_sha')==c['source_sha'] and row.get('head_branch')==branch and row.get('path')==path and row.get('event') in events and row.get('status')=='completed' and row.get('conclusion')=='success',label+' authority differs')
 r=s['release'];need(r.get('id')==c['release_id'] and r.get('tag_name')==c['tag'] and r.get('target_commitish')==c['source_sha'] and r.get('draft') is draft,'release identity/state differs')
 need(asset_map(r.get('assets'))==asset_map(c['expected_assets']),'release asset identity changed')
 if not draft:need(r.get('prerelease') is (c['channel']=='canary'),'published channel state differs')

def native_redirect_allowed(origin,next_url):
 a,b=urlsplit(origin),urlsplit(next_url)
 need(b.scheme==a.scheme=='https' and b.hostname==a.hostname and not b.username and not b.password,'native redirect scheme/host differs')

class NoRedirect(HTTPRedirectHandler):
 def redirect_request(self,*args,**kwargs):return None

def http_transport(method,url,headers,body,limit):
 opener=build_opener(ProxyHandler({}),NoRedirect())
 try:response=opener.open(Request(url,data=body,headers=headers,method=method),timeout=30)
 except HTTPError as e:response=e
 except Exception:raise Refusal('HTTP transport failed') from None
 with response:
  data=response.read(limit+1);need(len(data)<=limit,'HTTP response exceeds bound')
  return response.status,dict(response.headers),data

def fetch_public(url,size,transport=http_transport,native=False):
 origin=url;current=url
 for hop in range(6):
  need(urlsplit(current).scheme=='https','public URL must be HTTPS')
  status,headers,data=transport('GET',current,{'User-Agent':'SummriseRootFinalizer/1'},None,size)
  if native:need(status==200 and not headers.get('Location',headers.get('location')),'native proof requires direct 200 without Location')
  if status==200:need(len(data)==size,'download byte length differs');return data
  need(status in (301,302,303,307,308) and hop<5,'public download status')
  target=urljoin(current,headers.get('Location',headers.get('location','')))
  if native:native_redirect_allowed(origin,target)
  else:need(urlsplit(target).hostname in ('github.com','release-assets.githubusercontent.com','objects.githubusercontent.com'),'GitHub download redirect host')
  current=target
 raise Refusal('redirect limit')

class GitCredentialAuth:
 """Existing normal Git HTTPS auth only. Never echo command streams or put token in argv."""
 def __init__(self,repository,checkout,runner=subprocess.run):self.repository=repository;self.checkout=checkout;self.runner=runner
 def token(self):
  env=dict(os.environ,GIT_OPTIONAL_LOCKS='0',GIT_TERMINAL_PROMPT='0',GCM_INTERACTIVE='Never',GIT_ASKPASS='/bin/false')
  try:
   p=self.runner(['git','--no-optional-locks','remote','get-url','origin'],cwd=self.checkout,env=env,capture_output=True,timeout=15)
   need(p.returncode==0,'normal Git authentication unavailable')
   u=urlsplit(p.stdout.decode().strip())
   need(u.scheme=='https' and u.hostname=='github.com' and u.path.rstrip('/').removesuffix('.git')=='/'+self.repository,'Git origin differs')
   if u.password:return unquote(u.password)
   query=('protocol=https\nhost=github.com\npath='+self.repository+'.git\n\n').encode()
   p=self.runner(['git','--no-optional-locks','credential','fill'],cwd=self.checkout,env=env,input=query,capture_output=True,timeout=15)
   need(p.returncode==0,'normal Git authentication unavailable')
   fields=dict(line.split('=',1) for line in p.stdout.decode().splitlines() if '=' in line)
   need(fields.get('protocol')=='https' and fields.get('host')=='github.com' and bool(fields.get('password')),'normal Git credential differs')
   return fields['password']
  except Refusal:raise
  except Exception:raise Refusal('normal Git authentication unavailable') from None

class GitHubAPI:
 def __init__(self,repository,token,transport=http_transport):self.repository=repository;self._token=token;self.transport=transport
 def _request(self,method,path,payload=None):
  need(path.startswith('/repos/'+self.repository+'/') or path=='/repos/'+self.repository,'API path outside repository')
  headers={'Authorization':'Bearer '+self._token,'Accept':'application/vnd.github+json','X-GitHub-Api-Version':'2022-11-28','User-Agent':'SummriseRootFinalizer/1'}
  if payload is not None:headers['Content-Type']='application/json'
  code,_,raw=self.transport(method,'https://api.github.com'+path,headers,None if payload is None else json.dumps(payload).encode(),4*1024*1024)
  need(code==200,'GitHub API request refused')
  try:return json.loads(raw)
  except Exception:raise Refusal('GitHub API JSON malformed') from None
 def snapshot(self,c):
  base='/repos/'+self.repository
  repo=self._request('GET',base);need(repo.get('full_name')==self.repository and repo.get('permissions',{}).get('push') is True,'normal identity lacks repository push')
  return {'main':self._request('GET',base+'/commits/main'),'tag':self._request('GET',base+'/commits/'+quote(c['tag'],safe='')),'ci':self._request('GET',base+'/actions/runs/'+str(c['ci_run_id'])),'installer':self._request('GET',base+'/actions/runs/'+str(c['installer_run_id'])),'release':self._request('GET',base+'/releases/'+str(c['release_id']))}
 def download_asset(self,asset_id,size):
  url='https://api.github.com/repos/'+self.repository+'/releases/assets/'+str(asset_id)
  status,headers,data=self.transport('GET',url,{'Authorization':'Bearer '+self._token,'Accept':'application/octet-stream','X-GitHub-Api-Version':'2022-11-28','User-Agent':'SummriseRootFinalizer/1'},None,size)
  if status==200:need(len(data)==size,'asset byte length differs');return data
  need(status in (301,302,303,307,308),'asset download status')
  target=headers.get('Location',headers.get('location',''));need(urlsplit(target).scheme=='https' and urlsplit(target).hostname in ('release-assets.githubusercontent.com','objects.githubusercontent.com','github.com'),'asset redirect host')
  return fetch_public(target,size,self.transport)
 def publish(self,c):return self._request('PATCH','/repos/'+self.repository+'/releases/'+str(c['release_id']),{'draft':False,'prerelease':c['channel']=='canary','make_latest':'false' if c['channel']=='canary' else 'true'})
 def public_asset(self,url,size):return fetch_public(url,size,self.transport)

class PublicationLock:
 """Exclusive local ownership; never remove a stale or foreign lock automatically."""
 def __init__(self,path):self.path=path;self.owned=None
 def acquire(self):
  p=self.path;need(p.is_absolute() and p.resolve()==p and p.parent.is_dir(),'publication lock parent/path')
  for part in (p.parent,*p.parent.parents):need(not part.is_symlink(),'publication lock symlink')
  try:fd=os.open(p,os.O_WRONLY|os.O_CREAT|os.O_EXCL|getattr(os,'O_NOFOLLOW',0),0o600)
  except FileExistsError:raise Refusal('publication lock already owned') from None
  with os.fdopen(fd,'wb') as out:
   raw=json.dumps({'pid':os.getpid(),'owner_nonce':os.urandom(16).hex()}).encode();out.write(raw);out.flush();os.fsync(out.fileno());info=os.fstat(out.fileno())
  self.owned=(info.st_dev,info.st_ino,raw)
 def release(self):
  if self.owned is None:return
  p=self.path;need(p.resolve()==p and p.is_file() and not p.is_symlink(),'publication lock target changed');info=p.stat()
  need((info.st_dev,info.st_ino,p.read_bytes())==self.owned,'publication lock owner changed')
  p.unlink();self.owned=None

def validate_mapping(c):
 m=c.get('mapping');need(isinstance(m,dict) and m.get('confirmed') is True,'production mapping remains unknown')
 need(m.get('kind')=='nginx-static-v1','production mapping kind differs')
 root=Path(str(m.get('artifact_root','')));need(root.is_absolute() and root.is_dir() and root.resolve()==root,'production artifact root missing or not canonical')
 for part in (root,*root.parents):need(not part.is_symlink(),'production root contains symlink')
 pointer=Path(str(m.get('pointer_path','')));need(pointer.is_absolute() and pointer.parent.is_dir() and not pointer.is_relative_to(root),'pointer path missing or inside the public root')
 receipt=Path(str(m.get('receipt_path','')));need(receipt.is_absolute() and pin(receipt)==m.get('receipt_pin'),'mapping receipt pin differs')
 body=json.loads(receipt.read_bytes())
 need(body.get('root_author')=='lead' and body.get('confirmed') is True and body.get('artifact_root')==str(root) and body.get('artifact_origin')==ORIGIN,'mapping receipt differs')
 need(m.get('pointer_url')=='https://console.saisi.online/installers/release-'+c['channel']+'.json','channel pointer URL differs')
 native_redirect_allowed(ORIGIN,m['pointer_url'])
 return m

class MirrorClient:
 """Thin wrapper over the frozen task-98 writer. It adds no second production writer."""
 def __init__(self,c,evidence):
  import importlib.util
  m=validate_mapping(c);self.c=c;self.mapping=m
  need(pin(ADAPTER_MODULE)['sha256']==ADAPTER_MODULE_SHA,'revision-8 adapter module pin differs')
  spec=importlib.util.spec_from_file_location('root_revision8_static_mirror',ADAPTER_MODULE)
  module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
  need(getattr(module,'ReleaseSourceMirrorAdapter',None) is not None,'revision-8 adapter exposes no ReleaseSourceMirrorAdapter')
  need(getattr(module,'FROZEN_ADAPTER_SHA',None)==FROZEN_ADAPTER_SHA,'revision-8 adapter does not pin the frozen task-98 adapter')
  pointer=Path(str(m['pointer_path']))
  self.backend=module.ReleaseSourceMirrorAdapter(Path(str(m['artifact_root'])),helper_path=TRUSTED_HELPER,helper_sha256=TRUSTED_HELPER_SHA,signer_path=TRUSTED_CLI,signer_sha256=TRUSTED_CLI_SHA,launcher_pubkey=c['launcher_public_key'],release_pubkey=c['release_public_key'],evidence_dir=evidence,pointer_path=pointer,min_version=c['min_version'],pending_parent=Path(str(m['pending_parent'])) if m.get('pending_parent') else None,release_source_root=Path(str(c['release_source_root'])),expected_cli_sha256=c['release_cli']['sha256'],expected_cli_bytes=c['release_cli']['bytes'])
 def stage(self,bundle):
  c=self.c
  receipt=self.backend.stage_verified(bundle,version=c['version'],channel=c['channel'],source_sha=c['source_sha'],inventory_sha256=c['inventory_sha256'],artifact_base=c['artifact_base'],go=True)
  expected={p.name:pin(p) for p in bundle.iterdir()}
  need(receipt.get('artifact_base')==c['artifact_base'] and receipt.get('inventory_sha256')==c['inventory_sha256'] and receipt.get('files')==expected,'mirror receipt differs')
  return receipt
 def promote(self,document_path,expected_previous,publication_receipt):
  c=self.c
  need(publication_receipt.get('state')=='PUBLIC_BUNDLE_VERIFIED' and type(publication_receipt.get('release_id')) is int and publication_receipt.get('source_sha')==c['source_sha'] and publication_receipt.get('inventory_sha256')==c['inventory_sha256'],'publication proof missing or differs')
  return self.backend.promote_pointer(document_path,version=c['version'],channel=c['channel'],source_sha=c['source_sha'],inventory_sha256=c['inventory_sha256'],expected_previous=expected_previous,publication_receipt=publication_receipt,go=True)

class SameHostProof:
 """Direct same-origin 200 proof of the permanent files; never follows a redirect."""
 def __init__(self,transport):self.transport=transport
 def files(self,base,pins):
  out={}
  for name,info in sorted(pins.items()):
   raw=fetch_public(base+'/'+quote(name),info['bytes'],self.transport,native=True)
   need(sha(raw)==info['sha256'],'same-host mirror HTTP bytes differ')
   out[name]={'sha256':info['sha256'],'bytes':info['bytes'],'status':200,'redirects':0}
  return out
 def document(self,url,expected):
  raw=fetch_public(url,len(expected),self.transport,native=True);need(raw==expected,'public channel pointer differs');return {'status':200,'redirects':0,'bytes':len(raw),'sha256':sha(raw)}
 def absence(self,url):
  """The optional sibling revocations request must be a genuine direct 403/404, never a page."""
  status,headers,raw=self.transport('GET',url,{'User-Agent':'SummriseRootFinalizer/1'},None,65536)
  need(status in (403,404) and not headers.get('Location',headers.get('location')),'native optional revocations must be genuine absent 403/404')
  return {'url':url,'status':status,'bytes':len(raw),'sha256':sha(raw),'redirects':0}

class NativeBundleVerifier:
 def __init__(self,c,audit):
  self.c=c;self.audit=audit
  for key,path,wanted in [('helper',TRUSTED_HELPER,TRUSTED_HELPER_SHA),('signer',TRUSTED_CLI,TRUSTED_CLI_SHA)]:
   need(pin(path)['sha256']==wanted,'trusted '+key+' pin differs')
 def _run(self,label,argv):
  need(not (self.audit/(label+'.exit')).exists(),'duplicate verifier evidence')
  with (self.audit/(label+'.stdout.raw')).open('xb') as out,(self.audit/(label+'.stderr.raw')).open('xb') as err:
   try:p=subprocess.run(argv,capture_output=False,stdout=out,stderr=err,timeout=120,env=dict(os.environ,PYTHONDONTWRITEBYTECODE='1'))
   except Exception:raise Refusal('trusted verifier execution failed') from None
   (self.audit/(label+'.exit')).write_text(str(p.returncode)+'\n');out.flush();err.flush();os.fsync(out.fileno());os.fsync(err.fileno())
  need(p.returncode==0,'trusted signature/bundle verification refused')
 def manifest(self,bundle):
  """Read the nine-key manifest contract with the same meaning the pinned helper writes it."""
  c=self.c;raw=(bundle/'inventory.json').read_bytes()
  try:document=json.loads(raw)
  except Exception:raise Refusal('manifest is not JSON') from None
  need(isinstance(document,dict) and set(document)==INVENTORY_KEYS,'manifest key set differs from the revision-8 contract')
  need(document['schema']==1 and document['sourceSha']==c['source_sha'] and document['version']==c['version'] and document['channel']==c['channel'],'manifest identity differs')
  need(document['mirror']==c['artifact_base'] and document['minVersion']==c['min_version'],'manifest mirror/minimum version differs')
  tooling=document['tooling']
  need(isinstance(tooling,dict) and set(tooling)=={'cli','source'},'manifest tooling section differs')
  signing=c['release_cli']
  need(tooling['cli']=={'sha256':signing['sha256'],'bytes':signing['bytes']},'manifest does not name the signing CLI bytes')
  source=tooling['source'];need(isinstance(source,dict) and bool(source),'manifest source pins missing')
  root=Path(str(c['release_source_root']))
  for name in sorted(source):
   path=(root/name) if (root/name).is_file() else (root.parent/name)
   need(path.is_file() and not path.is_symlink(),'manifest source pin has no readable file: '+name)
   need(sha(path.read_bytes())==source[name],'manifest source pin differs from the live bytes: '+name)
  return document
 def verify(self,bundle,label):
  c=self.c;need(set(p.name for p in bundle.iterdir())==names(c),'bundle file set differs')
  need(pin(bundle/'inventory.json')['sha256']==c['inventory_sha256'],'expected signed inventory differs')
  self.manifest(bundle)
  self._run(label+'-bundle',[sys.executable,'-B',str(TRUSTED_HELPER),'verify','--signer',str(TRUSTED_CLI),'--out',str(bundle),'--version',c['version'],'--channel',c['channel'],'--source-sha',c['source_sha'],'--mirror',c['artifact_base'],'--launcher-pubkey',c['launcher_public_key'],'--release-pubkey',c['release_public_key'],'--release-source-root',str(Path(str(c['release_source_root']))),'--expected-cli-sha256',str(c['release_cli']['sha256']),'--expected-cli-bytes',str(c['release_cli']['bytes'])])
  document=bundle/f'release-{c["version"]}-{c["channel"]}.json'
  self._run(label+'-native',[str(TRUSTED_CLI),'verify-document','--document',str(document),'--public-key',c['release_public_key'],'--version',c['version'],'--channel',c['channel']])
  return {p.name:pin(p) for p in bundle.iterdir()}

def run(c,evidence,api,mirror,verifier,execute=False,phase='finalize',transport=http_transport):
 if not execute or c.get('go') is not True:return {'state':'HOLD','executed':False}
 validate_config(c)
 if phase=='finalize':validate_mapping(c)
 need(evidence.is_dir() and evidence.resolve()==evidence and not evidence.is_symlink(),'evidence root')
 root=Path(tempfile.mkdtemp(prefix='release-final-',dir=evidence));root.chmod(0o700)
 def save(name,value):
  with (root/name).open('x') as out:json.dump(value,out,indent=2,sort_keys=True);out.write('\n');out.flush();os.fsync(out.fileno())
 lock=None
 promotion_attempted=False
 try:
  if phase=='finalize':
   lock=PublicationLock(Path(c.get('publication_lock','')))
   need(not lock.path.is_relative_to(Path(c['mapping']['artifact_root'])),'publication lock must be outside public root')
   lock.acquire()
  before=api.snapshot(c);validate_authority(c,before,draft=True);save('authority-before.json',before)
  draft=root/'draft';draft.mkdir(mode=0o700)
  for name,(ident,size,_) in sorted(asset_map(before['release']['assets']).items()):
   raw=api.download_asset(ident,size);need(len(raw)==size,'draft download size');(draft/name).write_bytes(raw)
  verifier=verifier(root) if callable(verifier) else verifier
  draft_pins=verifier.verify(draft,'draft')
  second=api.snapshot(c);validate_authority(c,second,draft=True);save('authority-after-download.json',second)
  need(asset_map(second['release']['assets'])==asset_map(before['release']['assets']),'second authority asset changes')
  if phase=='draft':save('draft-verified.json',{'state':'DRAFT_VERIFIED','published':False,'inventory':pin(draft/'inventory.json')});return {'state':'DRAFT_VERIFIED','published':False,'evidence':str(root)}
  mirror=mirror(root) if callable(mirror) else mirror
  save('mirror-stage.json',mirror.stage(draft))
  proof=SameHostProof(transport)
  save('same-host-files.json',proof.files(c['artifact_base'],draft_pins))
  native=root/'native';native.mkdir(mode=0o700)
  for name,info in sorted(draft_pins.items()):
   raw=fetch_public(c['artifact_base']+'/'+quote(name),info['bytes'],transport,native=True);(native/name).write_bytes(raw)
  need(verifier.verify(native,'native')==draft_pins,'same-host signed bundle differs')
  save('same-host-revocations.json',proof.absence(c['artifact_base']+'/revocations.json'))
  third=api.snapshot(c);validate_authority(c,third,draft=True);save('authority-before-publish.json',third)
  published=api.publish(c);need(published.get('id')==c['release_id'] and published.get('draft') is False,'publish API result differs');save('publish.json',{'id':published['id'],'draft':False})
  public=root/'public';public.mkdir(mode=0o700)
  for name,info in sorted(draft_pins.items()):
   raw=api.public_asset(c['proof_base']+'/'+quote(name),info['bytes']);need(sha(raw)==info['sha256'],'public GitHub bytes differ');(public/name).write_bytes(raw)
  need(verifier.verify(public,'public')==draft_pins,'public signed bundle differs')
  fourth=api.snapshot(c);validate_authority(c,fourth,draft=False);save('authority-public.json',fourth)
  document=public/f'release-{c["version"]}-{c["channel"]}.json'
  receipt={'state':'PUBLIC_BUNDLE_VERIFIED','release_id':c['release_id'],'source_sha':c['source_sha'],'inventory_sha256':c['inventory_sha256']}
  promotion_attempted=True
  save('pointer-promote.json',mirror.promote(document,c.get('pointer_previous'),receipt))
  save('pointer-public.json',proof.document(c['mapping']['pointer_url'],document.read_bytes()))
  save('result.json',{'state':'PUBLISHED_DELIVERED_AND_POINTER_VERIFIED','source_sha':c['source_sha'],'release_id':c['release_id'],'files':draft_pins,'inventory':pin(public/'inventory.json'),'pointer_last':True})
  return {'state':'PUBLISHED_DELIVERED_AND_POINTER_VERIFIED','evidence':str(root),'pointer_last':True}
 except Refusal as error:save('failure.json',{'state':'STOPPED','reason':str(error),'no_old_files_deleted':True,'pointer_unchanged':None if promotion_attempted else True});raise
 except Exception as error:save('failure.json',{'state':'STOPPED','reason':'operation failed: '+type(error).__name__+': '+str(error),'no_old_files_deleted':True,'pointer_unchanged':None if promotion_attempted else True});raise Refusal('finalization operation failed: '+type(error).__name__+': '+str(error)) from None
 finally:
  if lock is not None:lock.release()

def main():
 p=argparse.ArgumentParser(description=__doc__);p.add_argument('--config',type=Path,required=True);p.add_argument('--config-sha256');p.add_argument('--evidence',type=Path,required=True);p.add_argument('--execute',action='store_true');p.add_argument('--phase',choices=('draft','finalize'),default='draft');a=p.parse_args()
 config_bytes=a.config.read_bytes();c=json.loads(config_bytes)
 if not a.execute or c.get('go') is not True:print(json.dumps({'state':'HOLD','executed':False}));return 0
 need(re.fullmatch(r'[0-9a-f]{64}',a.config_sha256 or '') and sha(config_bytes)==a.config_sha256,'explicit Root config pin differs')
 validate_config(c)
 if a.phase=='finalize':validate_mapping(c)
 token=GitCredentialAuth(c['repository'],c['checkout']).token();api=GitHubAPI(c['repository'],token)
 mirror=(lambda root:MirrorClient(c,root)) if a.phase=='finalize' else None
 print(json.dumps(run(c,a.evidence,api,mirror,lambda root:NativeBundleVerifier(c,root),execute=True,phase=a.phase),sort_keys=True));return 0
if __name__=='__main__':
 try:sys.exit(main())
 except Refusal as error:print('release-finalizer: '+str(error),file=sys.stderr);sys.exit(1)
 except Exception:print('release-finalizer: invalid local configuration or operation',file=sys.stderr);sys.exit(1)
