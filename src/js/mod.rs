//! An original, bounded JavaScript interpreter; not a conforming ECMAScript engine yet.
use std::rc::Rc;

pub mod regexp;
pub mod runtime;
mod storage;
pub mod syntax;
pub mod uri;

#[derive(Clone, Debug, PartialEq)]
pub struct Program(pub Vec<Stmt>);

#[derive(Clone, Debug, PartialEq)]
pub enum ForInBinding {
    Var { name: String, init: Option<Expr> },
    Reference(Expr),
}

#[derive(Clone, Debug, PartialEq)]
pub struct SwitchCase {
    pub test: Option<Expr>,
    pub body: Vec<Stmt>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Stmt {
    Empty,
    Expr(Expr),
    Block(Vec<Stmt>),
    Var(Vec<(String, Option<Expr>)>),
    Function {
        name: String,
        // Freeze parsed code once; closure instances share it without sharing
        // their environments, function identities or mutable properties.
        params: Rc<[String]>,
        body: Rc<[Stmt]>,
    },
    Return(Option<Expr>),
    If {
        test: Expr,
        consequent: Box<Stmt>,
        alternate: Option<Box<Stmt>>,
    },
    While {
        test: Expr,
        body: Box<Stmt>,
    },
    DoWhile {
        body: Box<Stmt>,
        test: Expr,
    },
    For {
        init: Option<Box<Stmt>>,
        // Keep infrequent loop fields out of every statement's inline size.
        // Boxes are storage only; they add no grammar node or logical depth.
        test: Option<Box<Expr>>,
        update: Option<Box<Expr>>,
        body: Box<Stmt>,
    },
    ForIn {
        binding: Box<ForInBinding>,
        object: Expr,
        body: Box<Stmt>,
    },
    Switch {
        discriminant: Expr,
        cases: Vec<SwitchCase>,
    },
    Label {
        name: String,
        body: Box<Stmt>,
    },
    Break(Option<String>),
    Continue(Option<String>),
    Throw(Expr),
    Try {
        body: Box<Stmt>,
        catch: Option<(String, Box<Stmt>)>,
        finally: Option<Box<Stmt>>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    Undefined,
    Null,
    Bool(bool),
    Number(f64),
    String(Vec<u16>),
    RegExp {
        pattern: Vec<u16>,
        flags: String,
    },
    Ident(String),
    This,
    Array(Vec<Option<Expr>>),
    Object(Vec<(String, Expr)>),
    Function {
        name: Option<String>,
        params: Rc<[String]>,
        body: Rc<[Stmt]>,
    },
    Unary {
        op: String,
        expr: Box<Expr>,
    },
    Binary {
        op: String,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Assign {
        op: String,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Update {
        op: String,
        expr: Box<Expr>,
        prefix: bool,
    },
    Conditional {
        test: Box<Expr>,
        consequent: Box<Expr>,
        alternate: Box<Expr>,
    },
    Sequence(Vec<Expr>),
    Member {
        object: Box<Expr>,
        property: Box<Expr>,
    },
    Call {
        callee: Box<Expr>,
        args: Vec<Expr>,
    },
    New {
        callee: Box<Expr>,
        args: Vec<Expr>,
    },
}

#[cfg(all(test, target_arch = "x86_64"))]
mod layout_tests {
    use super::*;
    use std::mem::size_of;

    #[test]
    fn selective_loop_boxes_keep_statement_layout_compact() {
        // A measured layout regression on this target, not a public Rust ABI
        // guarantee or an allocator/RSS estimate. Other AST variants stay inline.
        eprintln!(
            "AST_LAYOUT Stmt={} Expr={} ForInBinding={} OptionBoxExpr={} BoxForInBinding={}",
            size_of::<Stmt>(),
            size_of::<Expr>(),
            size_of::<ForInBinding>(),
            size_of::<Option<Box<Expr>>>(),
            size_of::<Box<ForInBinding>>(),
        );
        assert_eq!(size_of::<Stmt>(), 80);
        assert_eq!(size_of::<Expr>(), 56);
        assert_eq!(size_of::<ForInBinding>(), 80);
        assert_eq!(size_of::<Option<Box<Expr>>>(), 8);
        assert_eq!(size_of::<Box<ForInBinding>>(), 8);
    }
}
