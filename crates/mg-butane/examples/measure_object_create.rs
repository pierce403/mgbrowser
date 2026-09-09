// Independently authored public-API Object.create allocation measurement.
// Published successor to the frozen descriptor probe, with mandatory support.
// Build: cargo +1.91.1 build --locked --example measure_object_create
// Run: env -u RUST_MIN_STACK timeout 5s target/debug/examples/measure_object_create
// Eight fresh, identical realms; no site input, Host authority or engine internals.
// Live sizes are requested allocation sizes, NOT usable size, peak memory or RSS.
use mg_butane::runtime::{Host, Runtime, Value};
use std::alloc::{GlobalAlloc, Layout, System};
use std::mem::size_of;
use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};

struct Meter;
#[global_allocator]
static ALLOCATOR: Meter = Meter;
static LIVE_BYTES: AtomicUsize = AtomicUsize::new(0);
static LIVE_BLOCKS: AtomicUsize = AtomicUsize::new(0);
static REQUESTED_BYTES: AtomicUsize = AtomicUsize::new(0);
static REQUESTS: AtomicUsize = AtomicUsize::new(0);

fn requested(size: usize) {
    REQUESTED_BYTES.fetch_add(size, SeqCst);
    REQUESTS.fetch_add(1, SeqCst);
}
unsafe impl GlobalAlloc for Meter {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        requested(layout.size());
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            LIVE_BYTES.fetch_add(layout.size(), SeqCst);
            LIVE_BLOCKS.fetch_add(1, SeqCst);
        }
        pointer
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        requested(layout.size());
        let pointer = unsafe { System.alloc_zeroed(layout) };
        if !pointer.is_null() {
            LIVE_BYTES.fetch_add(layout.size(), SeqCst);
            LIVE_BLOCKS.fetch_add(1, SeqCst);
        }
        pointer
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) };
        LIVE_BYTES.fetch_sub(layout.size(), SeqCst);
        LIVE_BLOCKS.fetch_sub(1, SeqCst);
    }
    unsafe fn realloc(&self, pointer: *mut u8, old: Layout, new: usize) -> *mut u8 {
        // A realloc is one request for the complete new block, not its growth.
        requested(new);
        let pointer = unsafe { System.realloc(pointer, old, new) };
        if !pointer.is_null() {
            LIVE_BYTES.fetch_sub(old.size(), SeqCst);
            LIVE_BYTES.fetch_add(new, SeqCst);
        }
        pointer
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Live {
    bytes: usize,
    blocks: usize,
}
fn live() -> Live {
    Live {
        bytes: LIVE_BYTES.load(SeqCst),
        blocks: LIVE_BLOCKS.load(SeqCst),
    }
}
#[derive(Clone, Copy)]
struct Snapshot {
    live: Live,
    requested_bytes: usize,
    requests: usize,
}
fn snapshot() -> Snapshot {
    Snapshot {
        live: live(),
        requested_bytes: REQUESTED_BYTES.load(SeqCst),
        requests: REQUESTS.load(SeqCst),
    }
}

struct NoIo;
impl Host for NoIo {
    fn get(&mut self, _: &str, _: &str) -> Result<Value, String> {
        panic!("unexpected Host.get")
    }
    fn set(&mut self, _: &str, _: &str, _: Value) -> Result<(), String> {
        panic!("unexpected Host.set")
    }
    fn call(&mut self, _: &str, _: Value, _: Vec<Value>) -> Result<Value, String> {
        panic!("unexpected Host.call")
    }
}

#[derive(Clone, Copy, Debug)]
struct Measurement {
    supported: bool,
    // Signed: invoke consumes/deallocates the prebuilt native name and args Vec.
    retained_bytes: i128,
    retained_blocks: i128,
    requested_bytes: usize,
    requests: usize,
    runtime: u64,
    accepted: u64,
}

fn measure(source: &str, selection: &str, verification: &str) -> Measurement {
    let realm_base = live();
    let mut runtime = Runtime::new();
    runtime.execute(source, &mut NoIo).unwrap();
    let map = runtime.execute(selection, &mut NoIo).unwrap();
    let args = vec![Value::Null, map];
    let callable = Value::Native("Object.create".into());
    let before_report = runtime.allocation_report();
    assert!(before_report.is_valid() && before_report.first_rejected.is_none());
    let before = snapshot();

    // No parse, argument-vector construction, native-name creation or formatting
    // in this window. Real public ingress/Get/native copies still count.
    let (supported, result) = match runtime.invoke(callable, Value::Undefined, args, &mut NoIo) {
        Ok(value) => (true, Some(value)),
        Err(error) => {
            assert_eq!(
                error,
                "Uncaught JavaScript exception: Unsupported JavaScript behavior: Object.create property descriptors"
            );
            drop(error); // No diagnostic output buffer retained at endpoint.
            (false, None)
        }
    };
    let after = snapshot();
    let after_report = runtime.allocation_report();
    assert!(after_report.is_valid() && after_report.first_rejected.is_none());
    assert_eq!(after_report.limit_bytes, 4 * 1024 * 1024);
    assert!(!runtime.is_fatal());
    let mut other_phases = after_report.phases;
    other_phases.runtime = before_report.phases.runtime;
    assert_eq!(
        other_phases, before_report.phases,
        "non-Runtime phase changed"
    );
    let measured = Measurement {
        supported,
        retained_bytes: after.live.bytes as i128 - before.live.bytes as i128,
        retained_blocks: after.live.blocks as i128 - before.live.blocks as i128,
        requested_bytes: after.requested_bytes - before.requested_bytes,
        requests: after.requests - before.requests,
        runtime: after_report.phases.runtime - before_report.phases.runtime,
        accepted: after_report.accepted_bytes - before_report.accepted_bytes,
    };
    assert_eq!(measured.runtime, measured.accepted);

    // Correctness reads are explicitly outside the allocation/report window.
    if let Some(value) = result {
        assert!(matches!(value, Value::Object(_)));
        runtime.set_global("result", value);
        assert_eq!(
            runtime.execute(verification, &mut NoIo).unwrap(),
            Value::Bool(true)
        );
    }
    drop(runtime);
    assert_eq!(
        live(),
        realm_base,
        "dropping the entire realm leaked live requests"
    );
    measured
}

fn compare(
    label: &str,
    small: Measurement,
    large: Measurement,
    bytes: usize,
    blocks: usize,
    logical: u64,
) {
    assert_eq!(small.supported, large.supported);
    if small.supported {
        assert_eq!(
            large.retained_bytes - small.retained_bytes,
            bytes as i128,
            "{label}: retained payload"
        );
        assert_eq!(
            large.retained_blocks - small.retained_blocks,
            blocks as i128,
            "{label}: retained blocks"
        );
        assert_eq!(
            large.requested_bytes - small.requested_bytes,
            bytes,
            "{label}: cumulative allocator request bytes"
        );
        assert_eq!(
            large.requests - small.requests,
            blocks,
            "{label}: allocation request count"
        );
        assert_eq!(
            large.runtime - small.runtime,
            logical,
            "{label}: logical Runtime charge"
        );
        println!(
            "comparison={label} accepted=true actual_bytes={bytes} actual_blocks={blocks} logical_runtime={logical}"
        );
    } else {
        println!(
            "comparison={label} supported=false expected_old_unsupported=true candidate_assertions_not_run=true"
        );
    }
}

fn main() {
    // All maps, both callbacks, both key lengths and both UTF-16 payloads exist
    // upfront in every realm. Source and all labels also predate each meter.
    let source = format!(
        "function f(){{return 7;}}function g(value){{}}\
         var only={{x:{{get:f,set:undefined}}}},both={{x:{{get:f,set:g}}}},empty={{}},hidden=function Hidden(){{}};\
         var keyA={{}},keyB={{}},textA={{x:{{value:'{}'}}}},textB={{x:{{value:'{}'}}}};\
         keyA['{}']={{value:11}};keyB['{}']={{value:11}};",
        "\\uD800".repeat(256),
        "\\uD800".repeat(768),
        "k".repeat(64),
        "k".repeat(512)
    );
    assert!(source.len() < 8192);
    assert_eq!(size_of::<Value>(), 32, "public Value layout changed");
    // This public layout observation does not establish private Property size.
    println!(
        "arch={} public_value_size={} source_bytes={} realm_count=8 deadline_seconds=5",
        std::env::consts::ARCH,
        size_of::<Value>(),
        source.len()
    );
    let cases = [
        (
            "getter_only",
            "only;",
            "Object.getPrototypeOf(result)===null&&result.x===7;",
        ),
        (
            "getter_and_setter",
            "both;",
            "Object.getPrototypeOf(result)===null&&(result.x=9)===9&&result.x===7;",
        ),
        (
            "key_64",
            "keyA;",
            "var key=Object.getOwnPropertyNames(result)[0];key.length===64&&result[key]===11;",
        ),
        (
            "key_512",
            "keyB;",
            "var key=Object.getOwnPropertyNames(result)[0];key.length===512&&result[key]===11;",
        ),
        (
            "utf16_256",
            "textA;",
            "result.x.length===256&&result.x.charCodeAt(0)===55296;",
        ),
        (
            "utf16_768",
            "textB;",
            "result.x.length===768&&result.x.charCodeAt(0)===55296;",
        ),
        (
            "empty_map",
            "empty;",
            "Object.getPrototypeOf(result)===null&&Object.getOwnPropertyNames(result).length===0;",
        ),
        (
            "hidden_function_map",
            "hidden;",
            "Object.getPrototypeOf(result)===null&&Object.getOwnPropertyNames(result).length===0;",
        ),
    ];
    let measurements = cases.map(|(name, selection, verification)| {
        let measured = measure(&source, selection, verification);
        println!("case={name} measurement={measured:?} drop_restored=true");
        measured
    });
    let supported = measurements[0].supported;
    assert!(measurements.iter().all(|case| case.supported == supported));
    assert!(
        supported,
        "Object.create descriptors are required by this probe"
    );
    compare("setter_pair", measurements[0], measurements[1], 64, 1, 80);
    compare(
        "owned_ascii_key",
        measurements[2],
        measurements[3],
        512 - 64,
        0,
        448,
    );
    compare(
        "one_utf16_value_get",
        measurements[4],
        measurements[5],
        2 * (768 - 256),
        0,
        1024,
    );
    // Empty versus hidden maps is observation only: no guessed private key/row
    // sizes. Function metadata is nonenumerable and must not be invoked/read.
    println!(
        "control=empty_vs_hidden empty={:?} hidden={:?} private_layout_not_inferred=true",
        measurements[6], measurements[7]
    );
    println!("probe_complete=true supported={supported} all_realms_dropped=true");
}
