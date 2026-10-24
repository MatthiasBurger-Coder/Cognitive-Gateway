import argparse,hashlib,json,subprocess
from pathlib import Path

parser=argparse.ArgumentParser(description='Audit retained EPIC-06 live evidence without inference or external effects.')
parser.add_argument('--root',type=Path,default=Path.home()/'cg-tests/epic06-178-20261024')
parser.add_argument('--live',type=Path,default=Path.home()/'cg-tests/epic06-178-live-20261024')
parser.add_argument('--output',type=Path)
args=parser.parse_args()
root=args.root.resolve(); live=args.live.resolve()
report=json.loads((live/'live-qualification.json').read_text())
dataset=json.loads((root/'models/datasets/invocation-extraction-v2.json').read_text())
checks=[]
def sha(data): return hashlib.sha256(data).hexdigest()
def check(name,passed): checks.append({'name':name,'passed':bool(passed)})
def same(a,b):
    if type(a) is not type(b): return False
    if isinstance(b,dict): return a.keys()==b.keys() and all(same(a[k],v) for k,v in b.items())
    if isinstance(b,list): return len(a)==len(b) and all(same(x,y) for x,y in zip(a,b))
    return a==b
check('Both adapters have a fresh live pass',report['status']=='LIVE_ROLE_PASS' and report['evidence_kind']=='live' and {a['adapter'] for a in report['adapters']}=={'local','chat-completions'})
check('Every live source remains current',all(sha((root/name).read_bytes())==digest for name,digest in report['source_sha256'].items()))
expected={case['id']:case for case in dataset['cases']}
role=(root/'crates/gateway-daemon/src/file_request_extraction_v1.txt').read_text()
for adapter in report['adapters']:
    kind=adapter['adapter']; decision=adapter['decision']
    check(kind+': real evaluator approval',adapter['evaluator_exit_code']==0 and decision['gates_passed'] is True and decision['eligible_for_promotion'] is True and decision['evidence']=='live' and decision['failed_metrics']=={})
    check(kind+': every frozen case exactly once',len(adapter['samples'])==len(expected) and {s['case'] for s in adapter['samples']}==set(expected))
    check(kind+': six operational probes',len(adapter['probes'])==6 and all(value is True for value in adapter['probes'].values()))
    authority=None
    for sample in adapter['samples']:
        name=sample['case']; frozen=expected[name]; response=sample['response']; folder=live/(kind+'-'+name)
        check(kind+'/'+name+': independent frozen semantic grade',sample['exit_code']==0 and response['status']=='VERIFIED' and response['verification']['validity']=='VALID' and same(sample['expected'],frozen['expected']) and same(response['parsed']['result'],frozen['expected']))
        check(kind+'/'+name+': unchanged actual input', (folder/'source.txt').read_text()==frozen['input'])
        journals=list((folder/'journal').glob('*.json'))
        check(kind+'/'+name+': one durable journal',len(journals)==1)
        saved=json.loads(journals[0].read_text())['records']['sample']
        raw=bytes(saved['raw']); parsed=json.loads(raw); wire_text=saved['rendered']['rendered']['payload']; wire=json.loads(wire_text)
        check(kind+'/'+name+': journal and raw result match frozen truth',same(saved['result'],frozen['expected']) and same(parsed['result'],frozen['expected']) and same(saved['trace'],response['trace']) and parsed['tools']==[] and parsed['references']==[])
        check(kind+'/'+name+': raw and wire digests',sha(raw)==response['trace']['replay']['output']['output_digest'] and sha(wire_text.encode())==response['trace']['accounting']['count']['payload_digest'])
        check(kind+'/'+name+': immutable authority and complete request bridge',[m['role'] for m in wire['messages']]==['system','user'] and role in wire['messages'][0]['content'] and json.loads(wire['messages'][1]['content'])['request']==frozen['input'] and (authority is None or authority==wire['messages'][0]['content']))
        authority=wire['messages'][0]['content']
        profile=report['installed_profile']; receipt=response['adapter']['receipt']
        actual=profile['model_id'] if kind=='local' else profile['runtime_model']
        check(kind+'/'+name+': exact installed model and explicit JSON generation',receipt['actual_model_id']==actual and receipt['model']['digest']==profile['artifact_digest'].removeprefix('sha256:') and receipt['generation']['structured_output_mode']=='JSON' and receipt['generation']['max_output_tokens']==1024)
    process=subprocess.run([str(root/'target/debug/examples/qualify-invocation'),str(live/(kind+'-profile.json')),str(live/(kind+'-measurements.json'))],capture_output=True,text=True,check=False)
    check(kind+': fresh Rust metric decision reproduction',process.returncode==0 and same(json.loads(process.stdout),decision))
result={'status':'PASS' if all(c['passed'] for c in checks) else 'FAIL','reviewer':'Separate evidence role pass by the implementing agent; no independent-person approval claimed. No inference or external effects are dispatched.','checks':checks,'failed_checks':[c['name'] for c in checks if not c['passed']]}
(args.output or root/'target/epic06-remote-20261024/independent-live-review.json').write_text(json.dumps(result,indent=2)+'\n')
print(result['status'],len(checks),result['failed_checks'])
raise SystemExit(0 if result['status']=='PASS' else 1)
