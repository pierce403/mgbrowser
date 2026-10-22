#![cfg(feature = "modern")]
//! Frozen authored self-Window messaging controls, not upstream WPT tests.
use mg_sparkle::{
    document::{self, Document},
    js_browser::{Request, boa::BoaPageRealm},
    page_session::{
        DefaultAction, InputKind, LifecyclePhase, RealmState, SessionInput, SessionReply, TaskPump,
    },
};
use std::time::Duration;

const URL: &str = "https://example.test/message-control";

fn request(source: &str) -> Request {
    Request {
        url: URL.into(),
        external_scripts: None,
        html: format!(
            "<html><head><title>before</title></head><body><a id=a href=/event-trap>Owned input</a><script>{source}</script></body></html>"
        ),
    }
}
fn start(source: &str) -> (BoaPageRealm, SessionReply) {
    // Later-message controls explicitly create their workload during native
    // load. Stop at LoadComplete, not queue-idle: the message remains pending.
    let (mut realm, init) = raw_start(&format!("window.onload=function(){{{source}\n}};"));
    assert_eq!(
        (
            init.boa.as_ref().unwrap().jobs_admitted,
            init.boa.as_ref().unwrap().jobs_executed
        ),
        (2, 0)
    );
    let dcl = pump(&mut realm, &init);
    assert_eq!(
        dcl.lifecycle.unwrap().phase,
        LifecyclePhase::DomContentLoaded
    );
    let loaded = pump(&mut realm, &dcl);
    assert_eq!(
        loaded.lifecycle.unwrap().phase,
        LifecyclePhase::LoadComplete
    );
    (realm, loaded)
}
fn raw_start(source: &str) -> (BoaPageRealm, SessionReply) {
    let (realm, reply) = BoaPageRealm::start(request(source));
    assert_eq!(reply.state, RealmState::Ready, "{:?}", reply.errors);
    clean(&reply);
    assert!(reply.accounting_valid());
    (realm.expect("owned retained realm"), reply)
}
fn source_complete(source: &str) -> (BoaPageRealm, SessionReply) {
    // Synchronous conversion/constructor controls retain original source-time
    // execution. Only their otherwise-empty mandatory lifecycle is awaited.
    let (mut realm, init) = raw_start(source);
    let loaded = finish_load(&mut realm, init);
    clean(&loaded);
    (realm, loaded)
}
fn finish_load(realm: &mut BoaPageRealm, mut reply: SessionReply) -> SessionReply {
    for _ in 0..4 {
        if reply.lifecycle.unwrap().phase == LifecyclePhase::LoadComplete {
            return reply;
        }
        reply = pump(realm, &reply);
    }
    panic!("owned initial lifecycle did not complete")
}
fn projected(reply: &SessionReply) -> Document {
    document::project_nodes(reply.snapshot.as_ref().unwrap().nodes.clone(), URL).unwrap()
}
fn title(reply: &SessionReply) -> String {
    projected(reply).title
}
fn clean(reply: &SessionReply) {
    assert!(reply.errors.is_empty(), "{:?}", reply.errors);
}
fn no_work(reply: &SessionReply) {
    assert!(reply.task_wake.is_none());
    assert_eq!(reply.boa.as_ref().unwrap().pending_jobs, 0);
}
fn pump(realm: &mut BoaPageRealm, previous: &SessionReply) -> SessionReply {
    let wake = previous
        .task_wake
        .as_ref()
        .expect("advertised later message task");
    assert!(wake.delay_ms <= 100, "only owned short-delay controls");
    if wake.delay_ms != 0 {
        std::thread::sleep(Duration::from_millis(u64::from(wake.delay_ms)));
    }
    let reply = realm.pump_tasks(TaskPump {
        wake_token: wake.token,
    });
    assert_eq!(reply.revision, previous.revision + 1);
    assert_eq!(reply.state, RealmState::Ready, "{:?}", reply.errors);
    assert_eq!(reply.default_action, DefaultAction::None);
    assert!(reply.acknowledgements.is_empty());
    assert!(reply.outcome.click_canceled.is_none());
    assert!(reply.outcome.submit_canceled.is_none());
    assert_eq!(reply.scripts_executed, 0);
    assert!(reply.accounting_valid());
    reply
        .boa
        .as_ref()
        .unwrap()
        .validate_after(previous.boa.as_ref().unwrap())
        .unwrap();
    assert_eq!(
        reply.boa.as_ref().unwrap().pending_jobs > 0,
        reply.task_wake.is_some()
    );
    reply
}
fn click(realm: &mut BoaPageRealm, previous: &SessionReply) -> SessionReply {
    let target = projected(previous)
        .query_selector(0, "#a")
        .unwrap()
        .unwrap();
    let reply = realm.dispatch(SessionInput {
        kind: InputKind::Click { target },
        edits: vec![],
    });
    assert_eq!(reply.state, RealmState::Ready, "{:?}", reply.errors);
    assert_eq!(reply.revision, previous.revision + 1);
    assert!(reply.accounting_valid());
    reply
}

