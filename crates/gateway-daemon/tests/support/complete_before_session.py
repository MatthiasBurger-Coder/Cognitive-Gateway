"""#408 actual native CLI + credentialed stdio atomic-state provider + PG.

This controlled provider qualifies one explicit AtomicCompleteExport profile.
It is not evidence about any external production provider or installed client.
"""
import copy, hashlib, json, os, subprocess, sys, tempfile, time, threading, queue
from pathlib import Path
import observed_goal_session as legacy

ROOT, BIN = legacy.ROOT, legacy.BIN
encoded, digest = legacy.encoded, legacy.digest

PEER = r'''import json,sys,pathlib,os,time
root=pathlib.Path(sys.argv[1]); tool=json.loads((root/'wire-tool.json').read_text())
assert os.environ.get('CG_WORLD_AUTH')==(root/'peer-auth.fixture').read_text(),'credentialed provider admission denied'
for line in sys.stdin:
 r=json.loads(line)
 with (root/'wire.jsonl').open('a') as f:f.write(json.dumps(r)+'\n')
 if 'id' not in r:continue
 m=r['method']
 if m=='initialize':v={'protocolVersion':'2025-11-25','serverInfo':{'name':'observed-fixture','version':'1.0.0'},'capabilities':{'tools':{'listChanged':True}}}
 elif m=='tools/list':
  if (root/'pause-discovery').exists():
   (root/'discovery-waiting').touch();until=time.monotonic()+20
   while not (root/'release-discovery').exists():
    assert time.monotonic()<until,'qualification barrier timed out'
    time.sleep(.01)
  v={'tools':[tool]}
 elif m=='tools/call':
  assert r['params']['name']=='records.read'
  selector=r['params']['arguments']['query']; assert selector in ('one','two','project')
  # One immutable version of the ENTIRE native world, read once. Test updates
  # use atomic rename. Per-object requests retain their old goal-read identity;
  # only selector=project implements the qualified complete-export operation.
  world=json.loads((root/'atomic-world.json').read_text())
  result=dict(world['fields']);result.update(object='target-project' if selector=='project' else 'object-'+selector,id=selector,revision=world['epoch'])
  if selector=='project':
   if (root/'pause-world').exists():
    (root/'world-waiting').touch();until=time.monotonic()+20
    while not (root/'release-world').exists():
     assert time.monotonic()<until,'qualification world barrier timed out'
     time.sleep(.01)
   if (root/'response-fault').exists():
    fault=(root/'response-fault').read_text()
    if fault=='membership':result['membership']='0'*64
    elif fault=='epoch':result['epoch']='foreign-epoch'
    elif fault=='version':result['hidden_version']=''
    elif fault=='object':result['object']='object-one'
    elif fault=='identity':result['id']='foreign'
    elif fault=='missing':del result['hidden_bytes']
    elif fault=='extra':result['extra_member']=True
    elif fault=='type':result['hidden_bytes']='wrong-type'
    elif fault=='oversized':result['hidden_version']='x'*262145
    else:raise AssertionError('unknown response fault')
  v={'content':[],'structuredContent':result,'isError':False}
 elif m=='ping':v={}
 else:raise AssertionError('unexpected method '+m)
 print(json.dumps({'jsonrpc':'2.0','id':r['id'],'result':v}),flush=True)
'''

def repin_registry(runtime,docs):
    done={};active=set();pin_keys={'id','scope','contract','contract_version','revision','digest'}
    def update(value):
        if isinstance(value,dict):
            if set(value)=={'pin','body'} and value['pin'].get('contract')=='cg.scoped-snapshot-read-route':
                actual=repin(value['pin']['id']);return {'pin':actual,'body':copy.deepcopy(docs[actual['id']]['body'])}
            if pin_keys==set(value) and value['id'] in docs:return repin(value['id'])
            return {key:update(item) for key,item in value.items()}
        if isinstance(value,list):return [update(item) for item in value]
        return value
    def repin(name):
        if name in done:return copy.deepcopy(done[name])
        assert name not in active,'fixture reference cycle';active.add(name)
        body=update(docs[name]['body']);docs[name]['body']=body
        pin={**docs[name]['pin'],'digest':digest(body)};done[name]=pin;active.remove(name);return copy.deepcopy(pin)
    for name in docs:repin(name)
    runtime=update(runtime)
    runtime['documents']=[{'pin':done[name],'path':str(row['path'])} for name,row in docs.items()]
    return runtime

