//! Explicit continuations for the expression grammar. No expression production
//! recursively invokes another expression production on the native stack.

use super::{
    E, Expr, Kind, MAX_DEPTH, MAX_TOKENS, Parser, assignable, binary_operator, error,
    number_property, reserved,
};

// A full Expression forwarding path has eight stages (sequence, assignment,
// conditional, binary, unary/postfix, call, new/member, primary), plus a logical
// entry/leave and a container return. Twelve slots per permitted nesting level
// includes the popped, still-active instruction and scheduling bookkeeping.
// Counters belong to Parser, not Machine: a function body cannot reset them.
const MAX_FRAMES: usize = 12 * MAX_DEPTH;
// Forwarding/reduction dispatch is bounded independently of pending storage.
// Thirty-two transitions per source-token budget covers the same forwarding
// path on descent and return, including delimiters and container continuations.
const MAX_STEPS: usize = 32 * MAX_TOKENS;

#[derive(Clone, Copy)]
enum Production {
    Expression { allow_in: bool, comma: bool },
    Binary { minimum: u8, allow_in: bool },
    Unary,
    New,
}

enum Frame {
    Begin(Production),
    Leave,
    SequenceFirst(bool),
    SequenceNext {
        allow_in: bool,
        values: Vec<Expr>,
        depth: usize,
    },
    AssignmentTail(bool),
    AssignmentFinish {
        op: &'static str,
        left: E,
    },
    ConditionalTail(bool),
    ConditionalConsequent {
        allow_in: bool,
        test: E,
    },
    ConditionalAlternate {
        test: E,
        consequent: E,
    },
    BinaryStart {
        minimum: u8,
        allow_in: bool,
    },
    BinaryTail {
        minimum: u8,
        allow_in: bool,
    },
    BinaryRight {
        minimum: u8,
        allow_in: bool,
        op: &'static str,
        left: E,
    },
    UnaryStart,
    UnaryFinish(&'static str),
    Postfix,
    LeftHandSide,
    LeftHandSideTail,
    NewStart,
    NewCallee,
    NewTail,
    MemberFinish(E),
    ArgumentNext {
        callee: E,
        construct: bool,
        args: Vec<Expr>,
        depth: usize,
    },
    Primary,
    GroupEnd,
    ArrayStart {
        values: Vec<Option<Expr>>,
        depth: usize,
    },
    ArrayNext {
        values: Vec<Option<Expr>>,
        depth: usize,
    },
    ObjectStart {
        values: Vec<(String, Expr)>,
        depth: usize,
    },
    ObjectNext {
        values: Vec<(String, Expr)>,
        depth: usize,
        key: String,
    },
}

struct Machine {
    // Grow on demand, never reserve the full limit for every nested function.
    // Each frame owns only previously depth-checked ASTs. One result register
    // replaces a separate, potentially unbounded operand stack.
    frames: Vec<Frame>,
    value: Option<E>,
}

enum Action {
    Continue,
    Function,
}

impl<'a> Parser<'a> {
    fn array_literal(&mut self, values: Vec<Option<Expr>>, depth: usize) -> Result<E, String> {
        // Keep the existing node/depth rejection ahead of any finalization work.
        self.node(depth)?;
        // Only a completed literal loses builder slack. Consume the buffer,
        // without cloning descendants; the public AST and capacity visitor stay
        // unchanged. A shrink can reallocate, so parser-temporary/OS bounds still
        // apply (docs/AST_ARRAYS.md), separately from retained realm accounting.
        let values = if values.capacity() == values.len() {
            values
        } else {
            values.into_boxed_slice().into_vec()
        };
        Ok(E {
            value: Expr::Array(values),
            depth,
        })
    }

    pub(super) fn expression_machine(&mut self, allow_in: bool, comma: bool) -> Result<E, String> {
        let nesting = self.nesting;
        let frames = self.expression_frames;
        let mut machine = Machine {
            frames: Vec::new(),
            value: None,
        };
        let result = (|| {
            machine.enter(self, Production::Expression { allow_in, comma })?;
            self.expression_driver(&mut machine)
        })();
        // Pending Leave instructions do not run on an error. Restore exactly
        // this invocation's counters, retaining any suspended outer machine.
        // Work already performed is cumulative and must never be restored.
        self.nesting = nesting;
        self.expression_frames = frames;
        result
    }

