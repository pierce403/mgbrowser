//! Retained storage allowance for one freshly parsed, depth-checked AST.
//!
//! Container slots own inline enum/tuple fields; traversing those fields counts
//! only their separately allocated descendants. Capacity (including holes and
//! spare slots), not length alone, determines Vec/String storage. Parsed function
//! Rc slices are unique at admission; runtime closure clones share those already
//! admitted slices and must not call this visitor again. Each new parse pays anew.
//!
//! A fixed per-block allowance and an aligned two-counter Rc header allowance are
//! conservative policy, not claims about allocator internals or process RSS.
//! Source/attempt charges and bounded parser temporaries are separate. Walking
//! this immutable tree allocates nothing and relies on the parser's depth cap.

use super::{Expr, ForInBinding, Program, Stmt, SwitchCase};
use std::mem::{align_of, size_of};

const ALLOCATION_OVERHEAD: usize = 16;

pub(super) fn program_bytes(program: &Program) -> usize {
    let mut storage = Storage::default();
    storage.statements(&program.0, program.0.capacity());
    storage.0
}

// A dynamic Function parser returns its root Expr by value, not in a heap slot.
// Its owned name/parameter/body allocations still receive their full allowance.
pub(super) fn expression_bytes(expression: &Expr) -> usize {
    let mut storage = Storage::default();
    storage.expression(expression);
    storage.0
}

fn aligned(bytes: usize, alignment: usize) -> usize {
    let padding = (alignment - bytes % alignment) % alignment;
    bytes.saturating_add(padding)
}

fn buffer_bytes<T>(capacity: usize) -> usize {
    let payload = capacity.saturating_mul(size_of::<T>());
    if payload == 0 {
        0
    } else {
        payload.saturating_add(ALLOCATION_OVERHEAD)
    }
}

fn shared_bytes<T>(length: usize) -> usize {
    // All current AST element alignments are covered explicitly; unlike Vec,
    // even an empty Rc slice retains its shared allocation/control allowance.
    let header = aligned(2 * size_of::<usize>(), align_of::<T>());
    aligned(
        header.saturating_add(length.saturating_mul(size_of::<T>())),
        align_of::<T>().max(align_of::<usize>()),
    )
    .saturating_add(ALLOCATION_OVERHEAD)
}

#[derive(Default)]
struct Storage(usize);

impl Storage {
    fn add(&mut self, bytes: usize) {
        self.0 = self.0.saturating_add(bytes);
    }

    fn string(&mut self, string: &String) {
        self.add(buffer_bytes::<u8>(string.capacity()));
    }

    fn statements(&mut self, statements: &[Stmt], capacity: usize) {
        self.add(buffer_bytes::<Stmt>(capacity));
        for statement in statements {
            self.statement(statement);
        }
    }

    fn expressions(&mut self, expressions: &[Expr], capacity: usize) {
        self.add(buffer_bytes::<Expr>(capacity));
        for expression in expressions {
            self.expression(expression);
        }
    }

    fn boxed_statement(&mut self, statement: &Stmt) {
        self.add(buffer_bytes::<Stmt>(1));
        self.statement(statement);
    }

    fn boxed_expression(&mut self, expression: &Expr) {
        self.add(buffer_bytes::<Expr>(1));
        self.expression(expression);
    }

    fn function(&mut self, name: Option<&String>, parameters: &[String], body: &[Stmt]) {
        if let Some(name) = name {
            self.string(name);
        }
        self.add(shared_bytes::<String>(parameters.len()));
        for parameter in parameters {
            self.string(parameter);
        }
        self.add(shared_bytes::<Stmt>(body.len()));
        for statement in body {
            self.statement(statement);
        }
    }

