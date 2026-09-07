//! An original, bounded JavaScript interpreter; not a conforming ECMAScript engine yet.
pub mod runtime;
pub mod syntax;
pub mod uri;

#[derive(Clone, Debug, PartialEq)]
pub struct Program(pub Vec<Stmt>);

#[derive(Clone, Debug, PartialEq)]
pub enum Stmt {
    Empty,
    Expr(Expr),
    Block(Vec<Stmt>),
    Var(Vec<(String, Option<Expr>)>),
    Function {
        name: String,
        params: Vec<String>,
        body: Vec<Stmt>,
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
        test: Option<Expr>,
        update: Option<Expr>,
        body: Box<Stmt>,
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
    Ident(String),
    This,
    Array(Vec<Option<Expr>>),
    Object(Vec<(String, Expr)>),
    Function {
        name: Option<String>,
        params: Vec<String>,
        body: Vec<Stmt>,
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