def configure(root):
    launch, request, intent, root = legacy.configure(root, True)
    admission_path=Path(launch[13]); admission=json.loads(admission_path.read_text());entry=admission['mappings'][0]
    runtime=entry['sessions']['observed_goal_runtime'];scope=entry['canonical_scope']
    docs={d['pin']['id']:{'pin':d['pin'],'path':Path(d['path']),'body':json.loads(Path(d['path']).read_text())} for d in runtime['documents']}
    def add(name,contract,body):
        pin={'id':name,'scope':scope,'contract':contract,'contract_version':'1.0','revision':'1','digest':digest(body)}
        docs[name]={'pin':pin,'path':root/(name+'.json'),'body':body}
        return copy.deepcopy(pin)
    objects=['object-one','object-two','object-hidden']
    subject_sets=[['state.first','state.one.bytes','state.one.version'],['state.second','state.two.bytes','state.two.version'],['state.hidden.bytes','state.hidden.version']]
    target=docs['target-project']['pin']
    docs['target-project']['body']['subjects']=sum(subject_sets,[])
    for name,subjects in zip(objects[:2],subject_sets[:2]):docs[name]['body']['subjects']=subjects
    hidden=add('object-hidden','cg.connector-object',{'schema_version':'1.0','id':'object-hidden','revision':'1','scope':scope,'kind':'ARTIFACT','connector':'example.records','parent_target':target,'subjects':subject_sets[2],'coordinates':{'id':{'kind':'STRING','value':'hidden'}},'object_reference':'object-hidden'})
    docs['observed-read-task']['body']['inputs'].append({'role':'hidden','value':{'kind':'REFERENCE','value':hidden}})
    docs['observed-read-task']['body']['inputs'].sort(key=lambda i:i['role'])
    domain=add('atomic-domain','cg.connector-atomic-version-domain',{'schema_version':'1.0','scope':scope,'scope_target':target,'version_contract':'NON_REUSED_ATOMIC_GLOBAL_EPOCH_V1'})
    def header(name,contract):return {'id':name,'scope':scope,'contract':contract,'contract_version':'1.0','revision':'1'}
    route_body={'definition':header('atomic-route','cg.scoped-snapshot-read-route'),'authority_domain':domain,'target':{'connector':'example.records','scope':scope},'capability':'architecture.dependency-analysis','class':'read','arguments':{'query':{'kind':'STRING','value':'project'}},'object_reference':'target-project','epoch_argument':None}
    route=add('atomic-route','cg.scoped-snapshot-read-route',route_body)
    members=[{'object':docs[name]['pin'],'source':'observed.state','source_kind':'TOOL','object_reference':name,'subjects':[s.split('.') for s in subjects],'version_subject':next(s.split('.') for s in subjects if s.endswith('.version')),'route':{'pin':route,'body':copy.deepcopy(route_body)}} for name,subjects in zip(objects,subject_sets)]
    universe=add('atomic-universe','cg.scoped-snapshot-universe',{'definition':header('atomic-universe','cg.scoped-snapshot-universe'),'objects':members})
    profile=add('atomic-profile','cg.scoped-snapshot-profile',{'definition':header('atomic-profile','cg.scoped-snapshot-profile'),'authority_domain':domain,'kind':'ATOMIC_COMPLETE_EXPORT','anchor_source':'observed.state','anchor_source_kind':'TOOL','anchor_route':{'pin':route,'body':copy.deepcopy(route_body)},'epoch_subject':['scope','epoch'],'membership_subject':['scope','membership'],'max_age_seconds':120})
    fields={'first':'state.first','second':'state.second','one_bytes':'state.one.bytes','one_version':'state.one.version','two_bytes':'state.two.bytes','two_version':'state.two.version','hidden_bytes':'state.hidden.bytes','hidden_version':'state.hidden.version','epoch':'scope.epoch','membership':'scope.membership'}
    norm=add('atomic-normalization','cg.connector-normalization',{'schema_version':'1.0','acquisition':{'source_kind':'TOOL','sensitivity':'PUBLIC','authenticated_profile':'records.local','semantics':{'kind':'DIRECT_CURRENT_READ_PROFILE','max_age_seconds':120,'time_basis':'REQUEST_START_LOWER_BOUND'}},'source':'observed.state','object_field':'object','revision_field':'revision','digest_field':None,'fields':fields})
    provider=add('atomic-provider','cg.connector-atomic-export-provider',{'schema_version':'1.0','scope':scope,'task':runtime['task'],'projection':runtime['projection'],'mapping':runtime['mapping'],'universe':universe,'profile':profile,'route':route,'scope_target':target,'objects':[docs[name]['pin'] for name in objects],'selector_arguments':{'query':'id'},'response':{'object_field':'object','identity_fields':{'id':'id'}},'semantics':'ATOMIC_COMPLETE_EXPORT_V1'})
    # A single closed primitive raw schema serves explicit goal and world routes.
    schema={'type':'object','properties':{key:{'type':('boolean' if key in ('first','second') else 'integer' if key.endswith('_bytes') else 'string')} for key in ['object','id','revision',*fields]},'required':['object','id','revision',*fields],'additionalProperties':False}
    tool=json.loads((root/'wire-tool.json').read_text());tool['outputSchema']=schema;(root/'wire-tool.json').write_bytes(encoded(tool))
    docs['manifest']['body']['inventory'][0]['schema_digest']=digest({'discovery_schema_version':'1.0','kind':'tool','entry':tool})
    docs['mapping']['body']['records'][0]['result_schema']=schema
    runtime['complete_before']={'universe':universe,'profile':profile,'provider':provider,'read':{'mapping_reference':'mapping.read','capability':'architecture.dependency-analysis','source':'observed.state','arguments':{'query':{'kind':'STRING','value':'project'}},'normalization':norm}}
    runtime=repin_registry(runtime,docs)
    for row in docs.values():row['path'].write_bytes(encoded(row['body']))
    entry['sessions']['observed_goal_runtime']=runtime
    entry['sessions']['resource_limits']['amounts']=legacy.sum_amounts(entry['sessions']['resource_limits']['amounts'],legacy.amount())
    admission_path.write_bytes(encoded(admission))
    actual_universe=docs['atomic-universe']['body']
    membership_members=[{key:member[key] for key in ('object','source','source_kind','object_reference','subjects','version_subject')} for member in actual_universe['objects']]
    membership_members.sort(key=lambda m:encoded(m))
    membership=digest({'contract':'cg.scoped-snapshot-membership/1.0','scope':scope,'members':membership_members})
    epoch='world-epoch-42'
    world={'epoch':epoch,'fields':{'first':True,'second':False,'one_bytes':128,'one_version':'object-v3','two_bytes':4096,'two_version':'object-v9','hidden_bytes':777,'hidden_version':'object-v2','epoch':epoch,'membership':membership}}
    write_world(root,world)
    (root/'peer.py').write_text(PEER)
    secret='fixture-only-atomic-export-credential'
    (root/'peer-auth.fixture').write_text(secret);(root/'peer-auth.fixture').chmod(0o600)
    credentials=json.loads((root/'credentials.json').read_text());credentials['profiles']['records.local']['environment']={'CG_WORLD_AUTH':secret};(root/'credentials.json').write_bytes(encoded(credentials));(root/'credentials.json').chmod(0o600)
    (root/'fixture-launch.json').write_bytes(encoded(launch))
    return launch,request,intent,root

