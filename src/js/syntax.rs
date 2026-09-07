//! Original bounded classic-script lexer and parser.
//!
//! This initial ES5-shaped subset uses ASCII identifiers (including ASCII
//! Unicode escapes) and UTF-16 strings. Strict mode, regular expressions,
//! accessors, labels, for-in, and newer language syntax are explicit errors.

use super::{Expr, Program, Stmt};

const MAX_SOURCE: usize = 1024 * 1024;
const MAX_TOKENS: usize = 100_000;
const MAX_NODES: usize = 100_000;
const MAX_DEPTH: usize = 128;

#[derive(Clone, Debug, PartialEq)]
enum Kind {
    Word(String),
    Number(f64),
    String(Vec<u16>),
    Punct(&'static str),
    End,
}

#[derive(Clone, Debug)]
struct Token {
    kind: Kind,
    offset: usize,
    newline: bool,
}

struct Lexer<'a> {
    source: &'a str,
    pos: usize,
}

fn error(offset: usize, message: &str) -> String {
    format!("{message} at byte {offset}")
}

fn line_terminator(ch: char) -> bool {
    matches!(ch, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

fn whitespace(ch: char) -> bool {
    matches!(
        ch,
        '\t' | '\u{b}' | '\u{c}' | ' ' | '\u{a0}' | '\u{1680}' | '\u{180e}' | '\u{2000}'
            ..='\u{200a}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}'
    )
}

fn identifier_start(ch: char) -> bool {
    ch.is_ascii_alphabetic() || matches!(ch, '_' | '$')
}

fn identifier_part(ch: char) -> bool {
    identifier_start(ch) || ch.is_ascii_digit()
}

fn reserved(word: &str) -> bool {
    matches!(
        word,
        "break"
            | "case"
            | "catch"
            | "continue"
            | "debugger"
            | "default"
            | "delete"
            | "do"
            | "else"
            | "finally"
            | "for"
            | "function"
            | "if"
            | "in"
            | "instanceof"
            | "new"
            | "return"
            | "switch"
            | "this"
            | "throw"
            | "try"
            | "typeof"
            | "var"
            | "void"
            | "while"
            | "with"
            | "null"
            | "true"
            | "false"
            | "class"
            | "const"
            | "enum"
            | "export"
            | "extends"
            | "import"
            | "super"
            | "implements"
            | "interface"
            | "let"
            | "package"
            | "private"
            | "protected"
            | "public"
            | "static"
            | "yield"
            | "await"
    )
}

impl<'a> Lexer<'a> {
    fn peek(&self) -> Option<char> {
        self.source[self.pos..].chars().next()
    }

    fn take(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.pos += ch.len_utf8();
        Some(ch)
    }

    fn space_and_comments(&mut self) -> Result<bool, String> {
        let mut newline = false;
        loop {
            if self
                .peek()
                .is_some_and(|ch| whitespace(ch) || line_terminator(ch))
            {
                newline |= line_terminator(self.take().unwrap());
            } else if self.source[self.pos..].starts_with("//") {
                self.pos += 2;
                while self.peek().is_some_and(|ch| !line_terminator(ch)) {
                    self.take();
                }
            } else if self.source[self.pos..].starts_with("/*") {
                let start = self.pos;
                self.pos += 2;
                loop {
                    if self.source[self.pos..].starts_with("*/") {
                        self.pos += 2;
                        break;
                    }
                    let ch = self
                        .take()
                        .ok_or_else(|| error(start, "Unterminated block comment"))?;
                    newline |= line_terminator(ch);
                }
            } else {
                return Ok(newline);
            }
        }
    }

    fn hex(&mut self, digits: usize) -> Result<u16, String> {
        let start = self.pos;
        let mut value = 0u16;
        for _ in 0..digits {
            let ch = self
                .take()
                .ok_or_else(|| error(start, "Incomplete hexadecimal escape"))?;
            let digit = ch
                .to_digit(16)
                .ok_or_else(|| error(self.pos - ch.len_utf8(), "Invalid hexadecimal escape"))?;
            value = value * 16 + digit as u16;
        }
        Ok(value)
    }

    fn word(&mut self) -> Result<Kind, String> {
        let start = self.pos;
        let mut value = String::new();
        let mut escaped = false;
        loop {
            let ch = if self.peek() == Some('\\') {
                escaped = true;
                self.pos += 1;
                if self.take() != Some('u') {
                    return Err(error(
                        self.pos,
                        "Only Unicode escapes are supported in identifiers",
                    ));
                }
                char::from_u32(u32::from(self.hex(4)?))
                    .ok_or_else(|| error(start, "Invalid identifier escape"))?
            } else if self.peek().is_some_and(identifier_part) {
                self.take().unwrap()
            } else {
                break;
            };
            if !(if value.is_empty() {
                identifier_start(ch)
            } else {
                identifier_part(ch)
            }) {
                return Err(error(start, "Only ASCII identifiers are supported"));
            }
            value.push(ch);
        }
        if escaped && reserved(&value) {
            return Err(error(start, "Escaped reserved words are not identifiers"));
        }
        Ok(Kind::Word(value))
    }

    fn number(&mut self) -> Result<Kind, String> {
        let start = self.pos;
        let number;
        if self.source[self.pos..].starts_with("0x") || self.source[self.pos..].starts_with("0X") {
            self.pos += 2;
            let begin = self.pos;
            while self.peek().is_some_and(|ch| ch.is_ascii_hexdigit()) {
                self.take();
            }
            if begin == self.pos {
                return Err(error(start, "Hexadecimal literal needs a digit"));
            }
            number = hexadecimal_number(&self.source[begin..self.pos]);
        } else {
            while self.peek().is_some_and(|ch| ch.is_ascii_digit()) {
                self.take();
            }
            if self.pos - start > 1 && self.source.as_bytes()[start] == b'0' {
                return Err(error(
                    start,
                    "Legacy leading-zero numeric literals are unsupported",
                ));
            }
            if self.peek() == Some('.') {
                self.pos += 1;
                while self.peek().is_some_and(|ch| ch.is_ascii_digit()) {
                    self.take();
                }
            }
            if matches!(self.peek(), Some('e' | 'E')) {
                self.pos += 1;
                if matches!(self.peek(), Some('+' | '-')) {
                    self.pos += 1;
                }
                let begin = self.pos;
                while self.peek().is_some_and(|ch| ch.is_ascii_digit()) {
                    self.take();
                }
                if begin == self.pos {
                    return Err(error(begin, "Exponent needs a decimal digit"));
                }
            }
            number = self.source[start..self.pos]
                .parse()
                .map_err(|_| error(start, "Invalid number literal"))?;
        }
        if self
            .peek()
            .is_some_and(|ch| identifier_start(ch) || ch == '\\' || ch.is_ascii_digit())
        {
            return Err(error(
                self.pos,
                "Identifier or unsupported numeric suffix immediately follows number",
            ));
        }
        Ok(Kind::Number(number))
    }

    fn string(&mut self) -> Result<Kind, String> {
        let start = self.pos;
        let quote = self.take().unwrap();
        let mut value = Vec::new();
        loop {
            let ch = self
                .take()
                .ok_or_else(|| error(start, "Unterminated string literal"))?;
            if ch == quote {
                return Ok(Kind::String(value));
            }
            if line_terminator(ch) {
                return Err(error(
                    self.pos - ch.len_utf8(),
                    "Unescaped line terminator in string",
                ));
            }
            if ch != '\\' {
                value.extend_from_slice(ch.encode_utf16(&mut [0; 2]));
                continue;
            }
            let escaped = self
                .take()
                .ok_or_else(|| error(start, "Unterminated string escape"))?;
            match escaped {
                'b' => value.push(8),
                'f' => value.push(12),
                'n' => value.push(10),
                'r' => value.push(13),
                't' => value.push(9),
                'v' => value.push(11),
                '0' if !self.peek().is_some_and(|ch| ch.is_ascii_digit()) => value.push(0),
                '0'..='9' => {
                    return Err(error(
                        self.pos - 1,
                        "Legacy octal and decimal string escapes are unsupported",
                    ));
                }
                'x' => value.push(self.hex(2)?),
                'u' => value.push(self.hex(4)?),
                '\r' => {
                    if self.peek() == Some('\n') {
                        self.pos += 1;
                    }
                }
                '\n' | '\u{2028}' | '\u{2029}' => {}
                other => value.extend_from_slice(other.encode_utf16(&mut [0; 2])),
            }
        }
    }

    fn tokens(mut self) -> Result<Vec<Token>, String> {
        let mut tokens = Vec::new();
        loop {
            let newline = self.space_and_comments()?;
            let offset = self.pos;
            let Some(ch) = self.peek() else {
                tokens.push(Token {
                    kind: Kind::End,
                    offset,
                    newline,
                });
                return Ok(tokens);
            };
            if tokens.len() >= MAX_TOKENS {
                return Err(error(offset, "Token limit exceeded"));
            }
            let kind = if identifier_start(ch) || ch == '\\' {
                self.word()?
            } else if ch.is_ascii_digit()
                || ch == '.'
                    && self.source[self.pos + 1..]
                        .chars()
                        .next()
                        .is_some_and(|ch| ch.is_ascii_digit())
            {
                self.number()?
            } else if matches!(ch, '\'' | '"') {
                self.string()?
            } else if ch == '`' {
                return Err(error(offset, "Template literals are unsupported"));
            } else {
                let rest = &self.source[self.pos..];
                if ["=>", "...", "**", "??", "&&=", "||="]
                    .iter()
                    .any(|operator| rest.starts_with(operator))
                    || rest.starts_with("?.")
                        && !rest.as_bytes().get(2).is_some_and(u8::is_ascii_digit)
                {
                    return Err(error(offset, "Unsupported modern JavaScript operator"));
                }
                let operator = [
                    ">>>=", "===", "!==", ">>>", "<<=", ">>=", "++", "--", "&&", "||", "==", "!=",
                    "<=", ">=", "<<", ">>", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "{",
                    "}", "(", ")", "[", "]", ".", ";", ",", ":", "?", "+", "-", "*", "/", "%", "~",
                    "!", "=", "<", ">", "&", "|", "^",
                ]
                .into_iter()
                .find(|operator| rest.starts_with(operator))
                .ok_or_else(|| error(offset, "Unsupported character or non-ASCII identifier"))?;
                self.pos += operator.len();
                Kind::Punct(operator)
            };
            tokens.push(Token {
                kind,
                offset,
                newline,
            });
        }
    }
}

struct E {
    value: Expr,
    depth: usize,
}

struct S {
    value: Stmt,
    depth: usize,
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    nesting: usize,
    nodes: usize,
    functions: usize,
    loops: usize,
}

/// Parse the entire source or return a byte-positioned error. No successful
/// prefix is returned after a lexical, syntax, unsupported-feature, or limit error.
pub fn parse(source: &str) -> Result<Program, String> {
    if source.len() > MAX_SOURCE {
        return Err(error(MAX_SOURCE, "Source exceeds one MiB"));
    }
    let tokens = Lexer { source, pos: 0 }.tokens()?;
    let mut parser = Parser {
        tokens,
        pos: 0,
        nesting: 0,
        nodes: 0,
        functions: 0,
        loops: 0,
    };
    let (body, _) = parser.body(false, true)?;
    Ok(Program(body))
}

impl Parser {
    fn token(&self) -> &Token {
        &self.tokens[self.pos]
    }
    fn fail(&self, message: &str) -> String {
        error(self.token().offset, message)
    }
    fn punct(&self, value: &str) -> bool {
        matches!(&self.token().kind, Kind::Punct(actual) if *actual == value)
    }
    fn word(&self, value: &str) -> bool {
        matches!(&self.token().kind, Kind::Word(actual) if actual == value)
    }
    fn end(&self) -> bool {
        matches!(self.token().kind, Kind::End)
    }
    fn eat(&mut self, value: &str) -> bool {
        if self.punct(value) {
            self.pos += 1;
            true
        } else {
            false
        }
    }
    fn eat_word(&mut self, value: &str) -> bool {
        if self.word(value) {
            self.pos += 1;
            true
        } else {
            false
        }
    }
    fn expect(&mut self, value: &str) -> Result<(), String> {
        if self.eat(value) {
            Ok(())
        } else {
            Err(self.fail(&format!("Expected {value}")))
        }
    }
    fn nested<T>(
        &mut self,
        operation: impl FnOnce(&mut Self) -> Result<T, String>,
    ) -> Result<T, String> {
        if self.nesting >= MAX_DEPTH {
            return Err(self.fail("Parser nesting limit exceeded"));
        }
        self.nesting += 1;
        let result = operation(self);
        self.nesting -= 1;
        result
    }
    fn node(&mut self, depth: usize) -> Result<(), String> {
        if depth > MAX_DEPTH {
            return Err(self.fail("AST depth limit exceeded"));
        }
        if self.nodes >= MAX_NODES {
            return Err(self.fail("AST node limit exceeded"));
        }
        self.nodes += 1;
        Ok(())
    }
    fn expr(&mut self, value: Expr, depth: usize) -> Result<E, String> {
        self.node(depth)?;
        Ok(E { value, depth })
    }
    fn stmt(&mut self, value: Stmt, depth: usize) -> Result<S, String> {
        self.node(depth)?;
        Ok(S { value, depth })
    }
    fn name(&mut self, property: bool) -> Result<String, String> {
        if let Kind::Word(name) = &self.token().kind {
            if property || !reserved(name) {
                let name = name.clone();
                self.pos += 1;
                return Ok(name);
            }
        }
        Err(self.fail(
            "Expected an identifier; reserved words and modern binding syntax are unsupported",
        ))
    }
    fn semicolon(&mut self) -> Result<(), String> {
        if self.eat(";") || self.end() || self.punct("}") || self.token().newline {
            Ok(())
        } else {
            Err(self.fail("Expected semicolon or a valid automatic-semicolon boundary"))
        }
    }

    fn body(&mut self, braced: bool, declarations: bool) -> Result<(Vec<Stmt>, usize), String> {
        let mut body = Vec::new();
        let mut depth = 0;
        let mut directive = declarations;
        while !self.end() && !(braced && self.punct("}")) {
            let direct_string = matches!(self.token().kind, Kind::String(_));
            let offset = self.token().offset;
            let statement = self.statement(declarations)?;
            if directive && direct_string {
                if let Stmt::Expr(Expr::String(value)) = &statement.value {
                    if value.as_slice() == "use strict".encode_utf16().collect::<Vec<_>>() {
                        return Err(error(offset, "Strict-mode directives are unsupported"));
                    }
                } else {
                    directive = false;
                }
            } else {
                directive = false;
            }
            depth = depth.max(statement.depth);
            body.push(statement.value);
        }
        if braced {
            self.expect("}")?;
        }
        Ok((body, depth))
    }

    fn block(&mut self) -> Result<S, String> {
        self.expect("{")?;
        self.nested(|parser| {
            let (body, depth) = parser.body(true, false)?;
            parser.stmt(Stmt::Block(body), depth + 1)
        })
    }

    fn statement(&mut self, declarations: bool) -> Result<S, String> {
        self.nested(|parser| parser.statement_inner(declarations))
    }

    fn statement_inner(&mut self, declarations: bool) -> Result<S, String> {
        if self.eat(";") {
            return self.stmt(Stmt::Empty, 1);
        }
        if self.punct("{") {
            return self.block();
        }
        if self.eat_word("var") {
            let statement = self.variables(true)?;
            self.semicolon()?;
            return Ok(statement);
        }
        if self.eat_word("function") {
            if !declarations {
                return Err(self
                    .fail("Function declarations in nested statement positions are unsupported"));
            }
            let (name, params, body, depth) = self.function(true)?;
            return self.stmt(
                Stmt::Function {
                    name: name.unwrap(),
                    params,
                    body,
                },
                depth + 1,
            );
        }
        if self.eat_word("return") {
            if self.functions == 0 {
                return Err(self.fail("return outside a function"));
            }
            if self.token().newline || self.punct(";") || self.punct("}") || self.end() {
                self.semicolon()?;
                return self.stmt(Stmt::Return(None), 1);
            }
            let value = self.expression(true)?;
            self.semicolon()?;
            return self.stmt(Stmt::Return(Some(value.value)), value.depth + 1);
        }
        if self.eat_word("throw") {
            if self.token().newline {
                return Err(self.fail("Line terminator after throw is invalid"));
            }
            let value = self.expression(true)?;
            self.semicolon()?;
            return self.stmt(Stmt::Throw(value.value), value.depth + 1);
        }
        if self.word("break") || self.word("continue") {
            let is_break = self.eat_word("break");
            if !is_break {
                self.pos += 1;
            }
            if self.loops == 0 {
                return Err(self.fail("break or continue outside a loop"));
            }
            if !self.token().newline && matches!(self.token().kind, Kind::Word(_)) {
                return Err(self.fail("Labeled break and continue are unsupported"));
            }
            self.semicolon()?;
            return self.stmt(
                if is_break {
                    Stmt::Break
                } else {
                    Stmt::Continue
                },
                1,
            );
        }
        if self.eat_word("if") {
            let test = self.condition()?;
            let consequent = self.statement(false)?;
            let alternate = if self.eat_word("else") {
                Some(self.statement(false)?)
            } else {
                None
            };
            let depth = test
                .depth
                .max(consequent.depth)
                .max(alternate.as_ref().map_or(0, |s| s.depth))
                + 1;
            return self.stmt(
                Stmt::If {
                    test: test.value,
                    consequent: Box::new(consequent.value),
                    alternate: alternate.map(|s| Box::new(s.value)),
                },
                depth,
            );
        }
        if self.eat_word("while") {
            let test = self.condition()?;
            let body = self.loop_body()?;
            let depth = test.depth.max(body.depth) + 1;
            return self.stmt(
                Stmt::While {
                    test: test.value,
                    body: Box::new(body.value),
                },
                depth,
            );
        }
        if self.eat_word("do") {
            let body = self.loop_body()?;
            if !self.eat_word("while") {
                return Err(self.fail("Expected while after do body"));
            }
            let test = self.condition()?;
            self.semicolon()?;
            let depth = test.depth.max(body.depth) + 1;
            return self.stmt(
                Stmt::DoWhile {
                    body: Box::new(body.value),
                    test: test.value,
                },
                depth,
            );
        }
        if self.eat_word("for") {
            return self.for_statement();
        }
        if self.eat_word("try") {
            let body = self.block()?;
            let catch = if self.eat_word("catch") {
                self.expect("(")?;
                let binding = self.name(false)?;
                self.expect(")")?;
                Some((binding, self.block()?))
            } else {
                None
            };
            let finally = if self.eat_word("finally") {
                Some(self.block()?)
            } else {
                None
            };
            if catch.is_none() && finally.is_none() {
                return Err(self.fail("try requires catch or finally"));
            }
            let depth = body
                .depth
                .max(catch.as_ref().map_or(0, |(_, s)| s.depth))
                .max(finally.as_ref().map_or(0, |s| s.depth))
                + 1;
            return self.stmt(
                Stmt::Try {
                    body: Box::new(body.value),
                    catch: catch.map(|(name, s)| (name, Box::new(s.value))),
                    finally: finally.map(|s| Box::new(s.value)),
                },
                depth,
            );
        }
        if let Kind::Word(word) = &self.token().kind {
            if matches!(
                word.as_str(),
                "let" | "const" | "class" | "import" | "export" | "switch" | "with" | "debugger"
            ) {
                return Err(self.fail("Unsupported declaration or statement"));
            }
        }
        let value = self.expression(true)?;
        if self.punct(":") {
            return Err(self.fail("Labeled statements are unsupported"));
        }
        self.semicolon()?;
        self.stmt(Stmt::Expr(value.value), value.depth + 1)
    }

    fn variables(&mut self, allow_in: bool) -> Result<S, String> {
        let mut variables = Vec::new();
        let mut depth = 0;
        loop {
            let name = self.name(false)?;
            let value = if self.eat("=") {
                Some(self.assignment(allow_in)?)
            } else {
                None
            };
            depth = depth.max(value.as_ref().map_or(0, |e| e.depth));
            variables.push((name, value.map(|e| e.value)));
            if !self.eat(",") {
                break;
            }
        }
        self.stmt(Stmt::Var(variables), depth + 1)
    }

    fn condition(&mut self) -> Result<E, String> {
        self.expect("(")?;
        let expression = self.expression(true)?;
        self.expect(")")?;
        Ok(expression)
    }

    fn loop_body(&mut self) -> Result<S, String> {
        self.loops += 1;
        let result = self.statement(false);
        self.loops -= 1;
        result
    }

    fn for_statement(&mut self) -> Result<S, String> {
        self.expect("(")?;
        let init = if self.punct(";") {
            None
        } else if self.eat_word("var") {
            Some(self.variables(false)?)
        } else {
            let expression = self.expression(false)?;
            Some(self.stmt(Stmt::Expr(expression.value), expression.depth + 1)?)
        };
        if self.word("in") || self.word("of") {
            return Err(self.fail("for-in and for-of are unsupported"));
        }
        self.expect(";")?;
        let test = if self.punct(";") {
            None
        } else {
            Some(self.expression(true)?)
        };
        self.expect(";")?;
        let update = if self.punct(")") {
            None
        } else {
            Some(self.expression(true)?)
        };
        self.expect(")")?;
        let body = self.loop_body()?;
        let depth = body
            .depth
            .max(init.as_ref().map_or(0, |s| s.depth))
            .max(test.as_ref().map_or(0, |e| e.depth))
            .max(update.as_ref().map_or(0, |e| e.depth))
            + 1;
        self.stmt(
            Stmt::For {
                init: init.map(|s| Box::new(s.value)),
                test: test.map(|e| e.value),
                update: update.map(|e| e.value),
                body: Box::new(body.value),
            },
            depth,
        )
    }

    fn function(
        &mut self,
        declaration: bool,
    ) -> Result<(Option<String>, Vec<String>, Vec<Stmt>, usize), String> {
        let name = if declaration || !self.punct("(") {
            Some(self.name(false)?)
        } else {
            None
        };
        self.expect("(")?;
        let mut params = Vec::new();
        if !self.punct(")") {
            loop {
                params.push(self.name(false)?);
                if !self.eat(",") {
                    break;
                }
            }
        }
        self.expect(")")?;
        self.expect("{")?;
        let previous_loops = self.loops;
        self.loops = 0;
        self.functions += 1;
        let result = self.nested(|parser| parser.body(true, true));
        self.functions -= 1;
        self.loops = previous_loops;
        let (body, depth) = result?;
        Ok((name, params, body, depth))
    }

    fn expression(&mut self, allow_in: bool) -> Result<E, String> {
        let first = self.assignment(allow_in)?;
        if !self.eat(",") {
            return Ok(first);
        }
        let mut depth = first.depth;
        let mut values = vec![first.value];
        loop {
            let expression = self.assignment(allow_in)?;
            depth = depth.max(expression.depth);
            values.push(expression.value);
            if !self.eat(",") {
                break;
            }
        }
        self.expr(Expr::Sequence(values), depth + 1)
    }

    fn assignment(&mut self, allow_in: bool) -> Result<E, String> {
        self.nested(|parser| {
            let left = parser.conditional(allow_in)?;
            let Kind::Punct(
                op @ ("=" | "+=" | "-=" | "*=" | "/=" | "%=" | "<<=" | ">>=" | ">>>=" | "&=" | "|="
                | "^="),
            ) = parser.token().kind
            else {
                return Ok(left);
            };
            if !assignable(&left.value) {
                return Err(parser.fail("Invalid assignment target"));
            }
            parser.pos += 1;
            let right = parser.assignment(allow_in)?;
            let depth = left.depth.max(right.depth) + 1;
            parser.expr(
                Expr::Assign {
                    op: op.to_owned(),
                    left: Box::new(left.value),
                    right: Box::new(right.value),
                },
                depth,
            )
        })
    }

    fn conditional(&mut self, allow_in: bool) -> Result<E, String> {
        let test = self.binary(1, allow_in)?;
        if !self.eat("?") {
            return Ok(test);
        }
        let consequent = self.assignment(true)?;
        self.expect(":")?;
        let alternate = self.assignment(allow_in)?;
        let depth = test.depth.max(consequent.depth).max(alternate.depth) + 1;
        self.expr(
            Expr::Conditional {
                test: Box::new(test.value),
                consequent: Box::new(consequent.value),
                alternate: Box::new(alternate.value),
            },
            depth,
        )
    }

    fn binary(&mut self, minimum: u8, allow_in: bool) -> Result<E, String> {
        let mut left = self.unary()?;
        loop {
            let Some((op, precedence)) = binary_operator(&self.token().kind, allow_in) else {
                break;
            };
            if precedence < minimum {
                break;
            }
            let op = op.to_owned();
            self.pos += 1;
            let right = self.binary(precedence + 1, allow_in)?;
            let depth = left.depth.max(right.depth) + 1;
            left = self.expr(
                Expr::Binary {
                    op,
                    left: Box::new(left.value),
                    right: Box::new(right.value),
                },
                depth,
            )?;
        }
        Ok(left)
    }

    fn unary(&mut self) -> Result<E, String> {
        self.nested(|parser| {
            let op = match &parser.token().kind {
                Kind::Punct(op @ ("+" | "-" | "!" | "~" | "++" | "--")) => Some((*op).to_owned()),
                Kind::Word(op) if matches!(op.as_str(), "typeof" | "void" | "delete") => {
                    Some(op.clone())
                }
                _ => None,
            };
            if let Some(op) = op {
                parser.pos += 1;
                let expression = parser.unary()?;
                let depth = expression.depth + 1;
                return if op == "++" || op == "--" {
                    if !assignable(&expression.value) {
                        return Err(parser.fail("Invalid update target"));
                    }
                    parser.expr(
                        Expr::Update {
                            op,
                            expr: Box::new(expression.value),
                            prefix: true,
                        },
                        depth,
                    )
                } else {
                    parser.expr(
                        Expr::Unary {
                            op,
                            expr: Box::new(expression.value),
                        },
                        depth,
                    )
                };
            }
            let expression = parser.left_hand_side()?;
            if !parser.token().newline && (parser.punct("++") || parser.punct("--")) {
                if !assignable(&expression.value) {
                    return Err(parser.fail("Invalid update target"));
                }
                let Kind::Punct(op) = parser.token().kind else {
                    unreachable!()
                };
                parser.pos += 1;
                let depth = expression.depth + 1;
                parser.expr(
                    Expr::Update {
                        op: op.to_owned(),
                        expr: Box::new(expression.value),
                        prefix: false,
                    },
                    depth,
                )
            } else {
                Ok(expression)
            }
        })
    }

    fn left_hand_side(&mut self) -> Result<E, String> {
        let mut expression = self.new_expression()?;
        loop {
            if self.punct("(") {
                let (args, args_depth) = self.arguments()?;
                let depth = expression.depth.max(args_depth) + 1;
                expression = self.expr(
                    Expr::Call {
                        callee: Box::new(expression.value),
                        args,
                    },
                    depth,
                )?;
            } else if self.punct(".") || self.punct("[") {
                expression = self.member(expression)?;
            } else {
                return Ok(expression);
            }
        }
    }

    fn new_expression(&mut self) -> Result<E, String> {
        self.nested(|parser| {
            let mut expression = if parser.eat_word("new") {
                let callee = parser.new_expression()?;
                let (args, args_depth) = if parser.punct("(") {
                    parser.arguments()?
                } else {
                    (Vec::new(), 0)
                };
                let depth = callee.depth.max(args_depth) + 1;
                parser.expr(
                    Expr::New {
                        callee: Box::new(callee.value),
                        args,
                    },
                    depth,
                )?
            } else {
                parser.primary()?
            };
            while parser.punct(".") || parser.punct("[") {
                expression = parser.member(expression)?;
            }
            Ok(expression)
        })
    }

    fn member(&mut self, object: E) -> Result<E, String> {
        let property = if self.eat(".") {
            let name = self.name(true)?;
            self.expr(Expr::String(name.encode_utf16().collect()), 1)?
        } else {
            self.expect("[")?;
            let property = self.expression(true)?;
            self.expect("]")?;
            property
        };
        let depth = object.depth.max(property.depth) + 1;
        self.expr(
            Expr::Member {
                object: Box::new(object.value),
                property: Box::new(property.value),
            },
            depth,
        )
    }

    fn arguments(&mut self) -> Result<(Vec<Expr>, usize), String> {
        self.expect("(")?;
        let mut args = Vec::new();
        let mut depth = 0;
        if !self.punct(")") {
            loop {
                let argument = self.assignment(true)?;
                depth = depth.max(argument.depth);
                args.push(argument.value);
                if !self.eat(",") {
                    break;
                }
            }
        }
        self.expect(")")?;
        Ok((args, depth))
    }

    fn primary(&mut self) -> Result<E, String> {
        let token = self.token().clone();
        match token.kind {
            Kind::Number(value) => {
                self.pos += 1;
                self.expr(Expr::Number(value), 1)
            }
            Kind::String(value) => {
                self.pos += 1;
                self.expr(Expr::String(value), 1)
            }
            Kind::Word(ref word) if word == "function" => {
                self.pos += 1;
                let (name, params, body, depth) = self.function(false)?;
                self.expr(Expr::Function { name, params, body }, depth + 1)
            }
            Kind::Word(word) => {
                self.pos += 1;
                let expression = match word.as_str() {
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
                self.expr(expression, 1)
            }
            Kind::Punct("(") => {
                self.pos += 1;
                let value = self.expression(true)?;
                self.expect(")")?;
                Ok(value)
            }
            Kind::Punct("[") => {
                self.pos += 1;
                let mut values = Vec::new();
                let mut depth = 0;
                while !self.punct("]") {
                    if self.eat(",") {
                        values.push(None);
                        continue;
                    }
                    let value = self.assignment(true)?;
                    depth = depth.max(value.depth);
                    values.push(Some(value.value));
                    if !self.eat(",") {
                        break;
                    }
                }
                self.expect("]")?;
                self.expr(Expr::Array(values), depth + 1)
            }
            Kind::Punct("{") => self.object(),
            Kind::Punct("/" | "/=") => Err(error(
                token.offset,
                "Regular expression literals are unsupported",
            )),
            Kind::End => Err(error(
                token.offset,
                "Unexpected end of source; expected expression",
            )),
            _ => Err(error(
                token.offset,
                "Expected expression or encountered unsupported syntax",
            )),
        }
    }

    fn object(&mut self) -> Result<E, String> {
        self.expect("{")?;
        let mut values = Vec::new();
        let mut depth = 0;
        while !self.punct("}") {
            let token = self.token().clone();
            let key = match token.kind {
                Kind::Word(name) => name,
                Kind::String(value) => String::from_utf16(&value).map_err(|_| {
                    error(
                        token.offset,
                        "Lone-surrogate object property names are unsupported",
                    )
                })?,
                Kind::Number(value) => number_property(value),
                _ => {
                    return Err(error(
                        token.offset,
                        "Expected object property name; computed properties are unsupported",
                    ));
                }
            };
            self.pos += 1;
            if !self.eat(":") {
                return Err(
                    self.fail("Expected colon; object accessors and shorthand are unsupported")
                );
            }
            let value = self.assignment(true)?;
            depth = depth.max(value.depth);
            values.push((key, value.value));
            if !self.eat(",") {
                break;
            }
        }
        self.expect("}")?;
        self.expr(Expr::Object(values), depth + 1)
    }
}

fn assignable(expression: &Expr) -> bool {
    matches!(expression, Expr::Ident(_) | Expr::Member { .. })
}

fn binary_operator(kind: &Kind, allow_in: bool) -> Option<(&str, u8)> {
    let op = match kind {
        Kind::Punct(op) => *op,
        Kind::Word(op) => op,
        _ => return None,
    };
    let precedence = match op {
        "||" => 1,
        "&&" => 2,
        "|" => 3,
        "^" => 4,
        "&" => 5,
        "==" | "!=" | "===" | "!==" => 6,
        "<" | ">" | "<=" | ">=" | "instanceof" => 7,
        "in" if allow_in => 7,
        "<<" | ">>" | ">>>" => 8,
        "+" | "-" => 9,
        "*" | "/" | "%" => 10,
        _ => return None,
    };
    Some((op, precedence))
}

fn number_property(value: f64) -> String {
    if value.is_infinite() {
        return "Infinity".to_owned();
    }
    if value != 0.0 && !(1e-6..1e21).contains(&value) {
        let value = format!("{value:e}");
        let (mantissa, exponent) = value.split_once('e').unwrap();
        return format!(
            "{mantissa}e{}{exponent}",
            if exponent.starts_with('-') { "" } else { "+" }
        );
    }
    value.to_string()
}

fn hexadecimal_number(digits: &str) -> f64 {
    let digits = digits.trim_start_matches('0');
    if digits.is_empty() {
        return 0.0;
    }
    if digits.len() <= 16 {
        return u64::from_str_radix(digits, 16).unwrap() as f64;
    }
    if digits.len() > 256 {
        return f64::INFINITY;
    }
    // Preserve 53 significant bits plus guard/sticky bits, then round once.
    // Accumulating hexadecimal digits directly in f64 can double-round.
    let mut bits = 0usize;
    let mut significand = 0u64;
    let mut guard = false;
    let mut sticky = false;
    for digit in digits.chars().map(|ch| ch.to_digit(16).unwrap()) {
        for shift in (0..4).rev() {
            let bit = (digit >> shift) & 1;
            if bits == 0 && bit == 0 {
                continue;
            }
            if bits < 53 {
                significand = (significand << 1) | u64::from(bit);
            } else if bits == 53 {
                guard = bit != 0;
            } else {
                sticky |= bit != 0;
            }
            bits += 1;
        }
    }
    if guard && (sticky || significand & 1 != 0) {
        significand += 1;
    }
    significand as f64 * 2.0f64.powi(bits as i32 - 53)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expression(source: &str) -> Expr {
        let Program(mut body) = parse(source).unwrap();
        assert_eq!(body.len(), 1);
        let Stmt::Expr(value) = body.remove(0) else {
            panic!("expected expression")
        };
        value
    }

    #[test]
    fn precedence_assignment_and_sequences() {
        let value = expression("a = b = 1 + 2 * 3");
        let Expr::Assign { left, right, op } = value else {
            panic!()
        };
        assert_eq!(op, "=");
        assert_eq!(*left, Expr::Ident("a".into()));
        let Expr::Assign { right, .. } = *right else {
            panic!()
        };
        let Expr::Binary { op, right, .. } = *right else {
            panic!()
        };
        assert_eq!(op, "+");
        assert!(matches!(*right, Expr::Binary { op, .. } if op == "*"));
        assert!(
            matches!(expression("a || b && c"), Expr::Binary { op, right, .. } if op == "||" && matches!(right.as_ref(), Expr::Binary { op, .. } if op == "&&"))
        );
        assert!(
            matches!(expression("a ? b = 1 : c ? 2 : 3"), Expr::Conditional { alternate, .. } if matches!(*alternate, Expr::Conditional { .. }))
        );
        assert!(matches!(expression("a(), b(), 3"), Expr::Sequence(values) if values.len() == 3));
        assert!(
            matches!(expression("8 - 3 - 1"), Expr::Binary { left, .. } if matches!(*left, Expr::Binary { .. }))
        );
    }

    #[test]
    fn iife_members_calls_new_and_this() {
        assert!(
            matches!(expression("(function named(a) { return this.x + a; })(2)"), Expr::Call { callee, args } if args == vec![Expr::Number(2.0)] && matches!(callee.as_ref(), Expr::Function { name: Some(name), .. } if name == "named"))
        );
        assert!(
            matches!(expression("new Thing(1).method(2)"), Expr::Call { callee, .. } if matches!(callee.as_ref(), Expr::Member { object, .. } if matches!(object.as_ref(), Expr::New { .. })))
        );
        assert!(
            matches!(expression("new ns.Thing(1)"), Expr::New { callee, args } if args.len() == 1 && matches!(*callee, Expr::Member { .. }))
        );
        assert!(
            matches!(expression("new Thing()()"), Expr::Call { callee, .. } if matches!(*callee, Expr::New { .. }))
        );
        assert!(
            matches!(expression("new new Thing()"), Expr::New { callee, .. } if matches!(*callee, Expr::New { .. }))
        );
    }

    #[test]
    fn utf16_strings_comments_and_numeric_forms() {
        let Expr::String(value) = expression("'🦀\\uD800\\uDC00\\uDFFF\\x41\\0\\\r\nZ'") else {
            panic!()
        };
        assert_eq!(value, [0xd83e, 0xdd80, 0xd800, 0xdc00, 0xdfff, 65, 0, 90]);
        assert_eq!(
            expression("/*before*/ .5 // middle\n + 0x10 + 1e2"),
            Expr::Binary {
                op: "+".into(),
                left: Box::new(Expr::Binary {
                    op: "+".into(),
                    left: Box::new(Expr::Number(0.5)),
                    right: Box::new(Expr::Number(16.0))
                }),
                right: Box::new(Expr::Number(100.0))
            }
        );
        assert!(matches!(
            expression("(function(undefined) { return undefined; })(1)"),
            Expr::Call { .. }
        ));
        assert_eq!(expression("v\\u0061lue"), Expr::Ident("value".into()));
        assert_eq!(
            expression("0x1000000000000081"),
            Expr::Number(0x1000000000000081u64 as f64)
        );
        assert_eq!(
            expression("0x1000000000000080001"),
            Expr::Number(0x1000000000000080001u128 as f64)
        );
    }

    #[test]
    fn array_holes_and_object_properties() {
        assert_eq!(
            expression("[,,1,,]"),
            Expr::Array(vec![None, None, Some(Expr::Number(1.0)), None])
        );
        assert_eq!(
            expression("({default: 1, 'a\\x62': 2, 0x10: 3,})"),
            Expr::Object(vec![
                ("default".into(), Expr::Number(1.0)),
                ("ab".into(), Expr::Number(2.0)),
                ("16".into(), Expr::Number(3.0))
            ])
        );
        assert!(
            parse("({'\\uD800':1})")
                .unwrap_err()
                .contains("Lone-surrogate")
        );
    }

    #[test]
    fn statements_control_context_and_asi() {
        let Program(body) = parse("function f(n) { var sum=0; for(var i=0;i<n;i++){if(i===2) continue; sum+=i;} try { if(sum) throw sum; } catch(e) { return e; } finally { sum++; } return\n42; } while(false){break;} do {} while(false);").unwrap();
        assert_eq!(body.len(), 3);
        let Stmt::Function { body, .. } = &body[0] else {
            panic!()
        };
        assert!(matches!(body[3], Stmt::Return(None)));
        assert!(matches!(body[4], Stmt::Expr(Expr::Number(42.0))));
        let Program(body) = parse("var x=1\nx\n++x").unwrap();
        assert_eq!(body.len(), 3);
        assert!(matches!(
            body[2],
            Stmt::Expr(Expr::Update { prefix: true, .. })
        ));
        assert!(parse("function f(){return /*\n*/ 3}").is_ok());
        assert!(parse("while(1){(function(){break;})()}").is_err());
    }

    #[test]
    fn invalid_and_unsupported_sources_never_return_a_prefix() {
        for source in [
            "var x=1; @",
            "return 1",
            "break;",
            "continue;",
            "throw\n1",
            "var ;",
            "var x =",
            "if (1)",
            "try {}",
            "function f(,){}",
            "(1+2)=3",
            "f()++",
            "let x=1",
            "const x=1",
            "class C {}",
            "import x from 'x'",
            "x=>x",
            "`hello`",
            "/abc/g",
            "for(var x in y){}",
            "switch(x){}",
            "'use strict'; x=1",
            "function f(){'use strict';}",
            "({get x(){return 1}})",
            "({x})",
            "1e+",
            "012",
            "'\\1'",
            "'\\u12zz'",
            "/* unfinished",
            "'unterminated",
        ] {
            let message = parse(source).expect_err(source);
            assert!(message.contains("byte"), "{source}: {message}");
        }
        assert!(parse("'é'; @").unwrap_err().ends_with("at byte 6"));
    }

    #[test]
    fn source_tokens_recursion_and_flat_ast_chains_are_bounded() {
        assert!(
            parse(&" ".repeat(MAX_SOURCE + 1))
                .unwrap_err()
                .contains("Source")
        );
        assert!(
            parse(&";".repeat(MAX_TOKENS + 1))
                .unwrap_err()
                .contains("Token")
        );
        assert!(
            parse(&format!("{}1{}", "(".repeat(200), ")".repeat(200)))
                .unwrap_err()
                .contains("nesting")
        );
        assert!(
            parse(&format!("a{}", "+1".repeat(1000)))
                .unwrap_err()
                .contains("depth")
        );
        assert!(
            parse(&format!("a{}", ".x".repeat(1000)))
                .unwrap_err()
                .contains("depth")
        );
        assert!(parse(&format!("{}1", "a=".repeat(1000))).is_err());
    }
}
