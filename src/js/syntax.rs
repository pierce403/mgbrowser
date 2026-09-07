//! Original bounded classic-script lexer and parser.
//!
//! This initial ES5-shaped subset uses ASCII identifiers (including ASCII
//! Unicode escapes) and UTF-16 strings. Strict mode, regular expressions,
//! accessors, for-in, and newer language syntax are explicit errors.

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

    fn tokens(self) -> Result<Vec<Token>, String> {
        self.tokens_with_limit(MAX_TOKENS)
    }

    fn tokens_with_limit(mut self, limit: usize) -> Result<Vec<Token>, String> {
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
            if tokens.len() >= limit {
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

struct Label {
    name: String,
    iteration: bool,
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    nesting: usize,
    nodes: usize,
    functions: usize,
    loops: usize,
    labels: Vec<Label>,
}

/// Parse the entire source or return a byte-positioned error. No successful
/// prefix is returned after a lexical, syntax, unsupported-feature, or limit error.
pub fn parse(source: &str) -> Result<Program, String> {
    if source.len() > MAX_SOURCE {
        return Err(error(MAX_SOURCE, "Source exceeds one MiB"));
    }
    let tokens = Lexer { source, pos: 0 }.tokens()?;
    let mut parser = Parser::new(tokens);
    let (body, _) = parser.body(false, true)?;
    Ok(Program(body))
}

/// Parse the Function constructor's already comma-joined parameter arguments and
/// body as separate grammar inputs (ES5 15.3.2.1). Neither fragment can close,
/// comment out, or otherwise change the grammar of the other. The returned name
/// is display metadata; the runtime must create this function in global scope,
/// without introducing a named-function-expression binding for `anonymous`.
///
/// Both fragments share the one-MiB source, token and AST-growth limits. Parameter
/// bindings are charged alongside body nodes, and the function wrapper contributes
/// to the normal AST depth. Diagnostics identify the fragment and its byte offset.
pub fn parse_function(parameters: &str, body: &str) -> Result<Expr, String> {
    if parameters.len().saturating_add(body.len()) > MAX_SOURCE {
        return Err(error(MAX_SOURCE, "Source exceeds one MiB"));
    }
    let tokens = Lexer {
        source: parameters,
        pos: 0,
    }
    .tokens()
    .map_err(|error| format!("Function parameters: {error}"))?;
    // End-of-input sentinels are not source tokens, consistent with tokens().
    let remaining_tokens = MAX_TOKENS - (tokens.len() - 1);
    let mut parser = Parser::new(tokens);
    let params = (|| {
        let mut params = Vec::new();
        if !parser.end() {
            loop {
                let name = parser.name(false)?;
                parser.node(1)?;
                params.push(name);
                if !parser.eat(",") {
                    break;
                }
            }
        }
        if !parser.end() {
            return Err(parser.fail("Unexpected token in Function parameter list"));
        }
        Ok(params)
    })()
    .map_err(|error| format!("Function parameters: {error}"))?;

    // Drop parameter tokens before allocating body tokens. There is no synthetic
    // source concatenation or second copy of either input string.
    parser.tokens = Vec::new();
    parser.tokens = Lexer {
        source: body,
        pos: 0,
    }
    .tokens_with_limit(remaining_tokens)
    .map_err(|error| format!("Function body: {error}"))?;
    parser.pos = 0;
    parser.functions = 1;
    let (body, depth) = parser
        .nested(|parser| parser.body(false, true))
        .map_err(|error| format!("Function body: {error}"))?;
    parser
        .expr(
            Expr::Function {
                name: Some("anonymous".into()),
                params,
                body,
            },
            depth + 1,
        )
        .map(|expression| expression.value)
        .map_err(|error| format!("Function body: {error}"))
}

/// Recognize only parser-owned resource diagnostics. Runtime dynamic parsing
/// uses this to keep exhausted budgets fatal instead of catchable SyntaxErrors.
pub fn is_limit_error(message: &str) -> bool {
    let message = message
        .strip_prefix("Function parameters: ")
        .or_else(|| message.strip_prefix("Function body: "))
        .unwrap_or(message);
    let Some((reason, offset)) = message.rsplit_once(" at byte ") else {
        return false;
    };
    offset.parse::<usize>().is_ok()
        && matches!(
            reason,
            "Source exceeds one MiB"
                | "Token limit exceeded"
                | "Parser nesting limit exceeded"
                | "AST depth limit exceeded"
                | "AST node limit exceeded"
                | "Label nesting limit exceeded"
        )
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens,
            pos: 0,
            nesting: 0,
            nodes: 0,
            functions: 0,
            loops: 0,
            labels: Vec::new(),
        }
    }

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

    fn label_start(&self) -> bool {
        matches!(&self.token().kind, Kind::Word(name) if !reserved(name))
            && self
                .tokens
                .get(self.pos + 1)
                .is_some_and(|token| matches!(token.kind, Kind::Punct(":")))
    }

    fn labelled_statement(&mut self) -> Result<S, String> {
        // ES5 12.7/12.8/12.12: consecutive labels share the terminal statement's
        // label set; a block or another intervening statement ends that chain.
        // Collect once, so long chains cannot cause quadratic token lookahead.
        let first = self.labels.len();
        while self.label_start() {
            let offset = self.token().offset;
            if self.labels.len() >= MAX_DEPTH {
                return Err(error(offset, "Label nesting limit exceeded"));
            }
            let name = self.name(false)?;
            self.expect(":")?;
            if self.labels.iter().any(|label| label.name == name) {
                return Err(error(offset, "Duplicate active label"));
            }
            self.labels.push(Label {
                name,
                iteration: false,
            });
        }
        let iteration = self.word("while") || self.word("do") || self.word("for");
        for label in &mut self.labels[first..] {
            label.iteration = iteration;
        }
        let body = self.statement(false);
        let labels = self.labels.split_off(first);
        let mut body = body?;
        for label in labels.into_iter().rev() {
            body = self.stmt(
                Stmt::Label {
                    name: label.name,
                    body: Box::new(body.value),
                },
                body.depth + 1,
            )?;
        }
        Ok(body)
    }

    fn statement_inner(&mut self, declarations: bool) -> Result<S, String> {
        if self.label_start() {
            return self.labelled_statement();
        }
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
            let offset = self.token().offset;
            let is_break = self.eat_word("break");
            if !is_break {
                self.pos += 1;
            }
            let target_offset = self.token().offset;
            let target = if !self.token().newline && matches!(self.token().kind, Kind::Word(_)) {
                Some(self.name(false)?)
            } else {
                None
            };
            if let Some(name) = &target {
                let label = self.labels.iter().find(|label| &label.name == name);
                match label {
                    None => return Err(error(target_offset, "Unknown enclosing label")),
                    Some(label) if !is_break && !label.iteration => {
                        return Err(error(
                            target_offset,
                            "continue label does not target a loop",
                        ));
                    }
                    Some(_) => {}
                }
            } else if self.loops == 0 {
                return Err(error(offset, "break or continue outside a loop"));
            }
            self.semicolon()?;
            return self.stmt(
                if is_break {
                    Stmt::Break(target)
                } else {
                    Stmt::Continue(target)
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
        let previous_labels = std::mem::take(&mut self.labels);
        self.loops = 0;
        self.functions += 1;
        let result = self.nested(|parser| parser.body(true, true));
        self.functions -= 1;
        self.loops = previous_loops;
        self.labels = previous_labels;
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
    fn dynamic_function_parses_parameters_and_return_body_independently() {
        assert_eq!(
            parse_function("a /* first */, b\\u0063", "return a + bc;").unwrap(),
            Expr::Function {
                name: Some("anonymous".into()),
                params: vec!["a".into(), "bc".into()],
                body: vec![Stmt::Return(Some(Expr::Binary {
                    op: "+".into(),
                    left: Box::new(Expr::Ident("a".into())),
                    right: Box::new(Expr::Ident("bc".into())),
                }))],
            }
        );
        assert_eq!(
            parse_function("// no parameters", "// no body").unwrap(),
            Expr::Function {
                name: Some("anonymous".into()),
                params: vec![],
                body: vec![],
            }
        );
        let Expr::Function { params, body, .. } =
            parse_function("a // parameter comment", "return a;").unwrap()
        else {
            panic!()
        };
        assert_eq!(params, vec!["a"]);
        assert_eq!(body, vec![Stmt::Return(Some(Expr::Ident("a".into())))]);
        for (parameters, body) in [
            ("a, a", "return a;"), // Duplicate parameters are valid in non-strict code.
            ("eval, arguments, undefined", "return arguments;"),
            ("a, // between arguments\n b", "return b;"),
            ("", "function nested(){return 2;} return nested();"),
            ("", "outer: while(false){continue outer;} return 1;"),
            ("", "('use strict'); return 1;"),
        ] {
            assert!(
                parse_function(parameters, body).is_ok(),
                "{parameters:?}: {body:?}"
            );
        }
        let Expr::Function { body, .. } = parse_function("", "return\n2;").unwrap() else {
            panic!()
        };
        assert_eq!(
            body,
            vec![Stmt::Return(None), Stmt::Expr(Expr::Number(2.0))]
        );
        assert!(parse("return 1;").is_err());
    }

    #[test]
    fn dynamic_function_rejects_cross_fragment_syntax_and_invalid_bindings() {
        for parameters in [
            "a,",
            ",a",
            "a b",
            "a=1",
            "{a}",
            "[a]",
            "...a",
            "return",
            "r\\u0065turn",
            "é",
            "a) { return 1; } (",
            "a); harmlessMarker=1; (",
            "a /*",
        ] {
            let error = parse_function(parameters, "return 2;").expect_err(parameters);
            assert!(error.starts_with("Function parameters: "), "{error}");
            assert!(error.contains(" at byte "), "{error}");
            assert!(!is_limit_error(&error), "{error}");
        }
        let error = parse_function("a /*", "*/ return a;").unwrap_err();
        assert!(error.starts_with("Function parameters: "));
        for body in [
            "return 1; } function unexpected(){}",
            "}); harmlessMarker=1; (function(){",
            "return 1; /*",
            "return 1; @",
            "'use strict'; return 1;",
            "'another directive'; 'use strict'; return 1;",
            "break;",
            "continue;",
            "break outside;",
            "continue outside;",
            "outer: { (function(){break outer;})(); }",
            "outer: while(false) { (function(){continue outer;})(); }",
        ] {
            let error = parse_function("", body).expect_err(body);
            assert!(error.starts_with("Function body: "), "{error}");
            assert!(error.contains(" at byte "), "{error}");
            assert!(!is_limit_error(&error), "{error}");
        }
        assert_eq!(
            parse_function("a", "return 1; @").unwrap_err(),
            "Function body: Unsupported character or non-ASCII identifier at byte 10"
        );
    }

    #[test]
    fn dynamic_function_shares_source_token_ast_and_depth_limits() {
        let half = " ".repeat(MAX_SOURCE / 2);
        assert!(parse_function(&half, &half).is_ok());
        assert!(is_limit_error(
            &parse_function(&half, &format!("{half} ")).unwrap_err()
        ));

        // Three parameter-list tokens plus the body exactly consume the budget.
        let body = ";".repeat(MAX_TOKENS - 3);
        assert!(parse_function("a,b", &body).is_ok());
        let error = parse_function("a,b", &format!("{body};")).unwrap_err();
        assert!(error.contains("Token limit exceeded"), "{error}");
        assert!(is_limit_error(&error));

        // ASI gives two AST nodes per one source token. Bindings and the function
        // wrapper must share the node count with these body expression statements.
        let body = "1\n".repeat(MAX_NODES / 2 - 1);
        assert!(parse_function("a", &body).is_ok());
        let error = parse_function("a,b", &body).unwrap_err();
        assert!(error.contains("AST node limit exceeded"), "{error}");
        assert!(is_limit_error(&error));

        let body: String = (0..MAX_DEPTH - 2).map(|i| format!("label{i}:")).collect();
        assert!(parse_function("", &format!("{body};")).is_ok());
        let error = parse_function("", &format!("{body}last:;")).unwrap_err();
        assert!(error.contains("AST depth limit exceeded"), "{error}");
        assert!(is_limit_error(&error));
        let error =
            parse_function("", &format!("{}1{}", "(".repeat(200), ")".repeat(200))).unwrap_err();
        assert!(error.contains("Parser nesting limit exceeded"), "{error}");
        assert!(is_limit_error(&error));
    }

    #[test]
    fn parser_limit_classification_accepts_only_exact_owned_diagnostics() {
        for reason in [
            "Source exceeds one MiB",
            "Token limit exceeded",
            "Parser nesting limit exceeded",
            "AST depth limit exceeded",
            "AST node limit exceeded",
            "Label nesting limit exceeded",
        ] {
            for prefix in ["", "Function parameters: ", "Function body: "] {
                assert!(is_limit_error(&format!("{prefix}{reason} at byte 12")));
            }
        }
        for error in [
            "Unknown enclosing label at byte 12",
            "Token limit exceeded",
            "Token limit exceeded at byte -1",
            "Token limit exceeded at byte 1 trailing",
            "User said Token limit exceeded at byte 1",
            "Function body: Syntax error at byte 0",
        ] {
            assert!(!is_limit_error(error), "{error}");
        }
    }

    #[test]
    fn labels_retain_ast_names_and_allow_non_loop_break_targets() {
        assert_eq!(
            parse("exit: { break exit; }").unwrap(),
            Program(vec![Stmt::Label {
                name: "exit".into(),
                body: Box::new(Stmt::Block(vec![Stmt::Break(Some("exit".into()))])),
            }])
        );
        assert_eq!(
            parse("first: second: while (false) { continue first; break second; }").unwrap(),
            Program(vec![Stmt::Label {
                name: "first".into(),
                body: Box::new(Stmt::Label {
                    name: "second".into(),
                    body: Box::new(Stmt::While {
                        test: Expr::Bool(false),
                        body: Box::new(Stmt::Block(vec![
                            Stmt::Continue(Some("first".into())),
                            Stmt::Break(Some("second".into())),
                        ])),
                    }),
                }),
            }])
        );
        for source in [
            "empty: ;",
            "value: 1;",
            "declaration: var value=1;",
            "branch: if (true) break branch; else 2;",
            "outer: inner: for(;;) { continue outer; break inner; }",
            "outer: inner: do { continue inner; break outer; } while(false);",
            "outer: while(false) { inner: { continue outer; break inner; } }",
            "first\n:\nsecond: while(false) { continue first; }",
        ] {
            assert!(parse(source).is_ok(), "{source}");
        }
    }

    #[test]
    fn labels_are_unique_only_while_active_and_reset_at_functions() {
        for source in [
            "same: { break same; } same: { break same; }",
            "var same=1; same: { var same=2; break same; }",
            "outer: while(false) { (function(){outer: while(false){continue outer;}})(); continue outer; }",
            "outer: { (function named(){outer: {break outer;}})(); break outer; }",
            "function f(){same: {break same;}} function g(){same: {break same;}}",
            "outer: while(false) { inner: while(false) {continue outer;} continue outer; }",
        ] {
            assert!(parse(source).is_ok(), "{source}");
        }
        for source in [
            "same: same: ;",
            "same: { if(false) same: ; }",
            "same: while(false) { same: ; }",
            "s\\u0061me: { same: ; }",
            "outer: { (function(){})(); outer: ; }",
            "outer: { (function(){break outer;})(); }",
            "outer: while(false) { (function(){continue outer;})(); }",
            "outer: while(false) { (function(){break;})(); }",
            "outer: while(false) { (function(){continue;})(); }",
        ] {
            assert!(parse(source).is_err(), "{source}");
        }
    }

    #[test]
    fn invalid_label_targets_reject_the_whole_program_with_byte_offsets() {
        for source in [
            "var before=1; break missing;",
            "var before=1; continue missing;",
            "label: {} break label;",
            "label: while(false) {} continue label;",
            "block: { break; }",
            "block: { continue block; }",
            "block: { while(false) {continue block;} }",
            "outer: inner: { while(false) {continue inner;} }",
            "branch: if(true) while(false) {continue branch;}",
            "loop: while(false) { continue unknown; }",
            "loop: while(false) { break unknown; }",
            "loop: while(false) { continue loop extra; }",
            "label:",
            "(label): ;",
            "obj.label: ;",
            "1: ;",
            "if: ;",
            "label: function f(){}",
        ] {
            let error = parse(source).expect_err(source);
            assert!(error.contains("byte"), "{source}: {error}");
        }
        let source = "outer: { break missing; }";
        assert_eq!(
            parse(source).unwrap_err(),
            format!(
                "Unknown enclosing label at byte {}",
                source.find("missing").unwrap()
            )
        );
        let source = "label: { label: ; }";
        assert_eq!(
            parse(source).unwrap_err(),
            format!(
                "Duplicate active label at byte {}",
                source.rfind("label").unwrap()
            )
        );
    }

    #[test]
    fn labelled_control_obeys_asi_and_comment_line_terminators() {
        for gap in [
            "\n",
            "\r",
            "\r\n",
            "\u{2028}",
            "\u{2029}",
            "/*\n*/",
            "// next\n",
        ] {
            let Program(body) = parse(&format!(
                "outer: while(false) {{break{gap}outer; continue{gap}outer;}}"
            ))
            .unwrap();
            let Stmt::Label { body, .. } = &body[0] else {
                panic!()
            };
            let Stmt::While { body, .. } = body.as_ref() else {
                panic!()
            };
            assert_eq!(
                body.as_ref(),
                &Stmt::Block(vec![
                    Stmt::Break(None),
                    Stmt::Expr(Expr::Ident("outer".into())),
                    Stmt::Continue(None),
                    Stmt::Expr(Expr::Ident("outer".into())),
                ]),
                "{gap:?}"
            );
            assert!(parse(&format!("outer: {{break{gap}outer;}}")).is_err());
        }
        let Program(body) =
            parse("outer: while(false) {break /*same line*/ outer; continue /*same line*/ outer;}")
                .unwrap();
        let Stmt::Label { body, .. } = &body[0] else {
            panic!()
        };
        let Stmt::While { body, .. } = body.as_ref() else {
            panic!()
        };
        assert_eq!(
            body.as_ref(),
            &Stmt::Block(vec![
                Stmt::Break(Some("outer".into())),
                Stmt::Continue(Some("outer".into())),
            ])
        );
    }

    #[test]
    fn label_chains_obey_existing_ast_and_scope_bounds() {
        fn chain(count: usize) -> String {
            let mut source = String::new();
            for index in 0..count {
                source.push_str(&format!("label{index}:"));
            }
            source.push(';');
            source
        }
        assert!(parse(&chain(MAX_DEPTH - 1)).is_ok());
        assert!(parse(&chain(MAX_DEPTH)).unwrap_err().contains("AST depth"));
        assert!(
            parse(&chain(MAX_DEPTH + 1))
                .unwrap_err()
                .contains("Label nesting")
        );
        assert!(parse(&chain(10_000)).unwrap_err().contains("Label nesting"));
        let source = format!("{};{}", "label:{".repeat(MAX_DEPTH), "}".repeat(MAX_DEPTH));
        assert!(parse(&source).is_err());
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