def write_world(root,world):
    temporary=root/'atomic-world.pending';temporary.write_bytes(encoded(world));temporary.replace(root/'atomic-world.json')

def mutate_case(root,case):
    launch=json.loads((root/'fixture-launch.json').read_text())
    admission_path=Path(launch[13]);admission=json.loads(admission_path.read_text());runtime=admission['mappings'][0]['sessions']['observed_goal_runtime']
    docs={d['pin']['id']:{'pin':d['pin'],'path':Path(d['path']),'body':json.loads(Path(d['path']).read_text())} for d in runtime['documents']}
    universe=docs['atomic-universe']['body'];profile=docs['atomic-profile']['body'];provider=docs['atomic-provider']['body'];normalization=docs['atomic-normalization']['body'];route=docs['atomic-route']['body']
    if case=='age':
        profile['max_age_seconds']=2;normalization['acquisition']['semantics']['max_age_seconds']=2
    elif case=='missing-member':universe['objects'].pop()
    elif case=='duplicate-subject':universe['objects'][0]['subjects'].append(universe['objects'][0]['subjects'][0])
    elif case=='missing-version':universe['objects'][0]['version_subject']=['state','absent','version']
    elif case=='overlap-subject':universe['objects'][1]['subjects'].append(universe['objects'][0]['subjects'][0])
    elif case=='foreign-source':universe['objects'][1]['source']='foreign-source'
    elif case=='caller-source':universe['objects'][1]['source_kind']='CALLER'
    elif case=='non-atomic':profile['kind']='SNAPSHOT_SCOPED_READS'
    elif case=='anchor-in-world':profile['epoch_subject']=['state','first']
    elif case=='invalid-age':profile['max_age_seconds']=0
    elif case=='foreign-domain':profile['authority_domain']=provider['scope_target']
    elif case=='bad-header':route['definition']['revision']='foreign-revision'
    elif case=='unknown-route':route['unknown']=True
    elif case=='mutation-route':route['class']='mutate'
    elif case=='epoch-argument':route['epoch_argument']='epoch'
    elif case=='foreign-task':provider['task']=provider['scope_target']
    elif case=='foreign-target':provider['scope_target']=provider['objects'][0]
    elif case=='duplicate-member':provider['objects'].append(provider['objects'][0])
    elif case=='unassociated-member':docs['observed-read-task']['body']['inputs']=[i for i in docs['observed-read-task']['body']['inputs'] if i['role']!='hidden']
    elif case=='unknown-semantics':provider['semantics']='ATOMIC_TRUE'
    elif case=='missing-selector':provider['selector_arguments']={}
    elif case=='missing-identity':provider['response']['identity_fields']={}
    elif case=='missing-field':normalization['fields'].pop('hidden_bytes')
    elif case=='extra-field':normalization['fields']['phantom']='state.phantom'
    elif case=='weak-source':normalization['acquisition']['source_kind']='SYNTHETIC'
    elif case=='wrong-credential-profile':normalization['acquisition']['authenticated_profile']='foreign-profile'
    elif case=='wrong-age':normalization['acquisition']['semantics']['max_age_seconds']=121
    else:raise AssertionError('unknown controlled mutation '+case)
    runtime=repin_registry(runtime,docs)
    for row in docs.values():row['path'].write_bytes(encoded(row['body']))
    admission['mappings'][0]['sessions']['observed_goal_runtime']=runtime;admission_path.write_bytes(encoded(admission))