#[test]
fn native_domexception_has_error_identity_stack_codes_and_guarded_constructor() {
    let (_realm, init) = source_complete(
        r#"
        var order=[];
        const error=new DOMException({toString(){order.push('message');return 'owned';}},
            {toString(){order.push('name');return 'DataCloneError';}});
        if(order.join(',')!=='message,name'||!(error instanceof DOMException)||!(error instanceof Error)||
            Object.getPrototypeOf(error)!==DOMException.prototype||
            Object.getPrototypeOf(DOMException.prototype)!==Error.prototype||
            Object.prototype.toString.call(error)!=='[object DOMException]'||
            (error.stack!==undefined&&typeof error.stack!=='string')||
            error.name!=='DataCloneError'||error.message!=='owned'||error.code!==25||
            DOMException.DATA_CLONE_ERR!==25||DOMException.prototype.SYNTAX_ERR!==12||
            DOMException.NOT_SUPPORTED_ERR!==9)throw 'not a native DOMException';
        const empty=new DOMException(),unknown=new DOMException('message','OwnedUnknown');
        if(empty.name!=='Error'||empty.message!==''||empty.code!==0||unknown.code!==0)throw 'defaults/code';
        let rejected=0;
        for(const name of ['name','message','code']){
            const getter=Object.getOwnPropertyDescriptor(DOMException.prototype,name).get;
            try{getter.call(Object.create(DOMException.prototype));}catch(e){if(!(e instanceof TypeError))throw e;rejected++;}
        }
        try{DOMException('not constructed');}catch(e){if(!(e instanceof TypeError))throw e;rejected++;}
        try{new DOMException(Symbol('message'));}catch(e){if(!(e instanceof TypeError))throw e;rejected++;}
        try{error.name='forged';error.message='forged';error.code=0;}catch(e){}
        if(error.name!=='DataCloneError'||error.message!=='owned'||error.code!==25||rejected!==5)throw 'brand/readonly';
        document.title='native DOMException';
    "#,
    );
    assert_eq!(title(&init), "native DOMException");
    no_work(&init);
}

#[test]
fn domexception_defaults_utf16_new_target_order_and_native_brand_do_not_use_page_getters() {
    let (_realm, init) = source_complete(
        r#"
        const Native=DOMException,proto=Native.prototype;
        const messageGetter=Object.getOwnPropertyDescriptor(proto,'message').get;
        const nameGetter=Object.getOwnPropertyDescriptor(proto,'name').get;
        const codeGetter=Object.getOwnPropertyDescriptor(proto,'code').get;
        const defaults=new Native(undefined,undefined),nulls=new Native(null,null);
        const utf16='nul\0lone\ud800end\udfff';
        if(defaults.message!==''||defaults.name!=='Error'||nulls.message!=='null'||nulls.name!=='null'||
            new Native(utf16,utf16).message!==utf16||new Native(utf16,utf16).name!==utf16)throw 'DOMString/default conversion';
        const order=[],chosen=Object.create(proto);
        const target=new Proxy(function(){},{get(object,key,receiver){
            if(key==='prototype'){order.push('prototype');return chosen;}return Reflect.get(object,key,receiver);
        }});
        const constructed=Reflect.construct(Native,[{toString(){order.push('message');return 'owned';}},
            {toString(){order.push('name');return 'DataCloneError';}}],target);
        if(order.join(',')!=='message,name,prototype'||Object.getPrototypeOf(constructed)!==chosen||
            messageGetter.call(constructed)!=='owned'||nameGetter.call(constructed)!=='DataCloneError'||
            codeGetter.call(constructed)!==25)throw 'newTarget conversion order/brand';
        let effects=0,rejected=0;
        try{Native({toString(){effects++;throw 'must not coerce';}});}catch(error){if(!(error instanceof TypeError))throw error;rejected++;}
        const forged={get name(){effects++;throw 'forged name';},get message(){effects++;throw 'forged message';}};
        const proxy=new Proxy(constructed,{get(){effects++;throw 'proxy brand getter';}});
        for(const getter of [messageGetter,nameGetter,codeGetter])for(const receiver of [proto,Object.create(proto),forged,proxy]){
            try{getter.call(receiver);throw 'forged DOMException accepted';}catch(error){if(!(error instanceof TypeError))throw error;rejected++;}
        }
        if(effects!==0||rejected!==13)throw 'native receiver guards';document.title='native exception conversion';
    "#,
    );
    assert_eq!(title(&init), "native exception conversion");
    no_work(&init);
}

#[test]
fn primitive_values_preserve_utf16_bigint_negative_zero_and_nan() {
    let (mut realm, init) = start(
        r#"
        const values=[undefined,null,false,true,0,-0,NaN,Infinity,-Infinity,42,0n,-1n,
            123456789012345678901234567890n,-123456789012345678901234567890n,
            'nul\0lone\ud800end\udfff🦀'];
        window.onmessage=event=>{
            if(!Array.isArray(event.data)||event.data.length!==values.length)throw 'primitive container';
            for(let i=0;i<values.length;i++)if(!Object.is(event.data[i],values[i]))throw 'primitive changed';
            document.title='primitive clone';
        };
        postMessage(values,'/');
    "#,
    );
    assert_eq!(title(&init), "before");
    let reply = pump(&mut realm, &init);
    clean(&reply);
    assert_eq!(title(&reply), "primitive clone");
    assert_eq!(reply.boa.as_ref().unwrap().jobs_admitted, 3);
    assert_eq!(reply.boa.as_ref().unwrap().jobs_executed, 3);
    no_work(&reply);
}

#[test]
fn graph_snapshot_preserves_cycles_aliases_and_not_sender_mutation() {
    let (mut realm, init) = start(
        r#"
        const child={value:42},sent={child,alias:child};sent.self=sent;
        window.onmessage=event=>{
            const clone=event.data;
            if(clone===sent||clone.child===child||clone.child!==clone.alias||clone.self!==clone||
                clone.child.value!==42||Object.hasOwn(clone,'after'))throw 'graph not an enqueue snapshot';
            document.title='graph clone';
        };
        postMessage(sent,'*');sent.child.value=99;sent.after=true;
    "#,
    );
    let reply = pump(&mut realm, &init);
    clean(&reply);
    assert_eq!(title(&reply), "graph clone");
    no_work(&reply);
}

