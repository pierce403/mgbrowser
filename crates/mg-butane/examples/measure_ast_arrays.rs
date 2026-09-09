// Published successor to the independently frozen array parser allocation probe.
// Adds only the mandatory exact-capacity guard below, comments and formatting.
// Suggested execution: env -u RUST_MIN_STACK timeout 20s PATH_TO_PROBE
// Twelve fixed authored cases, no input files/network/runtime execution. Source
// creation and correctness checks are OUTSIDE parse/clone/drop meter windows.
// The 10,000-element corpus bound is NOT a new parser cap. Timing is one noisy,
// instrumented observation per stage, never a performance acceptance threshold.
// Requested/live System bytes are not allocator usable sizes, metadata, or RSS.
// Observed peaks are allocator-call boundaries; realloc's internal moving peak
// is unknowable here. The separate conservative bound permits old+new overlap.
use mg_butane::{Expr, Program, Stmt, syntax};
use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::mem::{align_of, size_of};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::SeqCst};
use std::time::Instant;

struct Meter;
#[global_allocator]
static ALLOCATOR: Meter = Meter;
static ACTIVE: AtomicBool = AtomicBool::new(false);
static LIVE_BYTES: AtomicUsize = AtomicUsize::new(0);
static LIVE_BLOCKS: AtomicUsize = AtomicUsize::new(0);
static ALLOC_CALLS: AtomicUsize = AtomicUsize::new(0);
static ZERO_CALLS: AtomicUsize = AtomicUsize::new(0);
static REALLOC_CALLS: AtomicUsize = AtomicUsize::new(0);
static DEALLOC_CALLS: AtomicUsize = AtomicUsize::new(0);
static FAILED_CALLS: AtomicUsize = AtomicUsize::new(0);
static REQUEST_BYTES: AtomicUsize = AtomicUsize::new(0);
static OBSERVED_PEAK_BYTES: AtomicUsize = AtomicUsize::new(0);
static OBSERVED_PEAK_BLOCKS: AtomicUsize = AtomicUsize::new(0);
static POTENTIAL_PEAK_BYTES: AtomicUsize = AtomicUsize::new(0);
static POTENTIAL_PEAK_BLOCKS: AtomicUsize = AtomicUsize::new(0);