    fn expression_driver(&mut self, machine: &mut Machine) -> Result<E, String> {
        while let Some(frame) = machine.frames.pop() {
            if self.expression_steps >= MAX_STEPS {
                return Err(error(self.token().offset, "Expression work limit exceeded"));
            }
            self.expression_steps += 1;
            // Keep this popped instruction charged until its action completes,
            // including nested function-body parsing. step() returns first so
            // its large dispatch frame is not retained across native recursion.
            match machine.step(self, frame)? {
                Action::Continue => {}
                Action::Function => {
                    machine.value = Some(self.expression_function()?);
                }
            }
            self.expression_frames -= 1;
        }
        Ok(machine.take())
    }

    #[inline(never)]
    fn expression_function(&mut self) -> Result<E, String> {
        let (name, params, body, depth) = self.function(false)?;
        self.expr(
            Expr::Function {
                name,
                params: params.into(),
                body: body.into(),
            },
            depth + 1,
        )
    }
}

impl Machine {
    fn take(&mut self) -> E {
        self.value
            .take()
            .expect("expression continuation follows its operand")
    }

    fn push(&mut self, parser: &mut Parser<'_>, frame: Frame) -> Result<(), String> {
        if parser.expression_frames >= MAX_FRAMES {
            return Err(error(
                parser.token().offset,
                "Expression stack limit exceeded",
            ));
        }
        parser.expression_frames += 1;
        self.frames.push(frame);
        Ok(())
    }

    fn enter(&mut self, parser: &mut Parser<'_>, production: Production) -> Result<(), String> {
        if parser.nesting >= MAX_DEPTH {
            return Err(error(
                parser.token().offset,
                "Parser nesting limit exceeded",
            ));
        }
        parser.nesting += 1;
        self.push(parser, Frame::Leave)?;
        self.push(parser, Frame::Begin(production))
    }

    fn child(
        &mut self,
        parser: &mut Parser<'_>,
        allow_in: bool,
        comma: bool,
    ) -> Result<(), String> {
        self.enter(parser, Production::Expression { allow_in, comma })
    }

    fn call(
        &mut self,
        parser: &mut Parser<'_>,
        callee: E,
        construct: bool,
        args: Vec<Expr>,
        depth: usize,
    ) -> Result<(), String> {
        let depth = callee.depth.max(depth) + 1;
        let callee = Box::new(callee.value);
        self.value = Some(parser.expr(
            if construct {
                Expr::New { callee, args }
            } else {
                Expr::Call { callee, args }
            },
            depth,
        )?);
        Ok(())
    }

    fn arguments(
        &mut self,
        parser: &mut Parser<'_>,
        callee: E,
        construct: bool,
    ) -> Result<(), String> {
        parser.expect("(")?;
        if parser.eat(")") {
            self.call(parser, callee, construct, Vec::new(), 0)
        } else {
            self.push(
                parser,
                Frame::ArgumentNext {
                    callee,
                    construct,
                    args: Vec::new(),
                    depth: 0,
                },
            )?;
            self.child(parser, true, false)
        }
    }

    fn member(&mut self, parser: &mut Parser<'_>, object: E) -> Result<(), String> {
        if parser.eat(".") {
            let name = parser.name(true)?;
            let property = parser.expr(Expr::String(name.encode_utf16().collect()), 1)?;
            self.member_value(parser, object, property)
        } else {
            parser.expect("[")?;
            self.push(parser, Frame::MemberFinish(object))?;
            self.child(parser, true, true)
        }
    }

    fn member_value(
        &mut self,
        parser: &mut Parser<'_>,
        object: E,
        property: E,
    ) -> Result<(), String> {
        let depth = object.depth.max(property.depth) + 1;
        self.value = Some(parser.expr(
            Expr::Member {
                object: Box::new(object.value),
                property: Box::new(property.value),
            },
            depth,
        )?);
        Ok(())
    }

