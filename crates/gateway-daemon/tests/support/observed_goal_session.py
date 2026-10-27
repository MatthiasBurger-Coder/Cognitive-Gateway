"""Actual cg CLI native records and installed cg-mcp session, no ProbeHost/model."""
import copy, hashlib, json, os, queue, shutil, subprocess, sys, tempfile, threading, time, uuid
from pathlib import Path
ROOT = Path(__file__).resolve().parents[4]
sys.path.insert(0, str(ROOT / 'tests/codex-local'))
import test_qualification as existing
import test_canonical_host as canonical
BIN = existing.BIN
FIXTURE = canonical.FIXTURE

def encoded(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':'), ensure_ascii=False).encode()
def digest(value):
    return hashlib.sha256(encoded(value)).hexdigest()
def amount(calls=1):
    return dict(model_attempts=0,input_tokens=0,output_tokens=0,output_bytes=0,cost_microunits=0,retrieval_rounds=1,retrieval_results=10000,retrieval_tokens=50000000,repair_attempts=0,worker_attempts=0,worker_compute_units=0,connector_calls=calls)
def sum_amounts(a,b): return {key:a[key]+b[key] for key in a}

PEER = r'''import json,sys,pathlib,time
root=pathlib.Path(sys.argv[1]); tool=json.loads((root/'wire-tool.json').read_text())
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
  v={'tools':[json.loads((root/'wire-tool.json').read_text())]}
 elif m=='tools/call':
  assert r['params']['name']=='records.read'
  ident=r['params']['arguments']['query']; assert ident in ('one','two')
  state=json.loads((root/'state.json').read_text())
  v={'content':[],'structuredContent':{'object':'object-'+ident,'id':ident,'first':state['first'],'second':state['second'],'revision':'revision-'+ident},'isError':False}
 elif m=='ping':v={}
 else:raise AssertionError('unexpected method '+m)
 print(json.dumps({'jsonrpc':'2.0','id':r['id'],'result':v}),flush=True)
'''
class Client(existing.Client):
    def exchange(self,method,params=None,notify=False,raw=None):
        self.next_id += 1
        frame={'jsonrpc':'2.0','method':method}
        if not notify:frame['id']=self.next_id
        if params is not None:frame['params']=params
        self.process.stdin.write((raw if raw is not None else json.dumps(frame))+'\n');self.process.stdin.flush()
        if notify:return None
        line=self.replies.get(timeout=45)
        if line is None:raise AssertionError('installed adapter closed before response')
        reply=json.loads(line)
        assert reply.get('id')==frame['id'],reply
        return reply