def journal(scope):
    assert scope.replace('-','').isalnum()
    return json.loads(legacy.sql("SELECT payload FROM cg_cognitive_journals WHERE kind='task-sessions-v2' AND scope='"+scope+"'"))

def run(case):
    with tempfile.TemporaryDirectory(prefix='cg408-installed-') as temporary:
        root=Path(temporary);launch,base,intent,root=configure(root)
        admission=json.loads(Path(launch[13]).read_text());scope=admission['mappings'][0]['canonical_scope']
        def request(op,inputs):return {'schema_version':'2.0','scope':base['scope'],'operation':'session.'+op,'correlation':{'request_id':'complete-before-qualification'},'execution':{'operating_mode':'DEVELOPMENT','execution_profile':'FULL_PATH'},'input':inputs}
        environment=dict(legacy.canonical.CHILD_ENV)
        if 'LLVM_PROFILE_FILE' in os.environ:environment['LLVM_PROFILE_FILE']=os.environ['LLVM_PROFILE_FILE']
        if case=='age':mutate_case(root,'age')
        negative=case in ('membership','epoch','version','object','identity','missing','extra','type','oversized','budget','commit-failure','revoke','cancel','config-drift','quote-drift','deadline','schema-drift','credential')
        response_faults=('membership','epoch','version','object','identity','missing','extra','type','oversized')
        if case in response_faults:(root/'response-fault').write_text(case)
        if case=='credential':
            credentials=json.loads((root/'credentials.json').read_text());credentials['profiles']['records.local']['environment']['CG_WORLD_AUTH']='incorrect'
            (root/'credentials.json').write_bytes(encoded(credentials))
        if case=='budget':
            entry=admission['mappings'][0]['sessions'];entry['resource_limits']['amounts']['connector_calls']=3
            Path(launch[13]).write_bytes(encoded(admission))
        if case=='schema-drift':
            tool=json.loads((root/'wire-tool.json').read_text());tool['outputSchema']['description']='unregistered schema revision';(root/'wire-tool.json').write_bytes(encoded(tool))
        if case=='satisfied':
            world=json.loads((root/'atomic-world.json').read_text());world['fields']['second']=True;write_world(root,world)
        race=case in ('revoke','cancel','config-drift','quote-drift','deadline')
        if race:(root/'pause-world').touch()
        client=legacy.Client(launch,environment=environment)
        trigger=None
        try:
            assert 'result' in client.initialize()
            started=legacy.call(client,request('start',{'command_id':'world-start','intent':{'kind':'document','contract':'cg.intent','contract_version':'1.0','document':intent}}))
            initial=started['result']['session'];assert initial['state']=='runnable'
            continued=request('continue',{'session_id':initial['session'],'command_id':'world-capture','expected_revision':initial['revision']})
            if case=='commit-failure':
                trigger='q408_'+scope.replace('-','')
                legacy.sql("CREATE FUNCTION "+trigger+"() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.scope='"+scope+"' AND NEW.kind='task-sessions-v2' AND EXISTS (SELECT FROM jsonb_array_elements(NEW.payload::jsonb->'records') r WHERE r->>'kind'='complete_before') THEN RAISE EXCEPTION 'qualification before append rejected'; END IF; RETURN NEW; END $$; CREATE TRIGGER "+trigger+" BEFORE UPDATE ON cg_cognitive_journals FOR EACH ROW EXECUTE FUNCTION "+trigger+"();")
            if race:
                responses=queue.Queue()
                def continuing():
                    try:responses.put(client.exchange('tools/call',{'name':'cg_session_continue_v2','arguments':continued}))
                    except BaseException as error:responses.put(error)
                worker=threading.Thread(target=continuing);worker.start();until=time.monotonic()+25
                while not (root/'world-waiting').exists():
                    assert time.monotonic()<until,'actual export barrier not reached'
                    time.sleep(.01)
                if case=='cancel':
                    second=legacy.Client(launch,environment=environment)
                    try:
                        second.initialize();live=legacy.call(second,request('inspect',{'session_id':initial['session']}))['result']['session']
                        cancelled=legacy.call(second,request('cancel',{'session_id':initial['session'],'command_id':'cancel-world','expected_revision':live['revision']}))
                        assert cancelled['result']['session']['state']=='cancelled'
                    finally:assert second.close()==0
                elif case=='deadline':time.sleep(10.5)
                else:
                    changed=json.loads(Path(launch[13]).read_text())
                    if case=='revoke':changed['mappings'][0]['sessions']['enabled']=False
                    elif case=='config-drift':changed['mappings'][0]['sessions']['observed_goal_runtime']['revision']='changed'
                    else:
                        path=root/'quotes.json';doc=json.loads(path.read_text());doc['discovery']['connector_calls']+=1;path.write_bytes(encoded(doc))
                    Path(launch[13]).write_bytes(encoded(changed))
                (root/'release-world').touch();worker.join(timeout=45);assert not worker.is_alive()
                reply=responses.get_nowait();assert isinstance(reply,dict),type(reply)
                assert 'error' in reply or reply['result']['structuredContent']['status']=='error',reply
                done=None
            else:done=legacy.call(client,continued,'error' if negative else 'ok')
            if negative:
                stored=journal(scope)
                assert not any(r['kind']=='complete_before' for r in stored['records']),stored
                assert not stored['sessions'][0]['checkpoint'].get('prepared_before')
                requests=[json.loads(l) for l in (root/'wire.jsonl').read_text().splitlines()] if (root/'wire.jsonl').exists() else []
                calls=[r for r in requests if r['method']=='tools/call']
                if case in ('budget','schema-drift','credential'):assert not calls,calls
                assert len([r for r in calls if r['params']['arguments']['query']=='project'])<=1
                assert not stored['grants']
                print('INSTALLED_COMPLETE_BEFORE_QUALIFIED',case,flush=True)
                return
            if case=='satisfied':
                assert done['result']['session']['state']=='completed',done
                assert not any(r['kind']=='complete_before' for r in journal(scope)['records'])
                assert all(json.loads(l).get('params',{}).get('arguments',{}).get('query')!='project' for l in (root/'wire.jsonl').read_text().splitlines())
                print('INSTALLED_COMPLETE_BEFORE_QUALIFIED',case,flush=True)
                return
            prepared=done['result']['session'];assert prepared['state']=='runnable' and prepared['dispatch_knowledge']=='prepared',done
            assert prepared.get('final_evidence') is None
            stored=journal(scope);cp=stored['sessions'][0]['checkpoint'];before=cp['prepared_before']
            assert cp['pre_execution_goal'] is None if 'pre_execution_goal' in cp else True
            assert cp['artifact'] is None and cp['pending'] is None and cp['accepted_consent'] is None
            assert not stored['grants']
            retained=[r for r in stored['records'] if r['kind']=='complete_before'];assert len(retained)==1
            record=retained[0];data=bytes(record['bytes']);envelope=json.loads(data)
            assert record['reference']==before['record'] and record['reference']['digest']=='sha256:'+hashlib.sha256(data).hexdigest()
            assert envelope['binding']==before['binding'] and envelope['snapshot_digest']==before['snapshot_digest']
            world=envelope['snapshot']['data'];members=world['universe']['objects']
            assert {m['object']['id'] for m in members}=={'object-one','object-two','object-hidden'}
            assert len({m['source'] for m in members})==1
            assert {s for m in members for s in m['subjects']}=={'state.first','state.second','state.one.bytes','state.one.version','state.two.bytes','state.two.version','state.hidden.bytes','state.hidden.version'}
            assert len(world['acquisitions'])==1,'Atomic export must retain ONE true root acquisition, not per-object inventions'
            assert world['acquisitions'][0]['observation']['metadata']['object_reference']=='target-project'
            requests=[json.loads(line) for line in (root/'wire.jsonl').read_text().splitlines()]
            effects=[r for r in requests if r['method']=='tools/call']
            assert len([r for r in effects if r['params']['arguments']['query']=='project'])==1
            assert all(r['params']['name']=='records.read' for r in effects)
            for value in (before,world,cp):
                assert not {'invocation','invocation_ir','after'}&value.keys()
            # No model/worker budget and no mutation request can hide in this path.
            def no_model(value):
                if isinstance(value,dict):
                    for key,item in value.items():
                        if key in ('model_attempts','input_tokens','output_tokens','output_bytes','worker_attempts','worker_compute_units','repair_attempts'):assert item==0,(key,item)
                        else:no_model(item)
                elif isinstance(value,list):
                    for item in value:no_model(item)
            no_model(cp['resource_ledger'])
            snapshot_before=copy.deepcopy(stored)
            if case=='age':time.sleep(3)
            # The existing private operator discloses exactly saved DATA and
            # cannot issue authority, move revision or reacquire after restart.
            operator=root/'inspect-before.json';operator.write_bytes(encoded({'session_id':prepared['session'],'issuer':admission['mappings'][0]['principal'],'decision':'inspect-before'}))
            inspection=subprocess.run([str(BIN/'cg-local'),'--operation','session.authority','--request',str(operator),*launch],env=environment,capture_output=True,text=True,timeout=45)
            assert inspection.returncode==0,(inspection.stdout,inspection.stderr)
            inspected=json.loads(inspection.stdout)['result'];assert inspected['historical'] is True
            assert inspected['before']==before and inspected['snapshot']==envelope['snapshot']
            assert journal(scope)==snapshot_before
            # Replay of the exact capture command and unsupported marker Continue
            # cannot reacquire or consume a second command/revision/reservation.
            duplicate=legacy.call(client,continued);assert duplicate['result']['session']==prepared
            rejected=legacy.call(client,request('continue',{'session_id':prepared['session'],'command_id':'marker-only','expected_revision':prepared['revision']}),'error')
            assert rejected['diagnostics']==[{'code':'CG_UNSUPPORTED_CAPABILITY'}],rejected
            assert journal(scope)==snapshot_before
            wire_before=(root/'wire.jsonl').read_bytes()
            assert client.close()==0
            client=legacy.Client(launch,environment=environment);client.initialize()
            reopened=legacy.call(client,request('inspect',{'session_id':prepared['session']}))['result']['session']
            assert reopened==prepared
            operator.write_bytes(encoded({'session_id':prepared['session'],'issuer':admission['mappings'][0]['principal'],'decision':'recover'}))
            recovery=subprocess.run([str(BIN/'cg-local'),'--operation','session.authority','--request',str(operator),*launch],env=environment,capture_output=True,text=True,timeout=45)
            assert recovery.returncode==0,(recovery.stdout,recovery.stderr)
            assert journal(scope)==snapshot_before
            assert (root/'wire.jsonl').read_bytes()==wire_before
            operator.write_bytes(encoded({'session_id':prepared['session'],'issuer':'foreign-owner','decision':'inspect-before'}))
            foreign=subprocess.run([str(BIN/'cg-local'),'--operation','session.authority','--request',str(operator),*launch],env=environment,capture_output=True,text=True,timeout=45)
            assert foreign.returncode!=0 and 'snapshot' not in json.loads(foreign.stdout).get('result',{})
            assert journal(scope)==snapshot_before
        finally:
            (root/'release-world').touch()
            if trigger:legacy.sql('DROP TRIGGER IF EXISTS '+trigger+' ON cg_cognitive_journals; DROP FUNCTION IF EXISTS '+trigger+'();')
            assert client.close()==0
            if sys.exc_info()[0]:print(''.join(client.errors),file=sys.stderr)
        print('INSTALLED_COMPLETE_BEFORE_QUALIFIED',flush=True)

if __name__=='__main__':run(sys.argv[1])
