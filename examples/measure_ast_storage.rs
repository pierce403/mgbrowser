// Independently authored, single-threaded retained-allocation diagnostic.
// Published formatting-only successor to the frozen static-operators probe.
// Run explicitly; the ordinary example test harness does not run main().
// Compile unchanged against the frozen String-operator library and candidate.
// Sources exist before each window; no website input or alternative evaluator.
// Requested retained sizes and live blocks are NOT allocator RSS evidence.
use mg_deps::js::runtime::{Host, Runtime, Value};
use mg_deps::js::{Expr, ForInBinding, Program, Stmt, SwitchCase, syntax};
use std::alloc::{GlobalAlloc, Layout, System};
use std::mem::size_of;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};

struct Meter;
static BYTES: AtomicUsize = AtomicUsize::new(0);
static BLOCKS: AtomicUsize = AtomicUsize::new(0);
#[global_allocator]
static ALLOCATOR: Meter = Meter;
unsafe impl GlobalAlloc for Meter {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let result = unsafe { System.alloc(layout) };
        if !result.is_null() {
            BYTES.fetch_add(layout.size(), SeqCst);
            BLOCKS.fetch_add(1, SeqCst);
        }
        result
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let result = unsafe { System.alloc_zeroed(layout) };
        if !result.is_null() {
            BYTES.fetch_add(layout.size(), SeqCst);
            BLOCKS.fetch_add(1, SeqCst);
        }
        result
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) };
        BYTES.fetch_sub(layout.size(), SeqCst);
        BLOCKS.fetch_sub(1, SeqCst);
    }
    unsafe fn realloc(&self, pointer: *mut u8, old: Layout, new: usize) -> *mut u8 {
        let result = unsafe { System.realloc(pointer, old, new) };
        if !result.is_null() {
            BYTES.fetch_sub(old.size(), SeqCst);
            BYTES.fetch_add(new, SeqCst);
        }
        result
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Snapshot {
    bytes: usize,
    blocks: usize,
}
fn snapshot() -> Snapshot {
    Snapshot {
        bytes: BYTES.load(SeqCst),
        blocks: BLOCKS.load(SeqCst),
    }
}
fn delta(after: Snapshot, before: Snapshot) -> Snapshot {
    Snapshot {
        bytes: after.bytes.checked_sub(before.bytes).unwrap(),
        blocks: after.blocks.checked_sub(before.blocks).unwrap(),
    }
}

const SPELLINGS: &[&str] = &[
    "+",
    "-",
    "!",
    "~",
    "typeof",
    "void",
    "delete",
    "++",
    "--",
    "=",
    "+=",
    "-=",
    "*=",
    "/=",
    "%=",
    "<<=",
    ">>=",
    ">>>=",
    "&=",
    "|=",
    "^=",
    "||",
    "&&",
    "|",
    "^",
    "&",
    "==",
    "!=",
    "===",
    "!==",
    "<",
    ">",
    "<=",
    ">=",
    "instanceof",
    "in",
    "<<",
    ">>",
    ">>>",
    "*",
    "/",
    "%",
];

#[derive(Default, Debug, PartialEq, Eq)]
struct Inventory {
    unary: usize,
    binary: usize,
    assignment: usize,
    update: usize,
    canonical_operator_bytes: usize,
    statement_slots: usize,
    boxed_expressions: usize,
    array_slots: usize,
    holes: usize,
    owned_text_units: usize,
    // A fixed, allocation-free histogram; payload is spelling length only,
    // deliberately valid for both String and &'static str operator fields.
    histogram: Histogram,
}
#[derive(Debug, PartialEq, Eq)]
struct Histogram([usize; SPELLINGS.len()]);
impl Default for Histogram {
    fn default() -> Self {
        Self([0; SPELLINGS.len()])
    }
}
impl Inventory {
    fn operator(&mut self, spelling: &str) {
        self.canonical_operator_bytes += spelling.len();
        let index = SPELLINGS
            .iter()
            .position(|candidate| *candidate == spelling)
            .unwrap();
        self.histogram.0[index] += 1;
    }
    fn list(&mut self, values: &[Stmt], slots: usize) {
        self.statement_slots += slots;
        for value in values {
            self.statement(value);
        }
    }
    fn boxed_statement(&mut self, value: &Stmt) {
        self.statement_slots += 1;
        self.statement(value);
    }
    fn optional(&mut self, value: Option<&Expr>) {
        if let Some(value) = value {
            self.expression(value);
        }
    }
    fn boxed_expression(&mut self, value: &Expr) {
        self.boxed_expressions += 1;
        self.expression(value);
    }
    fn statement(&mut self, value: &Stmt) {
        match value {
            Stmt::Empty => {}
            Stmt::Break(label) | Stmt::Continue(label) => {
                self.owned_text_units += label.as_ref().map_or(0, String::len);
            }
            Stmt::Expr(value) | Stmt::Throw(value) => self.expression(value),
            Stmt::Return(value) => self.optional(value.as_ref()),
            Stmt::Block(body) => self.list(body, body.capacity()),
            Stmt::Var(bindings) => {
                for (name, value) in bindings {
                    self.owned_text_units += name.len();
                    self.optional(value.as_ref());
                }
            }
            Stmt::Function { name, params, body } => {
                self.owned_text_units += name.len() + params.iter().map(String::len).sum::<usize>();
                self.list(body, body.len());
            }
            Stmt::If {
                test,
                consequent,
                alternate,
            } => {
                self.expression(test);
                self.boxed_statement(consequent);
                if let Some(alternate) = alternate {
                    self.boxed_statement(alternate);
                }
            }
            Stmt::While { test, body } | Stmt::DoWhile { test, body } => {
                self.expression(test);
                self.boxed_statement(body);
            }
            Stmt::For {
                init,
                test,
                update,
                body,
            } => {
                if let Some(init) = init {
                    self.boxed_statement(init);
                }
                if let Some(test) = test {
                    self.boxed_expression(test);
                }
                if let Some(update) = update {
                    self.boxed_expression(update);
                }
                self.boxed_statement(body);
            }
            Stmt::ForIn {
                binding,
                object,
                body,
            } => {
                match binding.as_ref() {
                    ForInBinding::Var { name, init } => {
                        self.owned_text_units += name.len();
                        self.optional(init.as_ref());
                    }
                    ForInBinding::Reference(value) => self.expression(value),
                }
                self.expression(object);
                self.boxed_statement(body);
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                self.expression(discriminant);
                for case in cases {
                    self.optional(case.test.as_ref());
                    self.list(&case.body, case.body.capacity());
                }
            }
            Stmt::Label { name, body } => {
                self.owned_text_units += name.len();
                self.boxed_statement(body);
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                self.boxed_statement(body);
                if let Some((name, body)) = catch {
                    self.owned_text_units += name.len();
                    self.boxed_statement(body);
                }
                if let Some(body) = finally {
                    self.boxed_statement(body);
                }
            }
        }
    }
    fn expression(&mut self, value: &Expr) {
        match value {
            Expr::Undefined | Expr::Null | Expr::Bool(_) | Expr::Number(_) | Expr::This => {}
            Expr::Ident(name) => self.owned_text_units += name.len(),
            Expr::String(units) => self.owned_text_units += units.len(),
            Expr::RegExp { pattern, flags } => self.owned_text_units += pattern.len() + flags.len(),
            Expr::Array(items) => {
                self.array_slots += items.capacity();
                for item in items {
                    if let Some(value) = item {
                        self.expression(value);
                    } else {
                        self.holes += 1;
                    }
                }
            }
            Expr::Object(items) => {
                for (name, value) in items {
                    self.owned_text_units += name.len();
                    self.expression(value);
                }
            }
            Expr::Function { name, params, body } => {
                self.owned_text_units += name.as_ref().map_or(0, String::len)
                    + params.iter().map(String::len).sum::<usize>();
                self.list(body, body.len());
            }
            Expr::Unary { op, expr } => {
                self.unary += 1;
                self.operator(op);
                self.boxed_expression(expr);
            }
            Expr::Update { op, expr, .. } => {
                self.update += 1;
                self.operator(op);
                self.boxed_expression(expr);
            }
            Expr::Binary { op, left, right } => {
                self.binary += 1;
                self.operator(op);
                self.boxed_expression(left);
                self.boxed_expression(right);
            }
            Expr::Assign { op, left, right } => {
                self.assignment += 1;
                self.operator(op);
                self.boxed_expression(left);
                self.boxed_expression(right);
            }
            Expr::Conditional {
                test,
                consequent,
                alternate,
            } => {
                self.boxed_expression(test);
                self.boxed_expression(consequent);
                self.boxed_expression(alternate);
            }
            Expr::Sequence(items) => {
                for item in items {
                    self.expression(item);
                }
            }
            Expr::Member { object, property } => {
                self.boxed_expression(object);
                self.boxed_expression(property);
            }
            Expr::Call { callee, args } | Expr::New { callee, args } => {
                self.boxed_expression(callee);
                for value in args {
                    self.expression(value);
                }
            }
        }
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

fn measure(name: &str, source: String) {
    measure_inner(name, source, true);
}
fn parse_only(name: &str, source: &str) {
    measure_inner(name, source.into(), false);
}
fn measure_inner(name: &str, source: String, execute: bool) {
    let before = snapshot();
    let program: Program = syntax::parse(&source).unwrap();
    let parsed = snapshot();
    let mut inventory = Inventory::default();
    inventory.list(&program.0, program.0.capacity());
    let cloned = program.clone();
    let after_clone = snapshot();
    assert_eq!(program, cloned);
    drop(program);
    let surviving_clone = snapshot();
    let mut clone_inventory = Inventory::default();
    clone_inventory.list(&cloned.0, cloned.0.capacity());
    // Vec::clone retains length rather than source spare capacity, so compare
    // semantic operator and owned text counts, not allocated slot counts.
    assert_eq!(
        (
            inventory.unary,
            inventory.binary,
            inventory.assignment,
            inventory.update,
            inventory.canonical_operator_bytes,
            inventory.owned_text_units
        ),
        (
            clone_inventory.unary,
            clone_inventory.binary,
            clone_inventory.assignment,
            clone_inventory.update,
            clone_inventory.canonical_operator_bytes,
            clone_inventory.owned_text_units
        )
    );
    assert_eq!(inventory.histogram, clone_inventory.histogram);
    drop(cloned);
    assert_eq!(
        snapshot(),
        before,
        "final AST owner did not restore allocator snapshot"
    );
    let retained = delta(parsed, before);
    let copied = delta(after_clone, parsed);
    let survivor = delta(surviving_clone, before);
    let report = if execute {
        let mut runtime = Runtime::new();
        runtime.execute(&source, &mut NoIo).unwrap();
        let report = runtime.allocation_report();
        assert!(report.is_valid() && report.first_rejected.is_none());
        assert_eq!(
            report.phases.ast,
            (retained.bytes + 16 * retained.blocks) as u64,
            "logical AST storage disagrees with independent retained allocator requests"
        );
        Some(report)
    } else {
        None
    };
    println!(
        "case={name} source={} retained_bytes={} retained_blocks={} clone_bytes={} clone_blocks={} surviving_clone_bytes={} surviving_clone_blocks={} measured_logical_ast={} report={report:?} inventory={inventory:?} drop_restored=true",
        source.len(),
        retained.bytes,
        retained.blocks,
        copied.bytes,
        copied.blocks,
        survivor.bytes,
        survivor.blocks,
        retained.bytes + 16 * retained.blocks
    );
}

fn main() {
    println!(
        "arch={} Expr={} Stmt={} OptionExpr={} ForInBinding={} SwitchCase={} VarTuple={} ObjectTuple={} RcStmt={} RcString={}",
        std::env::consts::ARCH,
        size_of::<Expr>(),
        size_of::<Stmt>(),
        size_of::<Option<Expr>>(),
        size_of::<ForInBinding>(),
        size_of::<SwitchCase>(),
        size_of::<(String, Option<Expr>)>(),
        size_of::<(String, Expr)>(),
        size_of::<Rc<[Stmt]>>(),
        size_of::<Rc<[String]>>()
    );
    println!("histogram_spellings={SPELLINGS:?}");
    measure("flat_no_operators_4096", "0;".repeat(4096));
    measure("flat_binary_4096", "0+0;".repeat(4096));
    measure(
        "retained_no_operators_20000",
        format!("function kept(){{{}}}", "0;".repeat(20000)),
    );
    measure(
        "retained_binary_14500",
        format!("function kept(){{{}}}", "0+0;".repeat(14500)),
    );
    measure(
        "hole_array_10000",
        format!("function kept(){{return [{}];}}", ",".repeat(10000)),
    );
    measure(
        "number_array_4096",
        format!("function kept(){{return [{}];}}", "0,".repeat(4096)),
    );
    measure(
        "number_array_4097",
        format!("function kept(){{return [{}];}}", "0,".repeat(4097)),
    );
    measure(
        "empty_for_256",
        format!("function kept(){{{}}}", "for(;;);".repeat(256)),
    );
    measure(
        "ordinary_for_256",
        format!(
            "function kept(){{{}}}",
            "for(var i=0;i<1;i++){}".repeat(256)
        ),
    );
    measure(
        "for_in_256",
        format!(
            "function kept(){{{}}}",
            "for(var i in source)break;".repeat(256)
        ),
    );
    measure("all_operator_families", "function kept(a,b,o,C){+a;-a;!a;~a;typeof a;void a;delete o.x;++a;--a;a++;a--;a=b;a+=b;a-=b;a*=b;a/=b;a%=b;a<<=b;a>>=b;a>>>=b;a&=b;a|=b;a^=b;a||b;a&&b;a|b;a^b;a&b;a==b;a!=b;a===b;a!==b;a<b;a>b;a<=b;a>=b;a instanceof C;a in o;a<<b;a>>b;a>>>b;a+b;a-b;a*b;a/b;a%b;}".into());
    measure("owned_names_text_and_regex", "function retainedNamed(longParameter){var retainedObject={longProperty:'owned UTF16 text'};return /owned+/g;}".into());
    // Exact frozen public test sources. Parse/clone/drop only: no Host calls,
    // exceptions or intentionally endless fuel workload execute in this probe.
    parse_only(
        "diagnostic_CAUGHT",
        "var rounds=0;for(var i=0;i<128;i++){try{null.length;}catch(e){rounds++;}}rounds;",
    );
    parse_only(
        "diagnostic_FINALLY",
        "var prior=0;try{null.length;}finally{prior=7;}",
    );
    parse_only(
        "diagnostic_FUEL",
        "var rounds=0,ticks=0;for(var i=0;i<128;i++){try{null.length;}catch(e){rounds++;}}while(true){ticks++;}",
    );
    parse_only(
        "producer_CAUGHT",
        "var box={length:undefined},rounds=0,last='';for(var i=0;i<64;i++){try{box.length.name;}catch(e){last=e;rounds++;}}rounds;",
    );
    parse_only(
        "producer_MISSING",
        "var box={},rounds=0,last='';for(var i=0;i<64;i++){try{box.length.name;}catch(e){last=e;rounds++;}}rounds;",
    );
    parse_only(
        "producer_FINALLY",
        "var box={},prior=0;try{box.length.name;}finally{prior=7;}",
    );
    parse_only(
        "producer_FUEL",
        "var box={},rounds=0,ticks=0;for(var i=0;i<64;i++){try{box.length.name;}catch(e){rounds++;}}while(true){ticks++;}",
    );
    parse_only(
        "producer_HOST",
        "var rounds=0,last='';for(var i=0;i<32;i++){try{fixture.value.length;}catch(e){last=e;rounds++;}try{fixture.method().name;}catch(e){last=e;rounds++;}}rounds;",
    );
    parse_only(
        "producer_CALLS",
        "function user(){return undefined;}var bound=user.bind(null),rounds=0,last='';for(var i=0;i<16;i++){try{user().length;}catch(e){last=e;rounds++;}try{[].pop().length;}catch(e){last=e;rounds++;}try{bound().length;}catch(e){last=e;rounds++;}}rounds;",
    );
    parse_only("diagnostic_public_function", "(function(){null[void 0];});");
    parse_only(
        "producer_getter",
        "var getterCalls=0;(function(){getterCalls++;return undefined;});",
    );
}