    fn statement(&mut self, statement: &Stmt) {
        match statement {
            Stmt::Empty => {}
            Stmt::Expr(expression) | Stmt::Throw(expression) => self.expression(expression),
            Stmt::Block(body) => self.statements(body, body.capacity()),
            Stmt::Var(bindings) => {
                self.add(buffer_bytes::<(String, Option<Expr>)>(bindings.capacity()));
                for (name, initializer) in bindings {
                    self.string(name);
                    if let Some(initializer) = initializer {
                        self.expression(initializer);
                    }
                }
            }
            Stmt::Function { name, params, body } => self.function(Some(name), params, body),
            Stmt::Return(expression) => {
                if let Some(expression) = expression {
                    self.expression(expression);
                }
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
                self.add(buffer_bytes::<ForInBinding>(1));
                match binding.as_ref() {
                    ForInBinding::Var { name, init } => {
                        self.string(name);
                        if let Some(init) = init {
                            self.expression(init);
                        }
                    }
                    ForInBinding::Reference(expression) => self.expression(expression),
                }
                self.expression(object);
                self.boxed_statement(body);
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                self.expression(discriminant);
                self.add(buffer_bytes::<SwitchCase>(cases.capacity()));
                for case in cases {
                    if let Some(test) = &case.test {
                        self.expression(test);
                    }
                    self.statements(&case.body, case.body.capacity());
                }
            }
            Stmt::Label { name, body } => {
                self.string(name);
                self.boxed_statement(body);
            }
            Stmt::Break(label) | Stmt::Continue(label) => {
                if let Some(label) = label {
                    self.string(label);
                }
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                self.boxed_statement(body);
                if let Some((name, catch)) = catch {
                    self.string(name);
                    self.boxed_statement(catch);
                }
                if let Some(finally) = finally {
                    self.boxed_statement(finally);
                }
            }
        }
    }