fn request(bytes: usize) {
    REQUEST_BYTES.fetch_add(bytes, SeqCst);
    // A realloc request includes the complete requested NEW block, not just
    // growth. LIVE still includes its old block, even if resizing is in place.
    POTENTIAL_PEAK_BYTES.fetch_max(LIVE_BYTES.load(SeqCst) + bytes, SeqCst);
    POTENTIAL_PEAK_BLOCKS.fetch_max(LIVE_BLOCKS.load(SeqCst) + 1, SeqCst);
}
fn observed() {
    OBSERVED_PEAK_BYTES.fetch_max(LIVE_BYTES.load(SeqCst), SeqCst);
    OBSERVED_PEAK_BLOCKS.fetch_max(LIVE_BLOCKS.load(SeqCst), SeqCst);
}
unsafe impl GlobalAlloc for Meter {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let active = ACTIVE.load(SeqCst);
        if active {
            ALLOC_CALLS.fetch_add(1, SeqCst);
            request(layout.size());
        }
        let pointer = unsafe { System.alloc(layout) };
        if pointer.is_null() {
            if active {
                FAILED_CALLS.fetch_add(1, SeqCst);
            }
        } else {
            LIVE_BYTES.fetch_add(layout.size(), SeqCst);
            LIVE_BLOCKS.fetch_add(1, SeqCst);
            if active {
                observed();
            }
        }
        pointer
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let active = ACTIVE.load(SeqCst);
        if active {
            ZERO_CALLS.fetch_add(1, SeqCst);
            request(layout.size());
        }
        let pointer = unsafe { System.alloc_zeroed(layout) };
        if pointer.is_null() {
            if active {
                FAILED_CALLS.fetch_add(1, SeqCst);
            }
        } else {
            LIVE_BYTES.fetch_add(layout.size(), SeqCst);
            LIVE_BLOCKS.fetch_add(1, SeqCst);
            if active {
                observed();
            }
        }
        pointer
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        if ACTIVE.load(SeqCst) {
            DEALLOC_CALLS.fetch_add(1, SeqCst);
        }
        unsafe { System.dealloc(pointer, layout) };
        LIVE_BYTES.fetch_sub(layout.size(), SeqCst);
        LIVE_BLOCKS.fetch_sub(1, SeqCst);
    }
    unsafe fn realloc(&self, pointer: *mut u8, old: Layout, new: usize) -> *mut u8 {
        let active = ACTIVE.load(SeqCst);
        if active {
            REALLOC_CALLS.fetch_add(1, SeqCst);
            request(new);
        }
        let pointer = unsafe { System.realloc(pointer, old, new) };
        if pointer.is_null() {
            // Failed realloc keeps the old block. No intentional OOM is tested;
            // a Rust allocation abort cannot yield a complete drop receipt.
            if active {
                FAILED_CALLS.fetch_add(1, SeqCst);
            }
        } else {
            LIVE_BYTES.fetch_sub(old.size(), SeqCst);
            LIVE_BYTES.fetch_add(new, SeqCst);
            if active {
                observed();
            }
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
fn begin() -> Live {
    assert!(!ACTIVE.load(SeqCst), "nested meter window");
    for counter in [
        &ALLOC_CALLS,
        &ZERO_CALLS,
        &REALLOC_CALLS,
        &DEALLOC_CALLS,
        &FAILED_CALLS,
        &REQUEST_BYTES,
    ] {
        counter.store(0, SeqCst);
    }
    let before = live();
    OBSERVED_PEAK_BYTES.store(before.bytes, SeqCst);
    OBSERVED_PEAK_BLOCKS.store(before.blocks, SeqCst);
    POTENTIAL_PEAK_BYTES.store(before.bytes, SeqCst);
    POTENTIAL_PEAK_BLOCKS.store(before.blocks, SeqCst);
    ACTIVE.store(true, SeqCst);
    before
}
#[derive(Debug)]
struct Window {
    before: Live,
    after: Live,
    allocations: usize,
    zeroed: usize,
    reallocations: usize,
    deallocations: usize,
    failed: usize,
    requested: usize,
    observed_peak: Live,
    potential_peak: Live,
    elapsed_ns: u128,
}
fn measure<T>(operation: impl FnOnce() -> T) -> (T, Window) {
    let before = begin();
    let start = Instant::now();
    let result = operation();
    let elapsed_ns = start.elapsed().as_nanos();
    assert!(ACTIVE.swap(false, SeqCst), "meter was not active");
    let window = Window {
        before,
        after: live(),
        elapsed_ns,
        allocations: ALLOC_CALLS.load(SeqCst),
        zeroed: ZERO_CALLS.load(SeqCst),
        reallocations: REALLOC_CALLS.load(SeqCst),
        deallocations: DEALLOC_CALLS.load(SeqCst),
        failed: FAILED_CALLS.load(SeqCst),
        requested: REQUEST_BYTES.load(SeqCst),
        observed_peak: Live {
            bytes: OBSERVED_PEAK_BYTES.load(SeqCst),
            blocks: OBSERVED_PEAK_BLOCKS.load(SeqCst),
        },
        potential_peak: Live {
            bytes: POTENTIAL_PEAK_BYTES.load(SeqCst),
            blocks: POTENTIAL_PEAK_BLOCKS.load(SeqCst),
        },
    };
    assert_eq!(window.failed, 0, "System allocation failed");
    (result, window)
}
fn print_window(stage: &str, w: &Window) {
    println!(
        "  stage={stage} live_before={:?} live_after={:?} live_delta_bytes={} live_delta_blocks={} requested={} alloc={} zeroed={} realloc={} dealloc={} failed={} observed_peak_extra_bytes={} observed_peak_extra_blocks={} conservative_overlap_extra_bytes={} conservative_overlap_extra_blocks={} elapsed_ns={}",
        w.before,
        w.after,
        w.after.bytes as i128 - w.before.bytes as i128,
        w.after.blocks as i128 - w.before.blocks as i128,
        w.requested,
        w.allocations,
        w.zeroed,
        w.reallocations,
        w.deallocations,
        w.failed,
        w.observed_peak.bytes - w.before.bytes,
        w.observed_peak.blocks - w.before.blocks,
        w.potential_peak.bytes - w.before.bytes,
        w.potential_peak.blocks - w.before.blocks,
        w.elapsed_ns
    );
}
fn dropped(w: &Window) {
    assert_eq!(
        (w.requested, w.allocations, w.zeroed, w.reallocations),
        (0, 0, 0, 0),
        "dropping these authored AST/error values unexpectedly allocated"
    );
}

#[derive(Clone, Copy)]
enum Case {
    Empty,
    EmptyArray,
    Tiny,
    Malformed,
    Holes(usize),
    Dense,
    Utf16,
    Nested,
    SharedDecl,
    SharedExpr,
}
const CASES: [(&str, Case); 12] = [
    ("empty-program", Case::Empty),
    ("empty-array", Case::EmptyArray),
    ("tiny-10000", Case::Tiny),
    ("tiny-10000-late-malformed", Case::Malformed),
    ("holes-4096", Case::Holes(4096)),
    ("holes-4097", Case::Holes(4097)),
    ("holes-10000", Case::Holes(10000)),
    ("dense-4097", Case::Dense),
    ("utf16-holes-1026", Case::Utf16),
    ("nested-256", Case::Nested),
    ("shared-declaration", Case::SharedDecl),
    ("shared-expression-holes-4097", Case::SharedExpr),
];
fn hole_source(length: usize) -> String {
    format!("[{}];", ",".repeat(length))
}
fn source(case: Case) -> String {
    match case {
        Case::Empty => String::new(),
        Case::EmptyArray => "[];".into(),
        Case::Tiny => "[0];".repeat(10_000),
        // The error occurs AFTER all 10,000 array statements have completed;
        // the parser must release both retained partial program and frames.
        Case::Malformed => "[0];".repeat(10_000) + "[",
        Case::Holes(n) => hole_source(n),
        Case::Dense => {
            use std::fmt::Write;
            let mut text = String::from("[");
            for n in 0..4097 {
                write!(text, "{n},").unwrap();
            }
            text.push_str("]; ");
            text
        }
        Case::Utf16 => format!("[{}];", "'\\uD800\\u0000\\uDFFFé',,".repeat(513)),
        Case::Nested => format!("[{}];", "[0,,'\\uD800é'],".repeat(256)),
        Case::SharedDecl => {
            "function outer(p){['\\uD800é'];return function inner(q){return [p,q,[0,,],,];};}"
                .into()
        }
        Case::SharedExpr => format!("(function kept(p){{return {}}});", hole_source(4097)),
    }
}
fn array(expr: &Expr) -> &[Option<Expr>] {
    let Expr::Array(values) = expr else {
        panic!("expected authored array");
    };
    values
}
fn expression(stmt: &Stmt) -> &Expr {
    let Stmt::Expr(expr) = stmt else {
        panic!("expected expression statement");
    };
    expr
}
fn returned(stmt: &Stmt) -> &Expr {
    let Stmt::Return(Some(expr)) = stmt else {
        panic!("expected authored return");
    };
    expr
}
fn number(expr: &Expr, expected: f64) {
    let Expr::Number(value) = expr else {
        panic!("expected authored number");
    };
    assert_eq!(value.to_bits(), expected.to_bits());
}
fn text(expr: &Expr, expected: &[u16]) {
    let Expr::String(value) = expr else {
        panic!("expected owned UTF16");
    };
    assert_eq!(value.as_slice(), expected);
}
fn holes(expr: &Expr, count: usize) {
    let values = array(expr);
    assert_eq!(values.len(), count);
    assert!(
        values.iter().all(Option::is_none),
        "hole became an expression"
    );
}
fn validate(program: &Program, case: Case) {
    if matches!(case, Case::Empty) {
        assert!(program.0.is_empty());
        return;
    }
    if matches!(case, Case::Tiny) {
        assert_eq!(program.0.len(), 10_000);
        for stmt in &program.0 {
            let values = array(expression(stmt));
            assert_eq!(values.len(), 1);
            number(values[0].as_ref().unwrap(), 0.0);
        }
        return;
    }
    assert_eq!(program.0.len(), 1);
    match case {
        Case::EmptyArray => holes(expression(&program.0[0]), 0),
        Case::Holes(n) => holes(expression(&program.0[0]), n),
        Case::Dense => {
            let values = array(expression(&program.0[0]));
            assert_eq!(values.len(), 4097);
            for (n, value) in values.iter().enumerate() {
                number(value.as_ref().unwrap(), n as f64);
            }
        }
        Case::Utf16 => {
            let values = array(expression(&program.0[0]));
            assert_eq!(values.len(), 1026);
            for pair in values.chunks_exact(2) {
                text(pair[0].as_ref().unwrap(), &[0xd800, 0, 0xdfff, 0xe9]);
                assert!(pair[1].is_none());
            }
        }
        Case::Nested => {
            let values = array(expression(&program.0[0]));
            assert_eq!(values.len(), 256);
            for value in values {
                let inner = array(value.as_ref().unwrap());
                assert_eq!(inner.len(), 3);
                number(inner[0].as_ref().unwrap(), 0.0);
                assert!(inner[1].is_none());
                text(inner[2].as_ref().unwrap(), &[0xd800, 0xe9]);
            }
        }
        Case::SharedDecl => {
            let Stmt::Function { name, params, body } = &program.0[0] else {
                panic!("expected declaration");
            };
            assert_eq!(name, "outer");
            assert_eq!(&**params, ["p"]);
            assert_eq!(body.len(), 2);
            let first = array(expression(&body[0]));
            assert_eq!(first.len(), 1);
            text(first[0].as_ref().unwrap(), &[0xd800, 0xe9]);
            let Expr::Function { name, params, body } = returned(&body[1]) else {
                panic!("expected inner function");
            };
            assert_eq!(name.as_deref(), Some("inner"));
            assert_eq!(&**params, ["q"]);
            assert_eq!(body.len(), 1);
            let values = array(returned(&body[0]));
            assert_eq!(values.len(), 4);
            assert!(matches!(values[0].as_ref(), Some(Expr::Ident(s)) if s == "p"));
            assert!(matches!(values[1].as_ref(), Some(Expr::Ident(s)) if s == "q"));
            let inner = array(values[2].as_ref().unwrap());
            assert_eq!(inner.len(), 2);
            number(inner[0].as_ref().unwrap(), 0.0);
            assert!(inner[1].is_none());
            assert!(values[3].is_none());
        }
        Case::SharedExpr => {
            let Expr::Function { name, params, body } = expression(&program.0[0]) else {
                panic!("expected function expression");
            };
            assert_eq!(name.as_deref(), Some("kept"));
            assert_eq!(&**params, ["p"]);
            assert_eq!(body.len(), 1);
            holes(returned(&body[0]), 4097);
        }
        Case::Empty | Case::Tiny | Case::Malformed => panic!("invalid validation route"),
    }
}

#[derive(Default, Debug)]
struct Shape {
    arrays: usize,
    slots: usize,
    capacity_slots: usize,
    strings: usize,
    utf16_units: usize,
    functions: usize,
}
fn shape_stmt(stmt: &Stmt, out: &mut Shape) {
    match stmt {
        Stmt::Expr(e) | Stmt::Return(Some(e)) => shape_expr(e, out),
        Stmt::Function { body, .. } => {
            out.functions += 1;
            for s in &**body {
                shape_stmt(s, out);
            }
        }
        _ => panic!("unlisted authored statement"),
    }
}
fn shape_expr(expr: &Expr, out: &mut Shape) {
    match expr {
        Expr::Array(values) => {
            out.arrays += 1;
            out.slots += values.len();
            out.capacity_slots += values.capacity();
            for e in values.iter().flatten() {
                shape_expr(e, out);
            }
        }
        Expr::String(value) => {
            out.strings += 1;
            out.utf16_units += value.len();
        }
        Expr::Function { body, .. } => {
            out.functions += 1;
            for s in &**body {
                shape_stmt(s, out);
            }
        }
        Expr::Number(_) | Expr::Ident(_) => (),
        _ => panic!("unlisted authored expression"),
    }
}
fn shape(program: &Program) -> Shape {
    let mut out = Shape::default();
    for stmt in &program.0 {
        shape_stmt(stmt, &mut out);
    }
    out
}
fn clone_expr(a: &Expr, b: &Expr) {
    match (a, b) {
        (Expr::Array(a), Expr::Array(b)) => {
            if !a.is_empty() {
                assert_ne!(a.as_ptr(), b.as_ptr(), "owned array clone aliased");
            }
            for (a, b) in a.iter().zip(b) {
                if let (Some(a), Some(b)) = (a, b) {
                    clone_expr(a, b);
                }
            }
        }
        (Expr::String(a), Expr::String(b)) => {
            assert_ne!(a.as_ptr(), b.as_ptr(), "owned string clone aliased")
        }
        (
            Expr::Function {
                params: ap,
                body: ab,
                ..
            },
            Expr::Function {
                params: bp,
                body: bb,
                ..
            },
        ) => {
            assert!(
                Rc::ptr_eq(ap, bp) && Rc::ptr_eq(ab, bb),
                "function code cloned instead of shared"
            );
        }
        (Expr::Number(_), Expr::Number(_)) | (Expr::Ident(_), Expr::Ident(_)) => (),
        _ => panic!("clone shape mismatch"),
    }
}
fn clone_identity(a: &Program, b: &Program) {
    assert!(a == b, "clone changed semantic AST");
    if !a.0.is_empty() {
        assert_ne!(a.0.as_ptr(), b.0.as_ptr(), "program slots aliased");
    }
    for (a, b) in a.0.iter().zip(&b.0) {
        match (a, b) {
            (Stmt::Expr(a), Stmt::Expr(b)) => clone_expr(a, b),
            (
                Stmt::Function {
                    params: ap,
                    body: ab,
                    ..
                },
                Stmt::Function {
                    params: bp,
                    body: bb,
                    ..
                },
            ) => {
                assert!(
                    Rc::ptr_eq(ap, bp) && Rc::ptr_eq(ab, bb),
                    "declaration code cloned instead of shared"
                );
            }
            _ => panic!("unlisted clone statement"),
        }
    }
}
fn run(name: &str, case: Case) {
    let baseline = live();
    let source = source(case); // All construction and growth outside windows.
    let source_bytes = source.len();
    let source_capacity = source.capacity();
    assert!(
        source_bytes < 1024 * 1024,
        "authored source unexpectedly unbounded"
    );
    let (parsed, parse_window) = measure(|| syntax::parse(black_box(source.as_str())));
    match parsed {
        Err(error) => {
            assert!(
                matches!(case, Case::Malformed),
                "unexpected parse failure: {error}"
            );
            assert_eq!(
                error,
                format!("Unexpected end of source; expected expression at byte {source_bytes}")
            );
            let error_bytes = error.len();
            drop(source); // Caller source is not charged to the error-drop stage.
            let ((), error_drop) = measure(|| drop(black_box(error)));
            dropped(&error_drop);
            assert_eq!(
                live(),
                baseline,
                "failed parse leaked partial trees or error storage"
            );
            println!(
                "CASE {name} expected_error=true source_bytes={source_bytes} source_capacity={source_capacity} retained_error_bytes={error_bytes} drop_restored=true"
            );
            print_window("parse-error", &parse_window);
            print_window("drop-error", &error_drop);
        }
        Ok(program) => {
            assert!(
                !matches!(case, Case::Malformed),
                "malformed suffix unexpectedly parsed"
            );
            validate(&program, case);
            let original_shape = shape(&program);
            let program_capacity = program.0.capacity();
            assert_eq!(
                original_shape.capacity_slots, original_shape.slots,
                "completed parser arrays must not retain geometric builder slack"
            );
            let (cloned, clone_window) = measure(|| black_box(&program).clone());
            clone_identity(&program, &cloned);
            let cloned_shape = shape(&cloned);
            drop(source);
            // Both ASTs outlive source. Cloned owned vectors are distinct; Rc
            // function code must remain readable after the original is dropped.
            validate(&program, case);
            validate(&cloned, case);
            let ((), original_drop) = measure(|| drop(black_box(program)));
            dropped(&original_drop);
            validate(&cloned, case);
            let ((), clone_drop) = measure(|| drop(black_box(cloned)));
            dropped(&clone_drop);
            assert_eq!(
                live(),
                baseline,
                "parse/clone/drop did not restore requested-live baseline"
            );
            println!(
                "CASE {name} expected_error=false source_bytes={source_bytes} source_capacity={source_capacity} program_capacity={program_capacity} original_shape={original_shape:?} cloned_shape={cloned_shape:?} drop_restored=true"
            );
            print_window("parse", &parse_window);
            print_window("clone", &clone_window);
            print_window("drop-original", &original_drop);
            print_window("drop-clone", &clone_drop);
        }
    }
}
fn main() {
    // These are measured baseline ABI gates on the pinned target, not claims
    // that private layouts or allocator RSS can be inferred from public types.
    assert_eq!(
        (
            size_of::<Expr>(),
            size_of::<Option<Expr>>(),
            size_of::<Stmt>(),
            size_of::<Vec<Option<Expr>>>(),
            size_of::<Program>()
        ),
        (56, 56, 80, 24, 24)
    );
    let _clock_warmup = Instant::now();
    println!(
        "AST_PARSE_LAYOUT Expr={} OptionExpr={} Stmt={} Program={} alignExpr={} alignStmt={} cases=12 timing=observational instrumented=true",
        size_of::<Expr>(),
        size_of::<Option<Expr>>(),
        size_of::<Stmt>(),
        size_of::<Program>(),
        align_of::<Expr>(),
        align_of::<Stmt>()
    );
    println!(
        "PEAK_POLICY observed=allocator-call-boundaries conservative=realloc-old-plus-new-request not_RSS=true source_build_outside_windows=true"
    );
    for (name, case) in CASES {
        run(name, case);
    }
    println!(
        "COMPLETE cases=12 successful_parses=11 expected_errors=1 clone_checks=11 full_drop_restorations=12 timing_thresholds=none"
    );
}