def configure(root, multiple, policy_case=None):
    repository=root/'project';repository.mkdir()
    setup=root/'setup'
    subprocess.run(['python3',str(ROOT/'scripts/bootstrap-codex-local.py'),'--repository',str(repository),'--output',str(setup),'--bin-dir',str(BIN),'--client-name','codex','--client-version','1.0'],check=True,capture_output=True,timeout=20)
    launch=json.loads((setup/'launch.json').read_text()); request=json.loads((setup/'request.json').read_text())
    admission_path=Path(launch[13]); admission=json.loads(admission_path.read_text()); entry=admission['mappings'][0]
    scope='observed-'+uuid.uuid4().hex;entry['canonical_scope']=scope
    catalog=repository/'catalog';shutil.copytree(FIXTURE/'catalog',catalog)
    def native(command,**options):
        args=[str(BIN/'cg'),command]
        for key,value in options.items():
            if not isinstance(value,Path):
                path=root/('native-'+command+'-'+key+'.json');path.write_text(json.dumps(value));value=path
            args += ['--'+key,str(value)]
        result=subprocess.run(args+['--json'],env=canonical.CHILD_ENV,capture_output=True,text=True,timeout=30)
        assert result.returncode==0,(command,result.stdout,result.stderr)
        return json.loads(result.stdout)
    intent=json.loads((FIXTURE/'intent.json').read_text())
    conditions=[{'id':'first','subject':'state.first','operator':'EQUALS','expected':{'kind':'BOOLEAN','value':True}},{'id':'second','subject':'state.second','operator':'EQUALS','expected':{'kind':'BOOLEAN','value':True}}]
    intent['id']='observed-intent';intent['desired_state'].update(id='observed-full-goal',conditions=conditions,expression={'kind':'ALL','value':[{'kind':'CONDITION','value':c['id']} for c in conditions]})
    context=json.loads((FIXTURE/'context.json').read_text());context['scope']=scope;context['unknown_subjects']=[c['subject'] for c in conditions]
    rules=json.loads((FIXTURE/'rules.json').read_text());process=json.loads((FIXTURE/'process.json').read_text())
    assessment=native('assess',context=context)
    plan=native('plan',context=assessment,intent=intent,catalog=catalog,rules=rules)
    resolved=native('resolve',plan=plan,catalog=catalog,rules=rules,process=process)['resolution'];basis=resolved['basis']
    steps=[step['id'] for step in plan['plan']['steps']];assert len(steps)>=2,plan
    policy=json.loads((FIXTURE/'policy.json').read_text());facts=copy.deepcopy(policy['steps']['step-condition.0']);policy['basis']=basis;policy['steps']={step:copy.deepcopy(facts) for step in steps}
    projection=json.loads((FIXTURE/'projection.json').read_text());projection.update(basis=basis,step=steps[0],id='observed-read-context');projection['task']['id']='observed-read-task'
    compiled=native('compile',plan=plan,catalog=catalog,rules=rules,process=process,policy=policy,projection=projection)
    assert compiled['execution_context']['id']=='observed-read-context',compiled
    if policy_case=='policy-denied':policy['steps'][steps[0]]['authorizations']['architecture.dependency-analysis']='DENIED'
    if policy_case=='missing-evidence':policy['steps'][steps[0]]['evidence']=[]
    if policy_case=='forged-consent':policy['steps'][steps[0]]['consents']={'architecture.dependency-analysis':'GRANTED'}
    # The canonical inbound resources retain the existing frozen v1 contracts.
    references={}
    for name,contract,document in [('plan','cg.plan',plan),('rules','cg.composition-rules',rules),('process','cg.process-snapshot',process),('projection','cg.context-projection',projection)]:
        ref={'id':name,'contract':contract,'contract_version':'1.0','revision':'1','digest':'sha256:'+digest(document)};references[name]=ref
        entry['resources'].append({'schema_version':'1.0','scope':entry['scope'],'reference':ref,'document':document,'provenance':[{'reference':ref,'source_id':name,'source_revision':'1','freshness':'current','sensitivity':'NORMAL','lineage':[]}]})
    entry['canonical']={'catalog':str(catalog),'plan':references['plan'],'rules':references['rules'],'process':references['process'],'policy':policy}
    documents=[]
    def doc(name,contract,value):
        pin={'id':name,'scope':scope,'contract':contract,'contract_version':'1.0','revision':'1','digest':digest(value)}
        path=root/(name+'.json');path.write_bytes(encoded(value));documents.append({'pin':pin,'path':str(path)})
        return pin
    goal_pin=doc('goal','cg.connector-goal',intent); policy_pin=doc('policy','cg.connector-canonical-policy',policy);projection_pin=doc('read-projection','cg.context-projection',projection)
    def object_doc(name,kind,subjects,coordinate,parent=None):
        return doc(name,'cg.connector-object',{'schema_version':'1.0','id':name,'revision':'1','scope':scope,'kind':kind,'connector':'example.records','parent_target':parent,'subjects':subjects,'coordinates':{'id':{'kind':'STRING','value':coordinate}},'object_reference':name})
    paths=['state.first','state.second']
    if multiple:
        target=object_doc('target-project','PROJECT',paths,'project')
        objects=[object_doc('object-one','ARTIFACT',paths[:1],'one',target),object_doc('object-two','ARTIFACT',paths[1:],'two',target)]
    else:
        target=object_doc('object-one','ARTIFACT',paths,'one');objects=[target]
    output=doc('read-output','cg.output-schema',{'type':'object','properties':{},'required':[],'additionalProperties':False})
    task=json.loads((ROOT/'tests/fixtures/semantic-task-v1/minimal.json').read_text());task['id']='observed-read-task';task['target']={'kind':'PROJECT' if multiple else 'ARTIFACT','reference':target};task['desired_state']=intent['desired_state'];task['output_contract']['schema']=output
    task['inputs']=[{'role':name,'value':{'kind':'REFERENCE','value':obj}} for name,obj in zip(('one','two'),objects)] if multiple else []
    task_pin=doc('observed-read-task','cg.semantic-task',task)
    result_schema={'type':'object','properties':{'object':{'type':'string'},'id':{'type':'string'},'first':{'type':'boolean'},'second':{'type':'boolean'},'revision':{'type':'string'}},'required':['object','id','first','second','revision'],'additionalProperties':False}
    wire_tool={'name':'records.read','inputSchema':{'type':'object','properties':{'query':{'type':'string'}},'required':['query'],'additionalProperties':False},'outputSchema':result_schema}
    (root/'wire-tool.json').write_text(json.dumps(wire_tool));(root/'state.json').write_text(json.dumps({'first':True,'second':True}));(root/'peer.py').write_text(PEER)
    manifest=json.loads((ROOT/'tests/fixtures/connector-mapping-v1/manifest.json').read_text())['manifest']
    manifest['inventory']=[{'kind':'tool','name':'records.read','capability_mapping_ref':'mapping.read','source_mapping_ref':'mapping.read','schema_digest':digest({'discovery_schema_version':'1.0','kind':'tool','entry':wire_tool})}]
    manifest_pin=doc('manifest','cg.connector-manifest',manifest)
    mapping={'mapping_version':'1.0','records':[{'capability':'architecture.dependency-analysis','class':'READ','consent':'policy','idempotency':'idempotent','idempotency_key':None,'kind':'tool','name':'records.read','reference':'mapping.read','result_schema':result_schema,'retry_safety':'read_only','side_effects':'none','source':'observed.state'}]}
    if policy_case in ('mapping-consent','forged-consent'):mapping['records'][0]['consent']='required'
    if policy_case in ('mutate-as-read','admin-as-read'):
        mapping['records'][0].update({'class':'MUTATE' if policy_case=='mutate-as-read' else 'ADMIN','side_effects':'mutation' if policy_case=='mutate-as-read' else 'administrative','retry_safety':'never'})
    mapping_pin=doc('mapping','cg.connector-mapping',mapping)
    reads=[];operations=[]
    for i,obj in enumerate(objects):
        coordinate='one' if i==0 else 'two'
        fields={('first' if i==0 else 'second'):paths[i]} if multiple else {'first':paths[0],'second':paths[1]}
        normalization={'schema_version':'1.0','acquisition':{'source_kind':'TOOL','sensitivity':'PUBLIC','authenticated_profile':'records.local','semantics':{'kind':'DIRECT_CURRENT_READ_PROFILE','max_age_seconds':120,'time_basis':'REQUEST_START_LOWER_BOUND'}},'source':'observed.state','object_field':'object','revision_field':'revision','digest_field':None,'fields':fields}
        normalization_pin=doc('normalization-'+str(i),'cg.connector-normalization',normalization)
        read={'mapping_reference':'mapping.read','capability':'architecture.dependency-analysis','source':'observed.state','arguments':{'query':{'kind':'STRING','value':coordinate}},'normalization':normalization_pin}
        reads += [{'condition':condition['id'],'read':copy.deepcopy(read)} for condition in (conditions[i:i+1] if multiple else conditions)]
        operations.append({'mapping_reference':'mapping.read','purpose':'FIXED_READ','profile':'EXACT_TASK_OPERANDS_V1','subject':{'kind':'INPUT_REFERENCE','role':coordinate} if multiple else {'kind':'TASK_TARGET'},'object':obj,'selector_arguments':{'query':'id'},'literal_arguments':{},'fixed_arguments':{},'response':{'object_field':'object','identity_fields':{'id':'id'}}})
    association_pin=doc('association','cg.connector-read-association',{'schema_version':'1.0','scope':scope,'task':task_pin,'projection':projection_pin,'mapping':mapping_pin,'step':steps[0],'connector':'example.records','operations':operations})
    quotes={'schema_version':'1.0','tariff':{'kind':'FIXED_LOCAL_NO_EMBEDDING','discovery_cost_microunits':0,'operation_cost_microunits':0},'manifest':manifest_pin,'mapping':mapping_pin,'cost_unit':'credits','discovery':amount(),'operations':[{'mapping_reference':'mapping.read','amounts':amount()}]}
    quotes_pin=doc('quotes','cg.connector-quotes',quotes)
    verifier_pin=doc('verifier','cg.connector-verifier',{'schema_version':'1.0','mode':'RETAINED_OBSERVATIONS','scope':scope,'goal':goal_pin,'max_iterations':2,'max_retries':0})
    (root/'credentials.json').write_text(json.dumps({'schema_version':1,'profiles':{'records.local':{'connector':'example.records','server':'example.records.local','tools':['records.read'],'generation':1,'environment':{}}}}))
    runtime={'schema_version':'1.0','id':'observed-runtime','revision':'1','scope':scope,'plan':{k:references['plan'][k] for k in ('id','revision','digest')},'goal':goal_pin,'execution':{'mode':'DEVELOPMENT','profile':'FULL_PATH'},'connector':'example.records','policy':policy_pin,'manifest':manifest_pin,'mapping':mapping_pin,'association':association_pin,'task':task_pin,'projection':projection_pin,'step':steps[0],'quotes':quotes_pin,'verifier':verifier_pin,'stdio':{'executable':'/usr/bin/python3','arguments':['-I',str(root/'peer.py'),str(root)],'directory':str(root),'environment':{}},'compatibility':{'gateway':{'major':0,'minor':1,'patch':0},'transports':['stdio']},'limits':{'frame_bytes':262144,'inventory_bytes':1048576,'items':64,'pages':4,'events_per_request':64,'call_timeout_ms':10000,'shutdown_ms':100},'credential_store':str(root/'credentials.json'),'access_profile':'records.local','documents':documents,'goal_reads':reads}
    total=amount()
    for _ in objects:total=sum_amounts(total,amount())
    store=root/'store';store.write_text(os.environ['CG_COGNITIVE_TEST_DATABASE']);store.chmod(0o600)
    entry['sessions']={'store_file':str(store),'intent':intent,'connector_goal':{'plan':runtime['plan'],'steps':[steps[0]],'observations':[{'condition':c['id'],'source':'observed.state','capability':'architecture.dependency-analysis','operation_class':'read'} for c in conditions]},'resource_limits':{'cost_unit':'credits','amounts':total},'observed_goal_runtime':runtime,'execution':runtime['execution'],'max_actions':2,'max_retries':0,'ttl_ms':300000,'consent_required':False,'enabled':True,'issuers':[entry['principal']]}
    if policy_case=='global-consent':entry['sessions']['consent_required']=True
    admission_path.write_text(json.dumps(admission))
    print('NATIVE_PLAN_POLICY_CONTEXT_COMPILED',flush=True)
    return launch,request,intent,root