    fn expression(&mut self, expression: &Expr) {
        match expression {
            Expr::Undefined | Expr::Null | Expr::Bool(_) | Expr::Number(_) | Expr::This => {}
            Expr::String(units) => self.add(buffer_bytes::<u16>(units.capacity())),
            Expr::RegExp { pattern, flags } => {
                self.add(buffer_bytes::<u16>(pattern.capacity()));
                self.string(flags);
            }
            Expr::Ident(name) => self.string(name),
            Expr::Array(items) => {
                self.add(buffer_bytes::<Option<Expr>>(items.capacity()));
                for item in items.iter().flatten() {
                    self.expression(item);
                }
            }
            Expr::Object(properties) => {
                self.add(buffer_bytes::<(String, Expr)>(properties.capacity()));
                for (name, value) in properties {
                    self.string(name);
                    self.expression(value);
                }
            }
            Expr::Function { name, params, body } => self.function(name.as_ref(), params, body),
            Expr::Unary { expr, .. } | Expr::Update { expr, .. } => {
                // Canonical operator tags are static; their real boxed children
                // and any separately owned descendants still pay in full.
                self.boxed_expression(expr);
            }
            Expr::Binary { left, right, .. } | Expr::Assign { left, right, .. } => {
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
            Expr::Sequence(items) => self.expressions(items, items.capacity()),
            Expr::Member { object, property } => {
                self.boxed_expression(object);
                self.boxed_expression(property);
            }
            Expr::Call { callee, args } | Expr::New { callee, args } => {
                self.boxed_expression(callee);
                self.expressions(args, args.capacity());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::rc::Rc;

    fn statement_payload(statement: &Stmt) -> usize {
        let mut storage = Storage::default();
        storage.statement(statement);
        storage.0
    }

    fn number() -> Expr {
        Expr::Number(0.0)
    }

    fn boxed_number() -> Box<Expr> {
        Box::new(number())
    }

    fn empty_statement() -> Box<Stmt> {
        Box::new(Stmt::Empty)
    }

    #[test]
    fn capacity_header_and_accumulation_arithmetic_saturate() {
        assert_eq!(buffer_bytes::<u8>(0), 0);
        assert_eq!(buffer_bytes::<u16>(3), 6 + ALLOCATION_OVERHEAD);
        assert_eq!(buffer_bytes::<u8>(usize::MAX), usize::MAX);
        assert_eq!(buffer_bytes::<Stmt>(usize::MAX), usize::MAX);
        assert_eq!(shared_bytes::<Stmt>(usize::MAX), usize::MAX);
        assert_eq!(aligned(usize::MAX, 8), usize::MAX);
        assert_eq!(aligned(17, 8), 24);
        let mut storage = Storage(usize::MAX - 2);
        storage.add(3);
        storage.add(0);
        assert_eq!(storage.0, usize::MAX);

        #[repr(align(64))]
        struct Aligned([u8; 64]);
        let element = Aligned([0; 64]);
        assert_eq!(element.0[0], 0);
        assert_eq!(shared_bytes::<Aligned>(0), 64 + ALLOCATION_OVERHEAD);
        assert_eq!(shared_bytes::<Aligned>(1), 128 + ALLOCATION_OVERHEAD);
    }

    #[test]
    fn root_slots_holes_and_spare_capacity_are_charged_once() {
        assert_eq!(program_bytes(&Program(vec![])), 0);
        for primitive in [
            Expr::Undefined,
            Expr::Null,
            Expr::Bool(true),
            number(),
            Expr::This,
        ] {
            assert_eq!(expression_bytes(&primitive), 0);
            assert_eq!(statement_payload(&Stmt::Expr(primitive)), 0);
        }
        let mut statements = Vec::with_capacity(9);
        statements.push(Stmt::Expr(number()));
        let expected = statements.capacity() * size_of::<Stmt>() + ALLOCATION_OVERHEAD;
        assert_eq!(program_bytes(&Program(statements)), expected);

        let mut items = Vec::with_capacity(17);
        let expected = items.capacity() * size_of::<Option<Expr>>() + ALLOCATION_OVERHEAD;
        items.extend([None, Some(number()), None]);
        assert_eq!(expression_bytes(&Expr::Array(items)), expected);
        let items = Vec::with_capacity(23);
        let expected = items.capacity() * size_of::<Option<Expr>>() + ALLOCATION_OVERHEAD;
        assert_eq!(expression_bytes(&Expr::Array(items)), expected);
    }

    #[test]
    fn string_identifier_and_regexp_storage_uses_capacity() {
        let mut units = Vec::with_capacity(31);
        units.extend([0xd800, b'a' as u16]);
        let expected = units.capacity() * 2 + ALLOCATION_OVERHEAD;
        assert_eq!(expression_bytes(&Expr::String(units)), expected);
        let mut name = String::with_capacity(37);
        name.push('x');
        let expected = name.capacity() + ALLOCATION_OVERHEAD;
        assert_eq!(expression_bytes(&Expr::Ident(name)), expected);
        let pattern = Vec::with_capacity(41);
        let flags = String::with_capacity(43);
        let expected = pattern.capacity() * 2 + flags.capacity() + 2 * ALLOCATION_OVERHEAD;
        assert_eq!(expression_bytes(&Expr::RegExp { pattern, flags }), expected);
        assert_eq!(expression_bytes(&Expr::String(vec![])), 0);
        assert_eq!(expression_bytes(&Expr::Ident(String::new())), 0);
    }

    #[test]
    fn expression_boxes_operators_and_argument_vectors_have_distinct_storage() {
        let slot = size_of::<Expr>() + ALLOCATION_OVERHEAD;
        for expression in [
            Expr::Unary {
                op: "+",
                expr: boxed_number(),
            },
            Expr::Update {
                op: "++",
                expr: boxed_number(),
                prefix: true,
            },
        ] {
            assert_eq!(expression_bytes(&expression), slot);
        }
        for expression in [
            Expr::Binary {
                op: "+",
                left: boxed_number(),
                right: boxed_number(),
            },
            Expr::Assign {
                op: "+=",
                left: boxed_number(),
                right: boxed_number(),
            },
        ] {
            assert_eq!(expression_bytes(&expression), 2 * slot);
        }
        assert_eq!(
            expression_bytes(&Expr::Member {
                object: boxed_number(),
                property: boxed_number(),
            }),
            2 * slot
        );
        assert_eq!(
            expression_bytes(&Expr::Conditional {
                test: boxed_number(),
                consequent: boxed_number(),
                alternate: boxed_number(),
            }),
            3 * slot
        );
        for constructor in [false, true] {
            let mut args = Vec::with_capacity(11);
            args.push(number());
            let expected = slot + args.capacity() * size_of::<Expr>() + ALLOCATION_OVERHEAD;
            let expression = if constructor {
                Expr::New {
                    callee: boxed_number(),
                    args,
                }
            } else {
                Expr::Call {
                    callee: boxed_number(),
                    args,
                }
            };
            assert_eq!(expression_bytes(&expression), expected);
        }
        let mut items = Vec::with_capacity(13);
        items.extend([number(), Expr::Null]);
        let expected = items.capacity() * size_of::<Expr>() + ALLOCATION_OVERHEAD;
        assert_eq!(expression_bytes(&Expr::Sequence(items)), expected);
    }

    #[test]
    fn static_operator_spelling_does_not_discount_owned_descendants() {
        let slot = size_of::<Expr>() + ALLOCATION_OVERHEAD;
        for op in ["+", "typeof"] {
            let mut name = String::with_capacity(73);
            name.push('x');
            let expected = slot + name.capacity() + ALLOCATION_OVERHEAD;
            let expression = Expr::Unary {
                op,
                expr: Box::new(Expr::Ident(name)),
            };
            assert_eq!(expression_bytes(&expression), expected);
        }
        for op in ["+", "instanceof"] {
            let mut units = Vec::with_capacity(31);
            units.extend([0xd800, b'a' as u16]);
            let mut name = String::with_capacity(43);
            name.push('x');
            let expected =
                2 * slot + units.capacity() * 2 + name.capacity() + 2 * ALLOCATION_OVERHEAD;
            let expression = Expr::Binary {
                op,
                left: Box::new(Expr::String(units)),
                right: Box::new(Expr::Ident(name)),
            };
            assert_eq!(expression_bytes(&expression), expected);
        }
    }

    #[test]
    fn variable_and_object_tuples_include_inline_slots_and_owned_names() {
        let mut name = String::with_capacity(19);
        name.push('v');
        let units = Vec::with_capacity(23);
        let child = name.capacity() + units.capacity() * 2 + 2 * ALLOCATION_OVERHEAD;
        let mut bindings = Vec::with_capacity(7);
        bindings.push((name, Some(Expr::String(units))));
        let expected =
            bindings.capacity() * size_of::<(String, Option<Expr>)>() + ALLOCATION_OVERHEAD + child;
        assert_eq!(statement_payload(&Stmt::Var(bindings)), expected);

        let mut name = String::with_capacity(29);
        name.push('k');
        let child = name.capacity() + ALLOCATION_OVERHEAD;
        let mut properties = Vec::with_capacity(5);
        properties.push((name, number()));
        let expected =
            properties.capacity() * size_of::<(String, Expr)>() + ALLOCATION_OVERHEAD + child;
        assert_eq!(expression_bytes(&Expr::Object(properties)), expected);
    }

    #[test]
    fn function_slices_include_empty_controls_and_shared_body_payload_once() {
        let params: Rc<[String]> = Vec::new().into();
        let body: Rc<[Stmt]> = Vec::new().into();
        let controls = 2 * (2 * size_of::<usize>() + ALLOCATION_OVERHEAD);
        assert_eq!(
            expression_bytes(&Expr::Function {
                name: None,
                params,
                body
            }),
            controls
        );

        let name = String::with_capacity(17);
        let parameter = String::with_capacity(19);
        let units = Vec::with_capacity(23);
        let expected = name.capacity()
            + parameter.capacity()
            + units.capacity() * 2
            + 3 * ALLOCATION_OVERHEAD
            + controls
            + size_of::<String>()
            + size_of::<Stmt>();
        let params: Rc<[String]> = vec![parameter].into();
        let body: Rc<[Stmt]> = vec![Stmt::Return(Some(Expr::String(units)))].into();
        let function = Expr::Function {
            name: Some(name),
            params,
            body,
        };
        assert_eq!(expression_bytes(&function), expected);
        if let Expr::Function { name, params, body } = &function {
            assert_eq!(Rc::strong_count(params), 1);
            assert_eq!(Rc::strong_count(body), 1);
            let mut storage = Storage::default();
            storage.function(name.as_ref(), params, body);
            assert_eq!(storage.0, expected);
            assert_eq!(Rc::strong_count(params), 1);
            assert_eq!(Rc::strong_count(body), 1);
        }
        let function = Stmt::Function {
            name: String::new(),
            params: vec![].into(),
            body: vec![].into(),
        };
        assert_eq!(statement_payload(&function), controls);
    }

    #[test]
    fn control_statements_count_boxes_without_recharging_inline_expressions() {
        let slot = size_of::<Stmt>() + ALLOCATION_OVERHEAD;
        for statement in [
            Stmt::Empty,
            Stmt::Expr(number()),
            Stmt::Throw(number()),
            Stmt::Return(None),
            Stmt::Return(Some(number())),
            Stmt::Break(None),
            Stmt::Continue(None),
        ] {
            assert_eq!(statement_payload(&statement), 0);
        }
        for statement in [
            Stmt::While {
                test: number(),
                body: empty_statement(),
            },
            Stmt::DoWhile {
                test: number(),
                body: empty_statement(),
            },
            Stmt::If {
                test: number(),
                consequent: empty_statement(),
                alternate: None,
            },
            Stmt::Try {
                body: empty_statement(),
                catch: None,
                finally: None,
            },
        ] {
            assert_eq!(statement_payload(&statement), slot);
        }
        assert_eq!(
            statement_payload(&Stmt::If {
                test: number(),
                consequent: empty_statement(),
                alternate: Some(empty_statement()),
            }),
            2 * slot
        );
        for is_continue in [false, true] {
            let name = String::with_capacity(17);
            let expected = name.capacity() + ALLOCATION_OVERHEAD;
            let statement = if is_continue {
                Stmt::Continue(Some(name))
            } else {
                Stmt::Break(Some(name))
            };
            assert_eq!(statement_payload(&statement), expected);
        }
        let name = String::with_capacity(19);
        let expected = name.capacity() + ALLOCATION_OVERHEAD + slot;
        assert_eq!(
            statement_payload(&Stmt::Label {
                name,
                body: empty_statement()
            }),
            expected
        );
        let name = String::with_capacity(23);
        let expected = name.capacity() + ALLOCATION_OVERHEAD + 3 * slot;
        assert_eq!(
            statement_payload(&Stmt::Try {
                body: empty_statement(),
                catch: Some((name, empty_statement())),
                finally: Some(empty_statement()),
            }),
            expected
        );
        let mut body = Vec::with_capacity(7);
        body.push(Stmt::Empty);
        let expected = body.capacity() * size_of::<Stmt>() + ALLOCATION_OVERHEAD;
        assert_eq!(statement_payload(&Stmt::Block(body)), expected);
    }

    #[test]
    fn selective_loop_boxes_are_paid_only_when_present() {
        let statement_slot = size_of::<Stmt>() + ALLOCATION_OVERHEAD;
        let expression_slot = size_of::<Expr>() + ALLOCATION_OVERHEAD;
        assert_eq!(
            statement_payload(&Stmt::For {
                init: None,
                test: None,
                update: None,
                body: empty_statement(),
            }),
            statement_slot
        );
        assert_eq!(
            statement_payload(&Stmt::For {
                init: Some(empty_statement()),
                test: Some(boxed_number()),
                update: Some(boxed_number()),
                body: empty_statement(),
            }),
            2 * statement_slot + 2 * expression_slot
        );
        let binding_slot = size_of::<ForInBinding>() + ALLOCATION_OVERHEAD;
        assert_eq!(
            statement_payload(&Stmt::ForIn {
                binding: Box::new(ForInBinding::Reference(number())),
                object: number(),
                body: empty_statement(),
            }),
            binding_slot + statement_slot
        );
        let name = String::with_capacity(17);
        let units = Vec::with_capacity(19);
        let expected = binding_slot
            + statement_slot
            + name.capacity()
            + units.capacity() * 2
            + 2 * ALLOCATION_OVERHEAD;
        assert_eq!(
            statement_payload(&Stmt::ForIn {
                binding: Box::new(ForInBinding::Var {
                    name,
                    init: Some(Expr::String(units))
                }),
                object: number(),
                body: empty_statement(),
            }),
            expected
        );
    }

    #[test]
    fn switch_slots_and_each_case_body_preserve_spare_capacity() {
        let mut first = Vec::with_capacity(5);
        first.push(Stmt::Empty);
        let second = Vec::with_capacity(7);
        let bodies =
            (first.capacity() + second.capacity()) * size_of::<Stmt>() + 2 * ALLOCATION_OVERHEAD;
        let mut cases = Vec::with_capacity(11);
        cases.push(SwitchCase {
            test: Some(number()),
            body: first,
        });
        cases.push(SwitchCase {
            test: None,
            body: second,
        });
        let expected = cases.capacity() * size_of::<SwitchCase>() + ALLOCATION_OVERHEAD + bodies;
        assert_eq!(
            statement_payload(&Stmt::Switch {
                discriminant: number(),
                cases
            }),
            expected
        );
    }

    #[test]
    fn parsed_nested_functions_can_be_measured_repeatedly_without_mutation() {
        let source = format!("{}return 1;{}", "function f(){".repeat(30), "}".repeat(30));
        let program = super::super::syntax::parse(&source).unwrap();
        let before = format!("{program:?}");
        let first = program_bytes(&program);
        assert!(first > 30 * size_of::<Stmt>());
        assert_eq!(program_bytes(&program), first);
        assert_eq!(format!("{program:?}"), before);
        drop(program);
    }
}