    #[inline(never)]
    fn step(&mut self, parser: &mut Parser<'_>, frame: Frame) -> Result<Action, String> {
        match frame {
            Frame::Begin(production) => match production {
                Production::Expression { allow_in, comma } => {
                    if comma {
                        self.push(parser, Frame::SequenceFirst(allow_in))?;
                    }
                    self.push(parser, Frame::AssignmentTail(allow_in))?;
                    self.push(parser, Frame::ConditionalTail(allow_in))?;
                    self.push(
                        parser,
                        Frame::BinaryStart {
                            minimum: 1,
                            allow_in,
                        },
                    )?;
                }
                Production::Binary { minimum, allow_in } => {
                    self.push(parser, Frame::BinaryStart { minimum, allow_in })?
                }
                Production::Unary => self.push(parser, Frame::UnaryStart)?,
                Production::New => self.push(parser, Frame::NewStart)?,
            },
            Frame::Leave => parser.nesting -= 1,
            Frame::SequenceFirst(allow_in) => {
                if parser.eat(",") {
                    let first = self.take();
                    self.push(
                        parser,
                        Frame::SequenceNext {
                            allow_in,
                            values: vec![first.value],
                            depth: first.depth,
                        },
                    )?;
                    self.child(parser, allow_in, false)?;
                }
            }
            Frame::SequenceNext {
                allow_in,
                mut values,
                depth,
            } => {
                let next = self.take();
                let depth = depth.max(next.depth);
                values.push(next.value);
                if parser.eat(",") {
                    self.push(
                        parser,
                        Frame::SequenceNext {
                            allow_in,
                            values,
                            depth,
                        },
                    )?;
                    self.child(parser, allow_in, false)?;
                } else {
                    self.value = Some(parser.expr(Expr::Sequence(values), depth + 1)?);
                }
            }
            Frame::AssignmentTail(allow_in) => {
                if let Kind::Punct(
                    op @ ("=" | "+=" | "-=" | "*=" | "/=" | "%=" | "<<=" | ">>=" | ">>>=" | "&="
                    | "|=" | "^="),
                ) = parser.token().kind
                {
                    let left = self.take();
                    if !assignable(&left.value) {
                        return Err(parser.fail("Invalid assignment target"));
                    }
                    parser.advance();
                    self.push(parser, Frame::AssignmentFinish { op, left })?;
                    self.child(parser, allow_in, false)?;
                }
            }
            Frame::AssignmentFinish { op, left } => {
                let right = self.take();
                let depth = left.depth.max(right.depth) + 1;
                self.value = Some(parser.expr(
                    Expr::Assign {
                        op,
                        left: Box::new(left.value),
                        right: Box::new(right.value),
                    },
                    depth,
                )?);
            }
            Frame::ConditionalTail(allow_in) => {
                if parser.eat("?") {
                    let test = self.take();
                    self.push(parser, Frame::ConditionalConsequent { allow_in, test })?;
                    self.child(parser, true, false)?;
                }
            }
            Frame::ConditionalConsequent { allow_in, test } => {
                let consequent = self.take();
                parser.expect(":")?;
                self.push(parser, Frame::ConditionalAlternate { test, consequent })?;
                self.child(parser, allow_in, false)?;
            }
            Frame::ConditionalAlternate { test, consequent } => {
                let alternate = self.take();
                let depth = test.depth.max(consequent.depth).max(alternate.depth) + 1;
                self.value = Some(parser.expr(
                    Expr::Conditional {
                        test: Box::new(test.value),
                        consequent: Box::new(consequent.value),
                        alternate: Box::new(alternate.value),
                    },
                    depth,
                )?);
            }
            Frame::BinaryStart { minimum, allow_in } => {
                self.push(parser, Frame::BinaryTail { minimum, allow_in })?;
                self.push(parser, Frame::UnaryStart)?;
            }
            Frame::BinaryTail { minimum, allow_in } => {
                if let Some((op, precedence)) = binary_operator(&parser.token().kind, allow_in)
                    && precedence >= minimum
                {
                    let left = self.take();
                    parser.advance();
                    self.push(
                        parser,
                        Frame::BinaryRight {
                            minimum,
                            allow_in,
                            op,
                            left,
                        },
                    )?;
                    self.enter(
                        parser,
                        Production::Binary {
                            minimum: precedence + 1,
                            allow_in,
                        },
                    )?;
                }
            }
            Frame::BinaryRight {
                minimum,
                allow_in,
                op,
                left,
            } => {
                let right = self.take();
                let depth = left.depth.max(right.depth) + 1;
                self.value = Some(parser.expr(
                    Expr::Binary {
                        op,
                        left: Box::new(left.value),
                        right: Box::new(right.value),
                    },
                    depth,
                )?);
                self.push(parser, Frame::BinaryTail { minimum, allow_in })?;
            }
            Frame::UnaryStart => {
                let op = match &parser.token().kind {
                    Kind::Punct(op @ ("+" | "-" | "!" | "~" | "++" | "--")) => Some(*op),
                    Kind::Word(op) => match op.as_str() {
                        "typeof" => Some("typeof"),
                        "void" => Some("void"),
                        "delete" => Some("delete"),
                        _ => None,
                    },
                    _ => None,
                };
                if let Some(op) = op {
                    parser.advance();
                    self.push(parser, Frame::UnaryFinish(op))?;
                    self.enter(parser, Production::Unary)?;
                } else {
                    self.push(parser, Frame::Postfix)?;
                    self.push(parser, Frame::LeftHandSide)?;
                }
            }
            Frame::UnaryFinish(op) => {
                let expression = self.take();
                let depth = expression.depth + 1;
                let value = if op == "++" || op == "--" {
                    if !assignable(&expression.value) {
                        return Err(parser.fail("Invalid update target"));
                    }
                    Expr::Update {
                        op,
                        expr: Box::new(expression.value),
                        prefix: true,
                    }
                } else {
                    Expr::Unary {
                        op,
                        expr: Box::new(expression.value),
                    }
                };
                self.value = Some(parser.expr(value, depth)?);
            }
            Frame::Postfix => {
                if !parser.token().newline && (parser.punct("++") || parser.punct("--")) {
                    let expression = self.take();
                    if !assignable(&expression.value) {
                        return Err(parser.fail("Invalid update target"));
                    }
                    let Kind::Punct(op) = parser.token().kind else {
                        unreachable!()
                    };
                    parser.advance();
                    self.value = Some(parser.expr(
                        Expr::Update {
                            op,
                            expr: Box::new(expression.value),
                            prefix: false,
                        },
                        expression.depth + 1,
                    )?);
                }
            }
            Frame::LeftHandSide => {
                self.push(parser, Frame::LeftHandSideTail)?;
                self.push(parser, Frame::NewStart)?;
            }
            Frame::LeftHandSideTail => {
                if parser.punct("(") {
                    let callee = self.take();
                    self.push(parser, Frame::LeftHandSideTail)?;
                    self.arguments(parser, callee, false)?;
                } else if parser.punct(".") || parser.punct("[") {
                    let object = self.take();
                    self.push(parser, Frame::LeftHandSideTail)?;
                    self.member(parser, object)?;
                }
            }
            Frame::NewStart => {
                self.push(parser, Frame::NewTail)?;
                if parser.eat_word("new") {
                    self.push(parser, Frame::NewCallee)?;
                    self.enter(parser, Production::New)?;
                } else {
                    self.push(parser, Frame::Primary)?;
                }
            }
            Frame::NewCallee => {
                let callee = self.take();
                if parser.punct("(") {
                    self.arguments(parser, callee, true)?;
                } else {
                    self.call(parser, callee, true, Vec::new(), 0)?;
                }
            }
            Frame::NewTail => {
                if parser.punct(".") || parser.punct("[") {
                    let object = self.take();
                    self.push(parser, Frame::NewTail)?;
                    self.member(parser, object)?;
                }
            }
            Frame::MemberFinish(object) => {
                let property = self.take();
                parser.expect("]")?;
                self.member_value(parser, object, property)?;
            }
            Frame::ArgumentNext {
                callee,
                construct,
                mut args,
                depth,
            } => {
                let argument = self.take();
                let depth = depth.max(argument.depth);
                args.push(argument.value);
                if parser.eat(",") {
                    self.push(
                        parser,
                        Frame::ArgumentNext {
                            callee,
                            construct,
                            args,
                            depth,
                        },
                    )?;
                    self.child(parser, true, false)?;
                } else {
                    parser.expect(")")?;
                    self.call(parser, callee, construct, args, depth)?;
                }
            }
            Frame::Primary => {
                let token = parser.token().clone();
                match token.kind {
                    Kind::Number(value) => {
                        parser.advance();
                        self.value = Some(parser.expr(Expr::Number(value), 1)?);
                    }
                    Kind::String(value) => {
                        parser.advance();
                        self.value = Some(parser.expr(Expr::String(value), 1)?);
                    }
                    Kind::Word(ref word) if word == "function" => {
                        parser.advance();
                        return Ok(Action::Function);
                    }
                    Kind::Word(word) => {
                        parser.advance();
                        let value = match word.as_str() {
                            "true" => Expr::Bool(true),
                            "false" => Expr::Bool(false),
                            "null" => Expr::Null,
                            "this" => Expr::This,
                            word if reserved(word) => {
                                return Err(error(
                                    token.offset,
                                    "Unsupported or misplaced reserved word",
                                ));
                            }
                            _ => Expr::Ident(word),
                        };
                        self.value = Some(parser.expr(value, 1)?);
                    }
                    Kind::Punct("(") => {
                        parser.advance();
                        self.push(parser, Frame::GroupEnd)?;
                        self.child(parser, true, true)?;
                    }
                    Kind::Punct("[") => {
                        parser.advance();
                        self.push(
                            parser,
                            Frame::ArrayStart {
                                values: Vec::new(),
                                depth: 0,
                            },
                        )?;
                    }
                    Kind::Punct("{") => {
                        parser.advance();
                        self.push(
                            parser,
                            Frame::ObjectStart {
                                values: Vec::new(),
                                depth: 0,
                            },
                        )?;
                    }
                    Kind::Punct("/" | "/=") => {
                        debug_assert!(parser.lookahead.is_none());
                        let (pattern, flags) = parser.lexer.regexp(token.offset)?;
                        parser.advance();
                        self.value = Some(parser.expr(Expr::RegExp { pattern, flags }, 1)?);
                    }
                    Kind::Invalid(message) => return Err(message),
                    Kind::End => {
                        return Err(error(
                            token.offset,
                            "Unexpected end of source; expected expression",
                        ));
                    }
                    _ => {
                        return Err(error(
                            token.offset,
                            "Expected expression or encountered unsupported syntax",
                        ));
                    }
                }
            }
            Frame::GroupEnd => parser.expect(")")?,
            Frame::ArrayStart { mut values, depth } => {
                while parser.eat(",") {
                    values.push(None);
                }
                if parser.eat("]") {
                    self.value = Some(parser.array_literal(values, depth + 1)?);
                } else {
                    self.push(parser, Frame::ArrayNext { values, depth })?;
                    self.child(parser, true, false)?;
                }
            }
            Frame::ArrayNext { mut values, depth } => {
                let value = self.take();
                let depth = depth.max(value.depth);
                values.push(Some(value.value));
                if parser.eat(",") {
                    self.push(parser, Frame::ArrayStart { values, depth })?;
                } else {
                    parser.expect("]")?;
                    self.value = Some(parser.array_literal(values, depth + 1)?);
                }
            }
            Frame::ObjectStart { values, depth } => {
                if parser.eat("}") {
                    self.value = Some(parser.expr(Expr::Object(values), depth + 1)?);
                } else {
                    let token = parser.token().clone();
                    let key = match token.kind {
                        Kind::Word(name) => name,
                        Kind::String(value) => String::from_utf16(&value).map_err(|_| {
                            error(
                                token.offset,
                                "Lone-surrogate object property names are unsupported",
                            )
                        })?,
                        Kind::Number(value) => number_property(value),
                        _ => return Err(parser.fail(
                            "Expected object property name; computed properties are unsupported",
                        )),
                    };
                    parser.advance();
                    if !parser.eat(":") {
                        return Err(parser.fail(
                            "Expected colon; object accessors and shorthand are unsupported",
                        ));
                    }
                    self.push(parser, Frame::ObjectNext { values, depth, key })?;
                    self.child(parser, true, false)?;
                }
            }
            Frame::ObjectNext {
                mut values,
                depth,
                key,
            } => {
                let value = self.take();
                let depth = depth.max(value.depth);
                values.push((key, value.value));
                if parser.eat(",") {
                    self.push(parser, Frame::ObjectStart { values, depth })?;
                } else {
                    parser.expect("}")?;
                    self.value = Some(parser.expr(Expr::Object(values), depth + 1)?);
                }
            }
        }
        Ok(Action::Continue)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::js::syntax::{MAX_NODES, is_limit_error};

    #[test]
    fn completed_array_moves_payloads_and_preserves_nested_storage() {
        use std::{mem::size_of, rc::Rc};

        assert_eq!(size_of::<Vec<Option<Expr>>>(), 3 * size_of::<usize>());
        #[cfg(target_arch = "x86_64")]
        assert_eq!((size_of::<Expr>(), size_of::<Option<Expr>>()), (56, 56));

        let mut text = Vec::with_capacity(17);
        text.extend([0xd800, 0, 0xdc00]);
        let text_storage = (text.as_ptr(), text.capacity());
        let mut nested = Vec::with_capacity(19);
        nested.extend([None, Some(Expr::Number(-0.0))]);
        let nested_storage = (nested.as_ptr(), nested.capacity());
        let params: Rc<[String]> = vec!["parameter".to_owned()].into();
        let body: Rc<[crate::js::Stmt]> = vec![crate::js::Stmt::Return(None)].into();
        let shared_storage = (params.as_ptr(), body.as_ptr());
        let boxed = Box::new(Expr::Number(42.0));
        let boxed_storage = &*boxed as *const Expr;
        let mut values = Vec::with_capacity(16);
        values.extend([
            Some(Expr::String(text)),
            Some(Expr::Array(nested)),
            Some(Expr::Function {
                name: None,
                params,
                body,
            }),
            Some(Expr::Unary {
                op: "-",
                expr: boxed,
            }),
            None,
        ]);
        let mut parser = Parser::new("", MAX_TOKENS);
        let completed = parser.array_literal(values, 4).unwrap();
        assert_eq!((completed.depth, parser.nodes), (4, 1));
        let Expr::Array(values) = completed.value else {
            panic!("array expected")
        };
        assert_eq!((values.len(), values.capacity()), (5, 5));
        let Some(Expr::String(text)) = &values[0] else {
            panic!("string expected")
        };
        assert_eq!((text.as_ptr(), text.capacity()), text_storage);
        assert_eq!(text, &[0xd800, 0, 0xdc00]);
        let Some(Expr::Array(nested)) = &values[1] else {
            panic!("nested array expected")
        };
        assert_eq!((nested.as_ptr(), nested.capacity()), nested_storage);
        assert!(nested[0].is_none());
        let Some(Expr::Number(number)) = &nested[1] else {
            panic!("number expected")
        };
        assert_eq!(number.to_bits(), (-0.0f64).to_bits());
        let Some(Expr::Function { params, body, .. }) = &values[2] else {
            panic!("function expected")
        };
        assert_eq!((params.as_ptr(), body.as_ptr()), shared_storage);
        assert_eq!((Rc::strong_count(params), Rc::strong_count(body)), (1, 1));
        let Some(Expr::Unary { expr, .. }) = &values[3] else {
            panic!("unary expected")
        };
        assert_eq!(&**expr as *const Expr, boxed_storage);
        assert!(values[4].is_none());

        // Empty spare storage is released; already exact storage moves intact.
        for (length, capacity) in [(0, 0), (0, 7), (3, 3)] {
            let mut values = Vec::with_capacity(capacity);
            values.resize_with(length, || None);
            let before = values.as_ptr();
            let completed = parser.array_literal(values, 1).unwrap();
            let Expr::Array(values) = completed.value else {
                panic!("array expected")
            };
            assert_eq!((values.len(), values.capacity()), (length, length));
            if length == capacity {
                assert_eq!(values.as_ptr(), before);
            }
        }
    }

    #[test]
    fn array_closing_paths_preserve_nodes_depth_and_pending_machine_state() {
        for (source, length, nodes, depth) in [
            ("[]", 0, 1, 1),
            ("[,]", 1, 1, 1),
            ("[1]", 1, 2, 2),
            ("[1,]", 1, 2, 2),
            ("[1,,]", 2, 2, 2),
            ("[[1],,]", 2, 3, 3),
        ] {
            let mut parser = Parser::new(source, MAX_TOKENS);
            parser.nodes = 11;
            parser.nesting = 5;
            parser.expression_frames = 19;
            parser.expression_steps = 23;
            let completed = parser.expression(true).unwrap();
            let Expr::Array(values) = completed.value else {
                panic!("{source}")
            };
            assert_eq!(
                (values.len(), values.capacity()),
                (length, length),
                "{source}"
            );
            assert_eq!(
                (completed.depth, parser.nodes),
                (depth, 11 + nodes),
                "{source}"
            );
            assert_eq!(
                (parser.nesting, parser.expression_frames),
                (5, 19),
                "{source}"
            );
            assert!(parser.expression_steps > 23, "{source}");
            assert!(parser.end(), "{source}");
        }
        for (source, admitted_children) in [("[1;]", 1), ("[1,;]", 1), ("[[1],;]", 2)] {
            let mut parser = Parser::new(source, MAX_TOKENS);
            parser.nodes = 11;
            parser.nesting = 5;
            parser.expression_frames = 19;
            parser.expression_steps = 23;
            let error = parser.expression(true).err().unwrap();
            assert!(!is_limit_error(&error), "{source}: {error}");
            assert_eq!(parser.nodes, 11 + admitted_children, "{source}");
            assert_eq!(
                (parser.nesting, parser.expression_frames),
                (5, 19),
                "{source}"
            );
            assert!(parser.expression_steps > 23, "{source}");
        }
    }

    #[test]
    fn array_finalization_retains_node_depth_admission_and_first_errors() {
        // A cached lexical error does not supersede either structural guard;
        // depth continues to win when both resource guards would reject.
        for (nodes, depth, expected) in [
            (
                MAX_NODES - 1,
                MAX_DEPTH + 1,
                "AST depth limit exceeded at byte 0",
            ),
            (
                MAX_NODES,
                MAX_DEPTH + 1,
                "AST depth limit exceeded at byte 0",
            ),
            (MAX_NODES, MAX_DEPTH, "AST node limit exceeded at byte 0"),
        ] {
            let mut parser = Parser::new("@", MAX_TOKENS);
            parser.nodes = nodes;
            let error = parser
                .array_literal(Vec::with_capacity(7), depth)
                .err()
                .unwrap();
            assert_eq!(error, expected);
            assert!(is_limit_error(&error));
            assert_eq!(parser.nodes, nodes);
            assert_eq!(
                (
                    parser.nesting,
                    parser.expression_frames,
                    parser.expression_steps
                ),
                (0, 0, 0)
            );
        }
        let mut parser = Parser::new("", MAX_TOKENS);
        parser.nodes = MAX_NODES - 1;
        let completed = parser
            .array_literal(Vec::with_capacity(7), MAX_DEPTH)
            .unwrap();
        assert_eq!((completed.depth, parser.nodes), (MAX_DEPTH, MAX_NODES));
        let Expr::Array(values) = completed.value else {
            panic!("array expected")
        };
        assert_eq!((values.len(), values.capacity()), (0, 0));

        // Both real closes encounter the invalid next token before attempting
        // the array node. Rejection must still report that node, not the token.
        for source in ["[1]@", "[1,]@"] {
            let mut parser = Parser::new(source, MAX_TOKENS);
            parser.nodes = MAX_NODES - 1;
            parser.nesting = 5;
            parser.expression_frames = 19;
            parser.expression_steps = 23;
            let error = parser.expression(true).err().unwrap();
            assert_eq!(
                error,
                format!("AST node limit exceeded at byte {}", source.len() - 1)
            );
            assert_eq!(parser.nodes, MAX_NODES);
            assert_eq!((parser.nesting, parser.expression_frames), (5, 19));
            assert!(parser.expression_steps > 23);
        }
    }

    #[test]
    fn grouping_uses_logical_depth_without_retaining_ast_wrappers() {
        let source = format!("{}42{}", "(".repeat(64), ")".repeat(64));
        let mut parser = Parser::new(&source, MAX_TOKENS);
        let value = parser.expression(true).unwrap();
        assert_eq!(value.value, Expr::Number(42.0));
        assert_eq!(value.depth, 1);
        assert_eq!(parser.nodes, 1);
        assert_eq!((parser.nesting, parser.expression_frames), (0, 0));
        assert!(parser.end());
        assert!(parser.expression_steps < MAX_STEPS);

        // The remaining syntax-depth guard is unchanged, even though grouping
        // deliberately adds no retained AST node.
        let deep = format!("{}42{}", "(".repeat(MAX_DEPTH), ")".repeat(MAX_DEPTH));
        let mut parser = Parser::new(&deep, MAX_TOKENS);
        let error = parser.expression(true).err().unwrap();
        assert!(
            error.starts_with("Parser nesting limit exceeded"),
            "{error}"
        );
        assert_eq!((parser.nesting, parser.expression_frames), (0, 0));
    }

    #[test]
    fn nested_functions_preserve_outer_machine_counters_on_success_and_error() {
        for (source, succeeds) in [
            ("(function(){return (function(){return 42;});})", true),
            ("(function(){return (function(){return [;});})", false),
        ] {
            let mut parser = Parser::new(source, MAX_TOKENS);
            parser.nesting = 7;
            parser.expression_frames = 19;
            parser.expression_steps = 23;
            let result = parser.expression(true);
            assert_eq!(result.is_ok(), succeeds, "{source}");
            assert_eq!((parser.nesting, parser.expression_frames), (7, 19));
            assert!(parser.expression_steps > 23);
            assert_eq!(parser.functions, 0);
        }

        // Entering a new function body must not reset the shared structural
        // guard, even though it starts a separate heap-backed machine. Run on
        // the ordinary Rust test thread; no increased native stack is needed.
        let source = format!(
            "{}42{}",
            "function(){return ".repeat(MAX_DEPTH),
            ";}".repeat(MAX_DEPTH)
        );
        let mut parser = Parser::new(&source, MAX_TOKENS);
        parser.nesting = 7;
        parser.expression_frames = 19;
        let error = parser.expression(true).err().unwrap();
        assert!(
            error.starts_with("Parser nesting limit exceeded"),
            "{error}"
        );
        assert_eq!((parser.nesting, parser.expression_frames), (7, 19));
        assert_eq!(parser.functions, 0);
    }

    #[test]
    fn active_function_instruction_remains_charged_during_reentry() {
        let mut parser = Parser::new("function(){return (function(){return 42;});}", MAX_TOKENS);
        let mut machine = Machine {
            frames: Vec::new(),
            value: None,
        };
        machine
            .enter(
                &mut parser,
                Production::Expression {
                    allow_in: true,
                    comma: true,
                },
            )
            .unwrap();
        let mut callbacks = 0;
        while let Some(frame) = machine.frames.pop() {
            assert_eq!(parser.expression_frames, machine.frames.len() + 1);
            parser.expression_steps += 1;
            if matches!(machine.step(&mut parser, frame).unwrap(), Action::Function) {
                callbacks += 1;
                let active = parser.expression_frames;
                let nesting = parser.nesting;
                let steps = parser.expression_steps;
                assert_eq!(active, machine.frames.len() + 1);
                machine.value = Some(parser.expression_function().unwrap());
                assert_eq!(parser.expression_frames, active);
                assert_eq!(parser.nesting, nesting);
                assert!(parser.expression_steps > steps);
            }
            parser.expression_frames -= 1;
        }
        assert_eq!(callbacks, 1);
        assert!(matches!(machine.take().value, Expr::Function { .. }));
        assert_eq!((parser.nesting, parser.expression_frames), (0, 0));
    }

    #[test]
    fn storage_and_work_exhaustion_restore_pending_outer_state_not_work() {
        // These direct state tests exercise guards that ordinarily have ample
        // headroom: 12 slots per structural level, 32 transitions per token.
        assert_eq!(MAX_FRAMES, 12 * MAX_DEPTH);
        assert_eq!(MAX_STEPS, 32 * MAX_TOKENS);
        for source in ["42", "@"] {
            let mut parser = Parser::new(source, MAX_TOKENS);
            parser.nesting = 5;
            parser.expression_frames = MAX_FRAMES - 1;
            let error = parser.expression(true).err().unwrap();
            assert!(
                error.starts_with("Expression stack limit exceeded"),
                "{error}"
            );
            assert!(is_limit_error(&error));
            assert_eq!(parser.nesting, 5);
            assert_eq!(parser.expression_frames, MAX_FRAMES - 1);
            assert_eq!(parser.expression_steps, 0);

            let mut parser = Parser::new(source, MAX_TOKENS);
            parser.nesting = 5;
            parser.expression_frames = 19;
            parser.expression_steps = MAX_STEPS - 1;
            let error = parser.expression(true).err().unwrap();
            assert!(
                error.starts_with("Expression work limit exceeded"),
                "{error}"
            );
            assert!(is_limit_error(&error));
            assert_eq!((parser.nesting, parser.expression_frames), (5, 19));
            assert_eq!(parser.expression_steps, MAX_STEPS);
            let second = parser.expression(true).err().unwrap();
            assert!(
                second.starts_with("Expression work limit exceeded"),
                "{second}"
            );
            assert_eq!(parser.expression_steps, MAX_STEPS);
            assert_eq!((parser.nesting, parser.expression_frames), (5, 19));

            // Structural and retained-AST limits are equally fatal when a
            // preceding advance already discovered malformed source. Resource
            // diagnostics must not be replaced by that cached lexical error.
            let mut parser = Parser::new(source, MAX_TOKENS);
            parser.nesting = MAX_DEPTH;
            let error = parser.expression(true).err().unwrap();
            assert!(
                error.starts_with("Parser nesting limit exceeded"),
                "{error}"
            );
            assert!(is_limit_error(&error));
            assert_eq!((parser.nesting, parser.expression_frames), (MAX_DEPTH, 0));
            let error = parser.nested(|_| Ok(())).unwrap_err();
            assert!(
                error.starts_with("Parser nesting limit exceeded"),
                "{error}"
            );
            assert!(is_limit_error(&error));
            let error = parser.node(MAX_DEPTH + 1).unwrap_err();
            assert!(error.starts_with("AST depth limit exceeded"), "{error}");
            assert!(is_limit_error(&error));
            parser.nodes = MAX_NODES;
            let error = parser.node(1).unwrap_err();
            assert!(error.starts_with("AST node limit exceeded"), "{error}");
            assert!(is_limit_error(&error));
        }
    }

    #[test]
    fn recoverable_syntax_failure_releases_only_its_own_pending_work() {
        let mut parser = Parser::new("1 + ; 42", MAX_TOKENS);
        parser.nesting = 5;
        parser.expression_frames = 19;
        let error = parser.expression(true).err().unwrap();
        assert!(!is_limit_error(&error));
        assert_eq!((parser.nesting, parser.expression_frames), (5, 19));
        let spent = parser.expression_steps;
        assert!(spent > 0);
        // Public parse() never recovers a prefix. An internal second invocation
        // nevertheless verifies exact counter cleanup after an ordinary error.
        assert!(parser.eat(";"));
        assert_eq!(parser.expression(true).unwrap().value, Expr::Number(42.0));
        assert_eq!((parser.nesting, parser.expression_frames), (5, 19));
        assert!(parser.expression_steps > spent);
    }
}