def call(client,request,expected='ok'):
    reply=client.exchange('tools/call',{'name':'cg_'+request['operation'].replace('.','_')+'_v2','arguments':request})
    assert 'result' in reply,reply
    result=reply['result']['structuredContent'];assert result['status']==expected,result
    return result

def sql(statement):
    container=os.environ['CG_COGNITIVE_TEST_CONTAINER']
    result=subprocess.run(['docker','exec','-i',container,'psql','-U','cognitive_gateway','-d','cognitive_gateway','-X','-qAt','-v','ON_ERROR_STOP=1'],input=statement,text=True,capture_output=True,timeout=15)
    assert result.returncode==0,'controlled qualification SQL failed'
    return result.stdout.strip()

def run(case):
    with tempfile.TemporaryDirectory(prefix='cg403-installed-') as temporary:
        root=Path(temporary);root.mkdir(parents=True,exist_ok=True);launch,base,intent,root=configure(root,case=='two')
        revoked=case in ('authority-revoked','cancelled')
        if revoked:(root/'pause-discovery').touch()
        if case=='false':(root/'state.json').write_text(json.dumps({'first':True,'second':False}))
        def request(op,inputs):return {'schema_version':'2.0','scope':base['scope'],'operation':'session.'+op,'correlation':{'request_id':'observed-qualification'},'execution':{'operating_mode':'DEVELOPMENT','execution_profile':'FULL_PATH'},'input':inputs}
        environment=dict(canonical.CHILD_ENV)
        client=Client(launch,environment=environment)
        barrier_created=False
        try:
            assert 'result' in client.initialize()
            started=call(client,request('start',{'command_id':'start','intent':{'kind':'document','contract':'cg.intent','contract_version':'1.0','document':intent}}));assert started['result']['session']['state']=='runnable',started
            snapshot=started['result']['session']
            replay=case in ('recovery','replay-drift')
            if replay:
                # A real PG transaction failure AFTER marker commit; no checkpoint rewrite.
                scope=json.loads(Path(launch[13]).read_text())['mappings'][0]['canonical_scope']
                assert scope.replace('-','').isalnum()
                function='qf_'+scope.replace('-','');trigger='qt_'+scope.replace('-','')
                sql(("""CREATE FUNCTION qualification_block_completed() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN
                  IF NEW.scope = '"""+scope+"""' AND NEW.kind='task-sessions-v2' AND EXISTS
                  (SELECT FROM jsonb_array_elements(NEW.payload::jsonb->'sessions') s WHERE s->'checkpoint'->'snapshot'->>'state'='completed')
                  THEN RAISE EXCEPTION 'qualification completion barrier'; END IF; RETURN NEW; END $$;
                  CREATE TRIGGER qualification_completion_barrier BEFORE UPDATE ON cg_cognitive_journals
                  FOR EACH ROW EXECUTE FUNCTION qualification_block_completed();""").replace('qualification_block_completed',function).replace('qualification_completion_barrier',trigger))
                barrier_created=True
            continue_request=request('continue',{'session_id':snapshot['session'],'command_id':'observe','expected_revision':snapshot['revision']})
            if revoked:
                responses=queue.Queue()
                def continuing():
                    try:responses.put(client.exchange('tools/call',{'name':'cg_session_continue_v2','arguments':continue_request}))
                    except BaseException as error:responses.put(error)
                worker=threading.Thread(target=continuing);worker.start()
                until=time.monotonic()+20
                while not (root/'discovery-waiting').exists():
                    assert time.monotonic()<until,'actual discovery barrier not reached'
                    time.sleep(.01)
                admission=json.loads(Path(launch[13]).read_text())
                if case=='authority-revoked':
                    admission['mappings'][0]['sessions']['enabled']=False
                    Path(launch[13]).write_text(json.dumps(admission))
                else:
                    second=Client(launch,environment=environment)
                    try:
                        assert 'result' in second.initialize()
                        live=call(second,request('inspect',{'session_id':snapshot['session']}))['result']['session']
                        assert live['state']=='dispatching',live
                        cancelled=call(second,request('cancel',{'session_id':snapshot['session'],'command_id':'cancel-live','expected_revision':live['revision']}))
                        assert cancelled['result']['session']['state']=='cancelled',cancelled
                    finally:assert second.close()==0
                (root/'release-discovery').touch()
                worker.join(timeout=45);assert not worker.is_alive()
                response=responses.get_nowait();assert isinstance(response,dict),type(response)
                done=response['result']['structuredContent'];assert done['status']=='error',done
                if case=='authority-revoked':
                    admission['mappings'][0]['sessions']['enabled']=True
                    Path(launch[13]).write_text(json.dumps(admission))
                live=call(client,request('inspect',{'session_id':snapshot['session']}))['result']['session']
                assert live['state']!='completed',live
                if case=='cancelled':assert live['state']=='cancelled',live
            else:done=call(client,continue_request,'error' if replay or case=='false' else 'ok')
            if replay:
                assert done['diagnostics']==[{'code':'CG_STORAGE_UNAVAILABLE'}],done
                inspected=call(client,request('inspect',{'session_id':snapshot['session']}));assert inspected['result']['session']['state']=='runnable',inspected
                assert sql("SELECT count(*) FROM cg_cognitive_journals j, jsonb_array_elements(j.payload::jsonb->'sessions') s WHERE j.kind='task-sessions-v2' AND j.scope='"+scope+"' AND s->'checkpoint'->'pre_execution_goal' IS NOT NULL AND s->'checkpoint'->'snapshot'->>'state'='runnable'")=='1'
                assert sql("SELECT string_agg(r->>'kind',',' ORDER BY r->>'kind') FROM cg_cognitive_journals j, jsonb_array_elements(j.payload::jsonb->'records') r WHERE j.kind='task-sessions-v2' AND j.scope='"+scope+"'")=='pre_execution_goal'
                sql('DROP TRIGGER '+trigger+' ON cg_cognitive_journals; DROP FUNCTION '+function+'();')
                barrier_created=False
            elif revoked:pass
            elif case=='false':
                assert done['diagnostics']==[{'code':'CG_UNSUPPORTED_CAPABILITY'}],done
                inspected=call(client,request('inspect',{'session_id':snapshot['session']}));assert inspected['result']['session']['state']=='failed',inspected
            else:assert done['result']['session']['state']=='completed',done
        finally:
            if barrier_created:sql('DROP TRIGGER IF EXISTS '+trigger+' ON cg_cognitive_journals; DROP FUNCTION IF EXISTS '+function+'();')
            (root/'release-discovery').touch()
            assert client.close()==0
        wire=[json.loads(line) for line in (root/'wire.jsonl').read_text().splitlines()]
        effects=[entry for entry in wire if entry['method']=='tools/call'];assert len(effects)==(0 if revoked else 2 if case=='two' else 1),wire
        assert all(entry['params']['name']=='records.read' for entry in effects)
        if case=='two':assert {entry['params']['arguments']['query'] for entry in effects}=={'one','two'}
        if case=='replay-drift':
            path=root/'normalization-0.json';document=json.loads(path.read_text());document['revision_field']='other_revision';path.write_text(json.dumps(document))
        if case!='false' and not revoked:
            admission=json.loads(Path(launch[13]).read_text())
            operator=root/'recover.json';operator.write_text(json.dumps({'session_id':snapshot['session'],'issuer':admission['mappings'][0]['principal'],'decision':'recover'}))
            recovery=subprocess.run([str(BIN/'cg-local'),'--operation','session.authority','--request',str(operator),*launch],env=environment,capture_output=True,text=True,timeout=45)
            recovered=json.loads(recovery.stdout)
            if case=='replay-drift':assert recovered['status']!='ok' or recovered['result']['session']['state']!='completed',recovered
            else:
                assert recovery.returncode==0,(recovery.stdout,recovery.stderr)
                assert recovered['status']=='ok' and recovered['result']['session']['state']=='completed',recovered
                reopened=Client(launch,environment=environment)
                try:
                    assert 'result' in reopened.initialize()
                    inspected=call(reopened,request('inspect',{'session_id':snapshot['session']}));assert inspected['result']['session']['state']=='completed',inspected
                    evidence_ref=inspected['result']['session']['final_evidence'];scope=base['scope']
                    uri=f"cg://workspaces/{scope['workspace_id']}/projects/{scope['project_id']}/bindings/{scope['binding_id']}/references/{evidence_ref['id']}/{evidence_ref['revision']}/{evidence_ref['digest']}"
                    resource=reopened.exchange('resources/read',{'uri':uri});assert 'result' in resource,resource
                    evidence_resource=json.loads(resource['result']['contents'][0]['text'])
                    assert evidence_resource['contract']=='cg.evidence' and evidence_resource['reference']==evidence_ref
                    evidence_document=evidence_resource['document']
                    assert evidence_document['contract']=='cg.connector-pre-execution-goal-verification'
                    assert evidence_document['purpose']=='PRE_EXECUTION_GOAL'
                    assert evidence_document['facts'] and evidence_document['evidence']
                    assert not {'invocation','artifact','after','invocation_ir'} & evidence_document.keys()

                finally:assert reopened.close()==0
        scope=json.loads(Path(launch[13]).read_text())['mappings'][0]['canonical_scope']
        assert scope.replace('-','').isalnum()
        stored=json.loads(sql("SELECT payload FROM cg_cognitive_journals WHERE kind='task-sessions-v2' AND scope='"+scope+"'"))
        cp=stored['sessions'][0]['checkpoint']
        assert cp['pending'] is None and cp['accepted_consent'] is None
        assert not stored['grants']
        kinds={record['kind'] for record in stored['records']}
        assert kinds <= {'pre_execution_goal','evidence'}
        if case in ('one','two','recovery'):
            assert kinds=={'pre_execution_goal','evidence'}
            assert evidence_document['read_manifest']==cp['pre_execution_goal']
            assert evidence_document['goal_digest']==cp['pre_execution_goal']['goal_digest']
            assert evidence_document['readset_digest']==cp['pre_execution_goal']['readset_digest']
            assert evidence_document['session']==cp['snapshot']['session'] and evidence_document['run']==cp['snapshot']['run']
            assert evidence_document['owner']==cp['owner']
            assert any(pin['contract']=='cg.connector-goal' and pin['digest']==digest(intent) for pin in evidence_document['read_manifest']['definition_pins'])
            record=next(record for record in stored['records'] if record['kind']=='pre_execution_goal')
            manifest=json.loads(bytes(record['bytes']))
            assert {read['payload']['condition'] for read in manifest['reads']}=={condition['id'] for condition in intent['desired_state']['conditions']}
            persisted_evidence=next(record for record in stored['records'] if record['kind']=='evidence')
            assert json.loads(bytes(persisted_evidence['bytes']))==evidence_document
            assert 'sha256:'+hashlib.sha256(bytes(persisted_evidence['bytes'])).hexdigest()==evidence_ref['digest']

        if case=='false' or revoked:assert not kinds
        if case=='replay-drift':assert kinds=={'pre_execution_goal'}
        ledger=cp['resource_ledger']
        assert ledger is not None
        def no_model_or_worker(value):
            if isinstance(value,dict):
                for key,item in value.items():
                    if key in ('model_attempts','input_tokens','output_tokens','output_bytes','worker_attempts','worker_compute_units','repair_attempts'):assert item==0,(key,item)
                    else:no_model_or_worker(item)
            elif isinstance(value,list):
                for item in value:no_model_or_worker(item)
        no_model_or_worker(ledger)
        assert len([entry for entry in (root/'wire.jsonl').read_text().splitlines() if json.loads(entry)['method']=='tools/call'])==len(effects)
        print('INSTALLED_GOAL_QUALIFICATION_COMPLETE',flush=True)
if __name__=='__main__':run(sys.argv[1])