#[test]
fn sparse_array_keeps_holes_large_length_properties_and_cycles_without_dense_storage() {
    let (mut realm, init) = start(
        r#"
        const source=[];source.length=4294967295;source[4294967294]='tail';source[3]=undefined;source.extra=source;
        Object.setPrototypeOf(source,{get 0(){throw 'inherited hole getter read';}});
        window.onmessage=event=>{
            const clone=event.data;
            if(!Array.isArray(clone)||clone.length!==4294967295||Object.hasOwn(clone,'0')||
                !Object.hasOwn(clone,'3')||clone[3]!==undefined||clone[4294967294]!=='tail'||
                clone.extra!==clone||Object.getPrototypeOf(clone)!==Array.prototype)throw 'sparse geometry lost';
            document.title='sparse clone';
        };
        postMessage(source,'/');
    "#,
    );
    let reply = pump(&mut realm, &init);
    clean(&reply);
    assert_eq!(title(&reply), "sparse clone");
    no_work(&reply);
}

#[test]
fn ordinary_custom_null_and_window_prototypes_are_ignored_and_poison_keys_are_data() {
    let (mut realm, init) = start(
        r#"
        let traps=0;Object.defineProperty(Object.prototype,'poison',{set(){traps++;},configurable:true});
        const bare=Object.create(null);bare['__proto__']='owned';bare['\ud800']='key';
        const custom=Object.create({get inherited(){throw 'inherited getter read';}});custom.own=7;
        const hostProto=Object.create(window);hostProto.own=8;
        class Owned{#hidden=3;constructor(){this.visible=9;}}
        window.onmessage=event=>{
            const [a,b,c,d,e]=event.data;
            if(Object.getPrototypeOf(a)!==Object.prototype||a.__proto__!=='owned'||a['\ud800']!=='key'||
                Object.getPrototypeOf(b)!==Object.prototype||b.own!==7||Object.hasOwn(b,'inherited')||
                c.own!==8||Object.getPrototypeOf(c)!==Object.prototype||d.visible!==9||
                Object.getPrototypeOf(d)!==Object.prototype||e.poison!==10||traps!==0)throw 'record/prototype clone';
            document.title='ordinary records';
        };
        postMessage([bare,custom,hostProto,new Owned(),{poison:10}],'*');
    "#,
    );
    let reply = pump(&mut realm, &init);
    clean(&reply);
    assert_eq!(title(&reply), "ordinary records");
}

#[test]
fn initial_enumerable_key_snapshot_rechecks_ownership_but_not_enumerability() {
    let (mut realm, init) = start(
        r#"
        let reads=[];const prototype={get deleted(){throw 'inherited deleted value read';}};
        const source=Object.create(prototype);
        Object.defineProperty(source,'first',{enumerable:true,get(){
            reads.push('first');Object.defineProperty(source,'hiddenLater',{enumerable:false});
            delete source.deleted;source.newKey=9;Object.defineProperty(source,'newlyVisible',{enumerable:true});return 1;
        }});
        source.hiddenLater=2;Object.defineProperty(source,'deleted',{value:3,enumerable:true,configurable:true});
        Object.defineProperty(source,'newlyVisible',{value:4,enumerable:false,configurable:true});
        Object.defineProperty(source,Symbol('ignored'),{enumerable:true,get(){throw 'symbol key read';}});
        Object.defineProperty(source,'toJSON',{value(){throw 'JSON conversion used';}});
        window.onmessage=event=>{
            const clone=event.data;
            if(Object.keys(clone).join(',')!=='first,hiddenLater'||clone.first!==1||clone.hiddenLater!==2||
                Object.hasOwn(clone,'deleted')||Object.hasOwn(clone,'newKey')||Object.hasOwn(clone,'newlyVisible')||
                Object.getOwnPropertySymbols(clone).length!==0||reads.join(',')!=='first')throw 'wrong key snapshot';
            document.title='key snapshot';
        };
        postMessage(source,'/');
    "#,
    );
    let reply = pump(&mut realm, &init);
    clean(&reply);
    assert_eq!(title(&reply), "key snapshot");
}

#[test]
fn iterative_graph_walk_preserves_depth_first_getter_order_and_single_reads() {
    let (mut realm, init) = start(
        r#"
        const reads=[],child={get x(){reads.push('x');source.later=9;return 4;}};
        const source={get a(){reads.push('a');return child;},get b(){reads.push('b');return 5;},later:6};
        window.onmessage=event=>{
            if(reads.join(',')!=='a,x,b'||event.data.a.x!==4||event.data.b!==5||event.data.later!==9)throw 'BFS/repeated getter';
            document.title='depth first';
        };
        postMessage(source,'*');
        if(reads.join(',')!=='a,x,b')throw 'serialization was deferred';
    "#,
    );
    let reply = pump(&mut realm, &init);
    clean(&reply);
    assert_eq!(title(&reply), "depth first");
}

#[test]
fn noncloneable_values_are_real_datacloneerrors_without_proxy_traps_or_events() {
    let (_realm, init) = source_complete(
        r#"
        let traps=0,failures=0;
        const proxy=new Proxy({}, {ownKeys(){traps++;throw 'proxy inspected';},get(){traps++;throw 'proxy read';}});
        const revoked=Proxy.revocable([],{});revoked.revoke();
        const values=[Symbol('value'),Object(Symbol('boxed')),()=>{},proxy,revoked.proxy,Promise.resolve(),
            new WeakMap(),new WeakSet(),new WeakRef({}),document,window,self,parent,top,globalThis];
        for(const value of values){
            try{postMessage({nested:value},'*');throw 'noncloneable accepted';}
            catch(error){if(!(error instanceof DOMException)||error.name!=='DataCloneError'||error.code!==25)throw error;failures++;}
        }
        if(failures!==values.length||traps!==0)throw 'wrong clone rejection';
        document.title='noncloneables rejected';
    "#,
    );
    assert_eq!(title(&init), "noncloneables rejected");
    no_work(&init);
}

#[test]
fn excluded_cloneable_builtins_are_explicit_not_supported_not_empty_records() {
    let (_realm, init) = source_complete(
        r#"
        const values=[new Boolean(false),new Number(2),new String('owned'),Object(3n),new Date(0),/owned/gi,
            new Map([[1,2]]),new Set([3]),new ArrayBuffer(4),new Uint8Array(4),new DataView(new ArrayBuffer(4)),
            new Error('owned'),new DOMException('owned','DataCloneError')];
        for(const value of values){
            try{postMessage({nested:value},'/');throw 'unsupported clone silently accepted';}
            catch(error){if(!(error instanceof DOMException)||error.name!=='NotSupportedError'||error.code!==9)throw error;}
        }
        document.title='unsupported explicit';
    "#,
    );
    assert_eq!(title(&init), "unsupported explicit");
    no_work(&init);
}

#[test]
fn valid_transfers_are_unsupported_and_invalid_duplicates_do_not_detach() {
    let (_realm, init) = source_complete(
        r#"
        const buffer=new ArrayBuffer(8);let visited=0;
        function expect(name,action){try{action();throw 'transfer accepted';}catch(error){
            if(!(error instanceof DOMException)||error.name!==name)throw error;}}
        expect('NotSupportedError',()=>postMessage({get x(){visited++;return 1;}},'*',[buffer]));
        expect('DataCloneError',()=>postMessage(1,'*',[buffer,buffer]));
        expect('DataCloneError',()=>postMessage(1,'*',[{}]));
        expect('DataCloneError',()=>postMessage(1,{transfer:[new Uint8Array(2)]}));
        let type=false;try{postMessage(1,'*',[1]);}catch(error){type=error instanceof TypeError;}
        if(!type||buffer.byteLength!==8||visited!==0)throw 'transfer conversion/detachment';
        document.title='transfers rejected safely';
    "#,
    );
    assert_eq!(title(&init), "transfers rejected safely");
    assert_eq!(init.boa.as_ref().unwrap().jobs_admitted, 2);
    no_work(&init);
}

#[test]
fn nonisolated_shared_buffers_are_datacloneerrors() {
    let (_realm, init) = source_complete(
        r#"
        // Pinned production Boa deliberately excludes experimental .transfer.
        // The actual detached-buffer clone/transfer control uses its unconditional
        // native detach API in the private binding tests, not a new page builtin.
        for(const value of [new SharedArrayBuffer(4)]){
            try{postMessage(value,'/');throw 'invalid buffer accepted';}
            catch(error){if(!(error instanceof DOMException)||error.name!=='DataCloneError'||error.code!==25)throw error;}
        }
        document.title='invalid buffers rejected';
    "#,
    );
    assert_eq!(title(&init), "invalid buffers rejected");
    no_work(&init);
}

#[test]
fn tasks_are_fifo_not_synchronous_or_promise_aliases_and_nested_tasks_stay_later() {
    let (mut realm, init) = start(
        r#"
        var order=['script'];
        window.onmessage=event=>{
            order.push(event.data);
            if(event.data==='a'){
                postMessage('inner','/');Promise.resolve().then(()=>{order.push('job');postMessage('job-message','/');});
            }
            document.title=order.join(',');
        };
        postMessage('a','/');postMessage('b','/');
        Promise.resolve().then(()=>order.push('script-job'));
        if(order.join(',')!=='script')throw 'message ran synchronously';
    "#,
    );
    assert_eq!(title(&init), "before");
    let first = pump(&mut realm, &init);
    clean(&first);
    assert_eq!(title(&first), "script,script-job,a");
    let second = pump(&mut realm, &first);
    clean(&second);
    assert_eq!(title(&second), "script,script-job,a,job,b");
    let third = pump(&mut realm, &second);
    clean(&third);
    assert_eq!(title(&third), "script,script-job,a,job,b,inner");
    let fourth = pump(&mut realm, &third);
    clean(&fourth);
    assert_eq!(
        title(&fourth),
        "script,script-job,a,job,b,inner,job-message"
    );
    no_work(&fourth);
}

#[test]
fn getter_posted_message_queues_before_outer_after_synchronous_serialization() {
    let (mut realm, init) = start(
        r#"
        var order=[];
        window.onmessage=event=>{order.push(event.data.tag);document.title=order.join(',');};
        postMessage({get tag(){postMessage({tag:'inner'},'/');order.push('getter');return 'outer';}},'/');
        if(order.join(',')!=='getter')throw 'not synchronous clone/no later task';
    "#,
    );
    assert_eq!(init.boa.as_ref().unwrap().jobs_admitted, 4);
    let inner = pump(&mut realm, &init);
    clean(&inner);
    assert_eq!(title(&inner), "getter,inner");
    let outer = pump(&mut realm, &inner);
    clean(&outer);
    assert_eq!(title(&outer), "getter,inner,outer");
    no_work(&outer);
}

#[test]
fn throwing_getter_preserves_original_exception_and_successful_inner_task_only() {
    let (mut realm, init) = start(
        r#"
        const thrown={owned:'getter'},source={get x(){postMessage('inner','/');throw thrown;}};
        let caught;try{postMessage(source,'/');}catch(error){caught=error;}
        if(caught!==thrown)throw 'getter exception replaced';
        window.onmessage=event=>{if(event.data!=='inner')throw 'failed outer queued';document.title='inner preserved';};
    "#,
    );
    assert_eq!(init.boa.as_ref().unwrap().jobs_admitted, 4);
    assert_eq!(init.boa.as_ref().unwrap().pending_jobs, 1);
    let reply = pump(&mut realm, &init);
    clean(&reply);
    assert_eq!(title(&reply), "inner preserved");
    assert_eq!(reply.boa.as_ref().unwrap().jobs_executed, 3);
    no_work(&reply);
}

#[test]
fn no_listener_at_enqueue_still_clones_and_later_listener_receives_snapshot() {
    let (mut realm, init) = start(
        r#"
        var reads=0;const source={get value(){reads++;return 7;}};
        postMessage(source);
        if(reads!==1)throw 'no-listener clone skipped';
        // The listener is genuinely installed after enqueue, before delivery.
        window.onmessage=event=>{
            if(event.data.value!==7||reads!==1)throw 'late listener clone';document.title='late listener';
        };
    "#,
    );
    let reply = pump(&mut realm, &init);
    clean(&reply);
    assert_eq!(title(&reply), "late listener");
    no_work(&reply);
}

#[test]
fn eventual_origin_mismatch_still_serializes_and_throws_before_nondelivery() {
    let (mut realm, init) = start(
        r#"
        var reads=0,calls=0;window.onmessage=()=>calls++;
        postMessage({get value(){reads++;return 7;}},'https://foreign.test/elsewhere');
        const thrown={proof:42};let caught;
        try{postMessage({get value(){throw thrown;}},'https://foreign.test');}catch(error){caught=error;}
        if(reads!==1||caught!==thrown)throw 'mismatch skipped serialization';
        document.getElementById('a').onclick=event=>{event.preventDefault();if(calls!==0)throw 'mismatch delivered';document.title='mismatch discarded';};
    "#,
    );
    assert_eq!(init.boa.as_ref().unwrap().jobs_admitted, 4);
    assert_eq!(init.boa.as_ref().unwrap().pending_jobs, 1);
    let dropped = pump(&mut realm, &init);
    clean(&dropped);
    assert_eq!(dropped.boa.as_ref().unwrap().jobs_executed, 3);
    no_work(&dropped);
    let checked = click(&mut realm, &dropped);
    clean(&checked);
    assert_eq!(title(&checked), "mismatch discarded");
}

#[test]
fn normalized_origins_options_and_opaque_targets_use_url_origin_not_navigation_rules() {
    let (mut realm, init) = start(
        r#"
        var received=[];
        window.onmessage=event=>{
            if(event.origin!=='https://example.test'||event.source!==window)throw 'sender origin/source';
            received.push(event.data);document.title=received.join(',');
        };
        postMessage('default');postMessage('slash','/');postMessage('wildcard','*');
        postMessage('normalized','https://user:pass@EXAMPLE.test:443/path?query#fragment');
        postMessage('options',{targetOrigin:'https://example.test/other',transfer:[]});
        postMessage('opaque','data:text/plain,owned');postMessage('other-scheme','http://example.test');
    "#,
    );
    let mut reply = init;
    for _ in 0..7 {
        reply = pump(&mut realm, &reply);
        clean(&reply);
    }
    assert_eq!(title(&reply), "default,slash,wildcard,normalized,options");
    no_work(&reply);
}

#[test]
fn invalid_target_is_domexception_syntaxerror_before_clone_and_unsupported_transfer() {
    let (_realm, init) = source_complete(
        r#"
        const Native=DOMException;let reads=0,traps=0;
        globalThis.DOMException=function(){traps++;throw 'page exception constructor called';};
        for(const target of ['', 'relative', '/not-the-special-slash', 'https://']){
            try{postMessage({get value(){reads++;return 1;}},target,[new ArrayBuffer(1)]);throw 'invalid target accepted';}
            catch(error){if(!(error instanceof Native)||!(error instanceof Error)||error instanceof SyntaxError||
                error.name!=='SyntaxError'||error.code!==12)throw error;}
        }
        if(reads!==0||traps!==0)throw 'origin/constructor order';
        document.title='syntax before clone';
    "#,
    );
    assert_eq!(title(&init), "syntax before clone");
    assert_eq!(init.boa.as_ref().unwrap().jobs_admitted, 2);
    no_work(&init);
}

#[test]
fn overload_conversion_and_empty_transfer_iteration_precede_cloning_once() {
    let (mut realm, init) = start(
        r#"
        var order=[];
        const empty={ [Symbol.iterator](){order.push('iterator');return {next(){order.push('next');return {done:true};}};}};
        const options={get targetOrigin(){order.push('target');return {toString(){order.push('string');return '/';}};},
            get transfer(){order.push('transfer');return empty;}};
        postMessage({get value(){order.push('clone');return 7;}},options);
        if(order.join(',')!=='transfer,iterator,next,target,string,clone')throw 'IDL/clone order';
        window.onmessage=event=>{if(event.data.value!==7)throw 'options clone';document.title='converted once';};
    "#,
    );
    let reply = pump(&mut realm, &init);
    clean(&reply);
    assert_eq!(title(&reply), "converted once");
}

#[test]
fn arity_selects_dictionary_or_legacy_overload_without_reading_unselected_members() {
    let (mut realm, init) = start(
        r#"
        var received=[],strings=0;
        window.onmessage=event=>{received.push(event.data);document.title=received.join(',');};
        postMessage('one');postMessage('null',null);postMessage('undefined',undefined);postMessage('dictionary',{});
        const legacy={get transfer(){throw 'dictionary transfer read in legacy overload';},
            get targetOrigin(){throw 'dictionary target read in legacy overload';},toString(){strings++;return '/';}};
        postMessage('three',legacy,undefined);
        postMessage('four','/',undefined,{get ignored(){throw 'fourth argument read';}});
        postMessage('primitive','/');
        let rejected=0;
        for(const action of [()=>postMessage('legacy undefined',undefined,undefined),()=>postMessage('primitive invalid',false)]){
            try{action();throw 'invalid legacy origin accepted';}
            catch(error){if(!(error instanceof DOMException)||error.name!=='SyntaxError')throw error;rejected++;}
        }
        if(strings!==1||rejected!==2)throw 'overload arity conversion';
    "#,
    );
    assert_eq!(init.boa.as_ref().unwrap().jobs_admitted, 9);
    let mut reply = init;
    for _ in 0..7 {
        reply = pump(&mut realm, &reply);
        clean(&reply);
    }
    assert_eq!(
        title(&reply),
        "one,null,undefined,dictionary,three,four,primitive"
    );
    no_work(&reply);
}

#[test]
fn transfer_sequence_item_failure_closes_iterator_before_target_getter_and_origin_validation() {
    let (_realm, init) = source_complete(
        r#"
        const order=[];
        const transfer={ [Symbol.iterator](){order.push('iterator');return {
            next(){order.push('next');return {done:false,value:1};},
            return(){order.push('close');return {done:true};}
        };}};
        let rejected=false;
        try{postMessage({get value(){throw 'clone before IDL';}}, {
            get transfer(){order.push('transfer');return transfer;},
            get targetOrigin(){throw 'target getter after invalid sequence item';}
        });}catch(error){rejected=error instanceof TypeError;}
        if(!rejected||order.join(',')!=='transfer,iterator,next,close')throw 'IteratorClose/conversion order';
        // Conversion accepts an object item before origin syntax is checked;
        // transferability validation must not replace that SyntaxError.
        let syntax=false;
        try{postMessage(1,{transfer:[{}],targetOrigin:'not an absolute origin'});}
        catch(error){syntax=error instanceof DOMException&&error.name==='SyntaxError';}
        if(!syntax)throw 'origin/transfer-validation precedence';document.title='sequence conversion first';
    "#,
    );
    assert_eq!(title(&init), "sequence conversion first");
    assert_eq!(init.boa.as_ref().unwrap().jobs_admitted, 2);
    no_work(&init);
}

#[test]
fn legacy_origin_conversion_precedes_transfer_iteration_and_keeps_original_abrupt_value() {
    let (mut realm, init) = start(
        r#"
        const order=[],thrown={proof:42};let caught;
        try{postMessage(1,{toString(){order.push('throw');throw thrown;}},{
            get [Symbol.iterator](){throw 'legacy transfer converted before origin';}
        });}catch(error){caught=error;}
        if(caught!==thrown)throw 'USVString exception replaced';
        const transfer={ [Symbol.iterator](){order.push('iterator');return {next(){order.push('next');return {done:true};}};}};
        postMessage({get value(){order.push('clone');return 7;}},{toString(){order.push('origin');return 'https://example.test/\ud800';}},transfer);
        if(order.join(',')!=='throw,origin,iterator,next,clone')throw 'legacy IDL order';
        window.onmessage=event=>{if(event.data.value!==7)throw 'USVString origin match';document.title='legacy conversion';};
    "#,
    );
    assert_eq!(init.boa.as_ref().unwrap().jobs_admitted, 3);
    let reply = pump(&mut realm, &init);
    clean(&reply);
    assert_eq!(title(&reply), "legacy conversion");
    no_work(&reply);
}

#[test]
fn argument_conversion_exceptions_and_wrong_receivers_admit_no_clone_work() {
    let (_realm, init) = source_complete(
        r#"
        const thrown={proof:1};let caught,rejected=0;
        try{postMessage(1,{get targetOrigin(){throw thrown;}});}catch(error){caught=error;}
        if(caught!==thrown)throw 'conversion exception replaced';
        for(const action of [()=>postMessage.call({},1,'*'),()=>postMessage.call(document,1,'*'),
            ()=>postMessage(),()=>postMessage(1,Symbol('origin')),()=>postMessage(1,{transfer:[1]})]){
            try{action();throw 'invalid call accepted';}catch(error){if(!(error instanceof TypeError))throw error;rejected++;}
        }
        if(rejected!==5)throw 'wrong receiver/conversion';document.title='calls rejected';
    "#,
    );
    assert_eq!(title(&init), "calls rejected");
    assert_eq!(init.boa.as_ref().unwrap().jobs_admitted, 2);
    no_work(&init);
}

#[test]
fn sender_origin_ignores_base_aliases_and_location_proposals() {
    let (mut realm, init) = start(
        r#"
        const actual=globalThis,send=postMessage,origin=location.origin;
        const base=document.createElement('base');base.href='https://foreign.test/';document.head.appendChild(base);
        actual.window={postMessage(){throw 'forged window used';}};actual.self={};actual.parent={};
        actual.onmessage=event=>{
            if(event.origin!==origin||event.origin!=='https://example.test'||event.source!==actual)throw 'forged sender authority';
            document.title='trusted sender';
            // Navigation is proposed after actual delivery, not a startup
            // prefix that the parent must cancel without further task pumps.
            location.href='/proposed-navigation';
        };
        send.call(actual,{proof:42},'/');
    "#,
    );
    assert!(init.navigation.is_none());
    let reply = pump(&mut realm, &init);
    clean(&reply);
    assert_eq!(title(&reply), "trusted sender");
    assert_eq!(
        reply.navigation.as_deref(),
        Some("https://example.test/proposed-navigation")
    );
}

#[test]
fn property_false_and_prevent_default_do_not_cancel_but_existing_click_false_still_does() {
    let (mut realm, init) = start(
        r#"
        var order=[],saved;
        window.addEventListener('message',event=>{if(event.eventPhase!==2)throw 'capture phase';order.push('capture');},true);
        window.onmessage=()=>{throw 'old property slot';};
        window.addEventListener('message',event=>{
            if(event!==saved||event.defaultPrevented||event.bubbles||event.cancelable)throw 'message cancellation';
            order.push('listener');document.title=order.join(',');return false;
        });
        window.onmessage=function(event){
            if(this!==window||arguments.length!==1||event.type!=='message'||event.target!==window||
                event.currentTarget!==window||event.eventPhase!==2||event.lastEventId!==''||
                !Array.isArray(event.ports)||!Object.isFrozen(event.ports)||event.ports.length!==0)throw 'MessageEvent surface';
            saved=event;event.preventDefault();order.push('property');return false;
        };
        document.getElementById('a').onclick=()=>{document.title='click canceled';return false;};
        postMessage(1,'*');
    "#,
    );
    let reply = pump(&mut realm, &init);
    clean(&reply);
    assert_eq!(title(&reply), "capture,property,listener");
    let clicked = click(&mut realm, &reply);
    clean(&clicked);
    assert_eq!(clicked.outcome.click_canceled, Some(true));
    assert_eq!(clicked.default_action, DefaultAction::None);
    assert_eq!(title(&clicked), "click canceled");
}

#[test]
fn capture_stop_keeps_same_invocation_but_skips_noncapture_and_immediate_stops_both() {
    for (stop, expected) in [
        ("stopPropagation", "first,second"),
        ("stopImmediatePropagation", "first"),
    ] {
        let (mut realm, init) = start(&format!(
            r#"
            var order=[];
            window.addEventListener('message',event=>{{order.push('first');event.{stop}();document.title=order.join(',');}},true);
            window.addEventListener('message',()=>{{order.push('second');document.title=order.join(',');}},true);
            window.onmessage=()=>{{throw 'stopped noncapture ran';}};
            postMessage(1,'/');
        "#
        ));
        let reply = pump(&mut realm, &init);
        clean(&reply);
        assert_eq!(title(&reply), expected);
    }
}

#[test]
fn message_families_share_existing_thirty_two_cumulative_listener_slots() {
    let (mut realm, init) = raw_start(
        r#"
        var calls=0;
        for(let i=0;i<32;i++)window.addEventListener('message',()=>{calls++;document.title='listeners:'+calls;});
        let rejected=false;try{window.addEventListener('messageerror',()=>{});}catch(error){rejected=true;}
        if(!rejected)throw 'listener cap expanded';postMessage(1,'/');
    "#,
    );
    let reply = pump(&mut realm, &init);
    clean(&reply);
    assert_eq!(title(&reply), "listeners:32");
    let reply = finish_load(&mut realm, reply);
    no_work(&reply);
}

#[test]
fn removed_message_listeners_do_not_renew_slots_and_unobserved_message_still_runs() {
    let (mut realm, init) = raw_start(
        r#"
        function handler(){throw 'removed listener ran';}
        for(let i=0;i<32;i++){window.addEventListener('message',handler);window.removeEventListener('message',handler);}
        let rejected=false;try{window.onmessage=()=>{throw 'new property slot ran';};}catch(error){rejected=true;}
        if(!rejected)throw 'removed listener budget renewed';document.title='removed slots retained';postMessage(1,'/');
    "#,
    );
    assert_eq!(init.boa.as_ref().unwrap().jobs_admitted, 3);
    let reply = pump(&mut realm, &init);
    clean(&reply);
    assert_eq!(title(&reply), "removed slots retained");
    assert_eq!(reply.boa.as_ref().unwrap().jobs_executed, 1);
    let reply = finish_load(&mut realm, reply);
    no_work(&reply);
}

#[test]
fn listener_removal_is_immediate_but_new_noncapture_listener_waits_for_next_message() {
    let (mut realm, init) = start(
        r#"
        var order=[];
        function removed(){throw 'removed current listener ran';}
        function added(){order.push('added');document.title=order.join(',');}
        window.addEventListener('message',()=>{
            order.push('first');window.removeEventListener('message',removed);
            window.addEventListener('message',added);document.title=order.join(',');
        });
        window.addEventListener('message',removed);postMessage(1,'/');postMessage(2,'/');
    "#,
    );
    let first = pump(&mut realm, &init);
    clean(&first);
    assert_eq!(title(&first), "first");
    let second = pump(&mut realm, &first);
    clean(&second);
    assert_eq!(title(&second), "first,first,added");
    no_work(&second);
}

#[test]
fn listeners_share_one_clone_but_separate_messages_do_not_and_escaped_state_resets() {
    let (mut realm, init) = start(
        r#"
        var saved,seen=0;const source={value:1};
        window.onmessage=event=>{
            seen++;if(event.data.value!==1)throw 'messages share a clone';event.data.value=7;saved=event;
            try{event.origin='forged';event.source=null;event.data=source;}catch(error){}
        };
        window.addEventListener('message',event=>{
            if(event!==saved||event.data===source||event.data.value!==7||event.source!==window||event.origin!==location.origin)
                throw 'listener clone/readonly payload';
            document.title='message '+seen;
        });
        document.getElementById('a').onclick=event=>{
            event.preventDefault();if(saved.currentTarget!==null||saved.eventPhase!==0||saved.target!==window||
                saved.data.value!==7||saved.source!==window||saved.origin!==location.origin)throw 'escaped event changed';
            document.title='escaped clone';
        };
        postMessage(source,'/');postMessage(source,'/');
    "#,
    );
    let first = pump(&mut realm, &init);
    clean(&first);
    assert_eq!(title(&first), "message 1");
    let second = pump(&mut realm, &first);
    clean(&second);
    assert_eq!(title(&second), "message 2");
    let escaped = click(&mut realm, &second);
    clean(&escaped);
    assert_eq!(title(&escaped), "escaped clone");
}

#[test]
fn ordinary_callback_failure_reports_window_error_and_continues_listener_checkpoints() {
    let (mut realm, init) = start(
        r#"
        const thrown={proof:42};var order=[];
        window.onerror=(message,file,line,column,error)=>{if(error!==thrown)throw 'lost message error';order.push('error');return true;};
        window.onmessage=()=>{order.push('property');throw thrown;};
        window.addEventListener('message',()=>{order.push('listener');Promise.resolve().then(()=>{order.push('job');document.title=order.join(',');});});
        postMessage(1,'/');
    "#,
    );
    let reply = pump(&mut realm, &init);
    assert_eq!(reply.errors.len(), 1, "{:?}", reply.errors);
    assert!(
        reply.errors[0].starts_with("Event handler:"),
        "{:?}",
        reply.errors
    );
    assert_eq!(title(&reply), "property,error,listener,job");
    no_work(&reply);
}

#[test]
fn uncaught_domexception_retains_real_error_data_and_stack_after_vm_unwind() {
    // Pinned Boa may leave a newly constructed Error's stack undefined. Its
    // real ErrorData receives a backtrace when an uncaught throw unwinds the VM;
    // a fake ordinary object with Error.prototype cannot satisfy this control.
    let (realm, reply) = BoaPageRealm::start(request(
        r#"
        var owned=new DOMException('owned','DataCloneError');
        window.onerror=(message,file,line,column,error)=>{
            if(error!==owned||!(error instanceof DOMException)||!(error instanceof Error)||
                typeof error.stack!=='string'||error.name!=='DataCloneError'||error.code!==25)throw 'fake ErrorData';
            document.title='real exception stack';return true;
        };
        throw owned;
    "#,
    ));
    assert!(realm.is_some());
    assert_eq!(reply.state, RealmState::Ready, "{:?}", reply.errors);
    assert_eq!(reply.errors.len(), 1, "{:?}", reply.errors);
    assert!(reply.errors[0].starts_with("Inline script 1:"));
    assert_eq!(title(&reply), "real exception stack");
    // The source-time throw remains source-time evidence. Later clean
    // lifecycle must not erase its typed initial-failure bit.
    assert!(reply.lifecycle.unwrap().initial_failure);
    let mut realm = realm.unwrap();
    let reply = finish_load(&mut realm, reply);
    assert!(reply.lifecycle.unwrap().initial_failure);
    no_work(&reply);
}

#[test]
fn exact_frozen_native_message_fixture_creates_controls_only_in_the_later_task() {
    let (realm, init) = BoaPageRealm::start(Request {
        url: URL.into(),
        external_scripts: None,
        html: include_str!("../../../tests/fixtures/script/message-events.html").into(),
    });
    assert_eq!(init.state, RealmState::Ready, "{:?}", init.errors);
    clean(&init);
    assert!(projected(&init).forms.is_empty());
    assert_eq!(init.boa.as_ref().unwrap().pending_jobs, 2);
    let mut realm = realm.unwrap();
    let dcl = pump(&mut realm, &init);
    clean(&dcl);
    assert_eq!(
        dcl.lifecycle.unwrap().phase,
        LifecyclePhase::DomContentLoaded
    );
    assert!(projected(&dcl).forms.is_empty());
    let loaded = pump(&mut realm, &dcl);
    clean(&loaded);
    assert_eq!(
        loaded.lifecycle.unwrap().phase,
        LifecyclePhase::LoadComplete
    );
    assert!(projected(&loaded).forms.is_empty());
    assert_eq!(
        (
            loaded.boa.as_ref().unwrap().jobs_admitted,
            loaded.boa.as_ref().unwrap().jobs_executed,
            loaded.boa.as_ref().unwrap().pending_jobs
        ),
        (5, 4, 1)
    );
    let reply = pump(&mut realm, &loaded);
    clean(&reply);
    let document = projected(&reply);
    assert_eq!(document.title, "Window message controls ready");
    assert_eq!(document.forms.len(), 1);
    assert!(document.query_selector(0, "#cancel").unwrap().is_some());
    assert!(document.query_selector(0, "#query").unwrap().is_some());
    assert_eq!(reply.boa.as_ref().unwrap().jobs_admitted, 7);
    assert_eq!(reply.boa.as_ref().unwrap().jobs_executed, 7);
    no_work(&reply);
}
