//! Original bounded UTF-16 regular expressions, following ES5.1 section 15.10.
//! Matching uses explicit task/backtracking stacks; only lookahead submachines
//! nest in Rust, with a separate depth cap. Syntax/resource errors are distinct.
//! Limits are research policy, not an ECMAScript conformance claim. Unicode
//! canonicalization uses Rust's Unicode uppercase tables with the ES5 rules.

const MAX_PATTERN: usize = 16_384;
const MAX_NODES: usize = 8_192;
const MAX_GROUPS: usize = 64;
const MAX_DEPTH: usize = 64;
const MAX_CLASSES: usize = 128;
const MAX_COMPILED: usize = 2 * 1024 * 1024;
const COMPILE_FUEL: u64 = 2_000_000;
const MAX_INPUT: usize = 1024 * 1024;
const MAX_TASKS: usize = 16_384;
const MAX_PENDING: usize = 4_096;
const MAX_WORK_BYTES: usize = 4 * 1024 * 1024;
const MAX_LOOK_DEPTH: usize = 16;
const MAX_REPEAT: u32 = 1_000_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Syntax(String),
    Limit(String),
}
impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Syntax(text) | Self::Limit(text) => formatter.write_str(text),
        }
    }
}
impl std::error::Error for Error {}
fn limit(what: &str) -> Error {
    Error::Limit(format!("RegExp {what} limit exhausted"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    pub start: usize,
    pub end: usize,
    /// Slot zero is the whole match; unmatched numbered captures remain None.
    pub captures: Vec<Option<(usize, usize)>>,
}

#[derive(Clone, Copy, Debug)]
enum Builtin {
    Digit,
    Space,
    Word,
}
#[derive(Debug)]
struct Class {
    bits: Vec<u64>,
    invert: bool,
}
#[derive(Debug)]
enum Node {
    Empty,
    Character(u16),
    Dot,
    Builtin(Builtin, bool),
    Class(Class),
    Start,
    End,
    Boundary(bool),
    Backref(usize),
    Sequence(Vec<usize>),
    Alternative(Vec<usize>),
    Group {
        child: usize,
        index: usize,
    },
    Look {
        child: usize,
        positive: bool,
    },
    Repeat {
        child: usize,
        min: u32,
        max: Option<u32>,
        greedy: bool,
        first_capture: usize,
        capture_end: usize,
    },
}

#[derive(Debug)]
pub struct Regex {
    pattern: Vec<u16>,
    nodes: Vec<Node>,
    root: usize,
    groups: usize,
    global: bool,
    ignore_case: bool,
    multiline: bool,
}
impl Regex {
    pub fn compile(pattern: &[u16], flags: &str) -> Result<Self, Error> {
        if pattern.len() > MAX_PATTERN {
            return Err(limit("pattern size"));
        }
        let (mut global, mut ignore_case, mut multiline) = (false, false, false);
        for flag in flags.chars() {
            let slot = match flag {
                'g' => &mut global,
                'i' => &mut ignore_case,
                'm' => &mut multiline,
                _ => {
                    return Err(Error::Syntax(
                        "Unsupported RegExp flag (only g, i, m are implemented)".into(),
                    ));
                }
            };
            if *slot {
                return Err(Error::Syntax("Duplicate RegExp flag".into()));
            }
            *slot = true;
        }
        let mut parser = Parser {
            pattern,
            position: 0,
            nodes: Vec::new(),
            groups: 0,
            depth: 0,
            classes: 0,
            fuel: COMPILE_FUEL,
            ignore_case,
        };
        let root = parser.disjunction()?;
        if parser.position != pattern.len() {
            return Err(parser.syntax("Unexpected pattern character"));
        }
        for node in &parser.nodes {
            if let Node::Backref(index) = node
                && *index > parser.groups
            {
                return Err(parser
                    .syntax("Backreference exceeds capture count (legacy octal is unsupported)"));
            }
        }
        let result = Self {
            pattern: pattern.to_vec(),
            nodes: parser.nodes,
            root,
            groups: parser.groups,
            global,
            ignore_case,
            multiline,
        };
        if result.estimated_bytes() > MAX_COMPILED {
            return Err(limit("compiled size"));
        }
        Ok(result)
    }
    pub fn global(&self) -> bool {
        self.global
    }
    pub fn ignore_case(&self) -> bool {
        self.ignore_case
    }
    pub fn multiline(&self) -> bool {
        self.multiline
    }
    pub fn pattern(&self) -> &[u16] {
        &self.pattern
    }
    pub fn flags(&self) -> String {
        let mut result = String::new();
        if self.global {
            result.push('g');
        }
        if self.ignore_case {
            result.push('i');
        }
        if self.multiline {
            result.push('m');
        }
        result
    }
    pub fn estimated_bytes(&self) -> usize {
        self.pattern.capacity() * 2
            + self.nodes.capacity() * std::mem::size_of::<Node>()
            + self
                .nodes
                .iter()
                .map(|node| match node {
                    Node::Sequence(items) | Node::Alternative(items) => {
                        items.capacity() * std::mem::size_of::<usize>()
                    }
                    Node::Class(class) => class.bits.capacity() * 8,
                    _ => 0,
                })
                .sum::<usize>()
            + std::mem::size_of::<Self>()
    }
    /// Find the first match at or after start. Empty results do not advance;
    /// RegExp.exec and String methods implement their distinct progress rules.
    pub fn find(
        &self,
        input: &[u16],
        start: usize,
        fuel: &mut u64,
    ) -> Result<Option<Match>, Error> {
        if input.len() > MAX_INPUT {
            return Err(limit("input size"));
        }
        if start > input.len() {
            return Ok(None);
        }
        let mut work = Work {
            fuel,
            allocated: 0,
            pending: 0,
        };
        for candidate in start..=input.len() {
            work.step()?;
            work.allocate((self.groups + 1) * std::mem::size_of::<Option<(usize, usize)>>() + 128)?;
            let state = State {
                position: candidate,
                captures: vec![None; self.groups + 1],
                tasks: vec![Task::Visit(self.root)],
            };
            if let Some(mut result) = self.run(input, state, &mut work, 0)? {
                result.captures[0] = Some((candidate, result.position));
                return Ok(Some(Match {
                    start: candidate,
                    end: result.position,
                    captures: result.captures,
                }));
            }
        }
        Ok(None)
    }

    fn run(
        &self,
        input: &[u16],
        mut state: State,
        work: &mut Work<'_>,
        depth: usize,
    ) -> Result<Option<State>, Error> {
        if depth > MAX_LOOK_DEPTH {
            return Err(limit("lookahead depth"));
        }
        let mut alternatives: Vec<State> = Vec::new();
        'machine: loop {
            work.step()?;
            if state.tasks.len() > MAX_TASKS {
                return Err(limit("task stack"));
            }
            let Some(task) = state.tasks.pop() else {
                work.pending -= alternatives.len();
                return Ok(Some(state));
            };
            let mut failed = false;
            match task {
                Task::Visit(id) => match &self.nodes[id] {
                    Node::Empty => {}
                    Node::Character(character) => {
                        if input
                            .get(state.position)
                            .is_some_and(|unit| equal(*unit, *character, self.ignore_case))
                        {
                            state.position += 1;
                        } else {
                            failed = true;
                        }
                    }
                    Node::Dot => {
                        if input.get(state.position).is_some_and(|unit| !line(*unit)) {
                            state.position += 1;
                        } else {
                            failed = true;
                        }
                    }
                    Node::Builtin(kind, invert) => {
                        if input
                            .get(state.position)
                            .is_some_and(|unit| builtin(*kind, *unit) != *invert)
                        {
                            state.position += 1;
                        } else {
                            failed = true;
                        }
                    }
                    Node::Class(class) => {
                        if input.get(state.position).is_some_and(|unit| {
                            bit(&class.bits, canonical(*unit, self.ignore_case)) != class.invert
                        }) {
                            state.position += 1;
                        } else {
                            failed = true;
                        }
                    }
                    Node::Start => {
                        failed = state.position != 0
                            && !(self.multiline && line(input[state.position - 1]))
                    }
                    Node::End => {
                        failed = state.position != input.len()
                            && !(self.multiline && line(input[state.position]))
                    }
                    Node::Boundary(positive) => {
                        let before = state
                            .position
                            .checked_sub(1)
                            .and_then(|i| input.get(i))
                            .is_some_and(|u| word(*u));
                        let after = input.get(state.position).is_some_and(|u| word(*u));
                        failed = (before != after) != *positive;
                    }
                    Node::Backref(index) => {
                        if let Some((from, to)) = state.captures[*index] {
                            let length = to - from;
                            if length > input.len() - state.position {
                                failed = true;
                            } else {
                                for offset in 0..length {
                                    work.step()?;
                                    if !equal(
                                        input[from + offset],
                                        input[state.position + offset],
                                        self.ignore_case,
                                    ) {
                                        failed = true;
                                        break;
                                    }
                                }
                                if !failed {
                                    state.position += length;
                                }
                            }
                        }
                    }
                    Node::Sequence(items) => {
                        work.tasks(&state, items.len())?;
                        state
                            .tasks
                            .extend(items.iter().rev().copied().map(Task::Visit));
                    }
                    Node::Alternative(items) => {
                        for child in items.iter().skip(1).rev() {
                            let mut alternative = work.copy(&state)?;
                            work.tasks(&alternative, 1)?;
                            alternative.tasks.push(Task::Visit(*child));
                            work.queue(&mut alternatives, alternative)?;
                        }
                        work.tasks(&state, 1)?;
                        state.tasks.push(Task::Visit(items[0]));
                    }
                    Node::Group { child, index } => {
                        state.captures[*index] = None;
                        work.tasks(&state, 2)?;
                        state.tasks.push(Task::EndGroup(*index, state.position));
                        state.tasks.push(Task::Visit(*child));
                    }
                    Node::Look { child, positive } => {
                        let mut inner = work.copy(&state)?;
                        inner.tasks.clear();
                        inner.tasks.push(Task::Visit(*child));
                        let result = self.run(input, inner, work, depth + 1)?;
                        if *positive {
                            if let Some(result) = result {
                                state.captures = result.captures;
                            } else {
                                failed = true;
                            }
                        } else {
                            failed = result.is_some();
                        }
                    }
                    Node::Repeat { .. } => {
                        work.tasks(&state, 1)?;
                        state.tasks.push(Task::Repeat(id, 0));
                    }
                },
                Task::EndGroup(index, from) => state.captures[index] = Some((from, state.position)),
                Task::Repeat(id, count) => {
                    let Node::Repeat {
                        child,
                        min,
                        max,
                        greedy,
                        first_capture,
                        capture_end,
                    } = &self.nodes[id]
                    else {
                        unreachable!()
                    };
                    if max.is_some_and(|max| count >= max) {
                        continue;
                    }
                    if count >= *min && *greedy {
                        let alternative = work.copy(&state)?;
                        work.queue(&mut alternatives, alternative)?;
                    }
                    if count >= *min && !*greedy {
                        let mut alternative = work.copy(&state)?;
                        for index in *first_capture..*capture_end {
                            alternative.captures[index] = None;
                        }
                        work.tasks(&alternative, 2)?;
                        alternative
                            .tasks
                            .push(Task::AfterRepeat(id, count, state.position));
                        alternative.tasks.push(Task::Visit(*child));
                        work.queue(&mut alternatives, alternative)?;
                    } else {
                        for index in *first_capture..*capture_end {
                            state.captures[index] = None;
                        }
                        work.tasks(&state, 2)?;
                        state
                            .tasks
                            .push(Task::AfterRepeat(id, count, state.position));
                        state.tasks.push(Task::Visit(*child));
                    }
                }
                Task::AfterRepeat(id, count, from) => {
                    let Node::Repeat { min, .. } = self.nodes[id] else {
                        unreachable!()
                    };
                    if count >= min && state.position == from {
                        failed = true;
                    } else {
                        work.tasks(&state, 1)?;
                        state.tasks.push(Task::Repeat(id, count + 1));
                    }
                }
            }
            if failed {
                if let Some(next) = alternatives.pop() {
                    work.pending -= 1;
                    state = next;
                    continue 'machine;
                }
                return Ok(None);
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum Task {
    Visit(usize),
    EndGroup(usize, usize),
    Repeat(usize, u32),
    AfterRepeat(usize, u32, usize),
}
#[derive(Clone)]
struct State {
    position: usize,
    captures: Vec<Option<(usize, usize)>>,
    tasks: Vec<Task>,
}
struct Work<'a> {
    fuel: &'a mut u64,
    allocated: usize,
    pending: usize,
}
impl Work<'_> {
    fn step(&mut self) -> Result<(), Error> {
        *self.fuel = self.fuel.checked_sub(1).ok_or_else(|| limit("fuel"))?;
        Ok(())
    }
    fn allocate(&mut self, bytes: usize) -> Result<(), Error> {
        self.allocated = self.allocated.saturating_add(bytes);
        if self.allocated > MAX_WORK_BYTES {
            return Err(limit("matching state work"));
        }
        Ok(())
    }
    fn tasks(&mut self, state: &State, additional: usize) -> Result<(), Error> {
        if state.tasks.len().saturating_add(additional) > MAX_TASKS {
            return Err(limit("task stack"));
        }
        self.allocate(additional.saturating_mul(std::mem::size_of::<Task>()))
    }
    fn copy(&mut self, state: &State) -> Result<State, Error> {
        self.allocate(
            128 + state.captures.len() * std::mem::size_of::<Option<(usize, usize)>>()
                + state.tasks.len() * std::mem::size_of::<Task>(),
        )?;
        Ok(state.clone())
    }
    fn queue(&mut self, stack: &mut Vec<State>, state: State) -> Result<(), Error> {
        if self.pending >= MAX_PENDING {
            return Err(limit("backtracking states"));
        }
        self.pending += 1;
        stack.push(state);
        Ok(())
    }
}

struct Parser<'a> {
    pattern: &'a [u16],
    position: usize,
    nodes: Vec<Node>,
    groups: usize,
    depth: usize,
    classes: usize,
    fuel: u64,
    ignore_case: bool,
}
enum ClassAtom {
    Unit(u16),
    Builtin(Builtin, bool),
}
impl Parser<'_> {
    fn syntax(&self, message: &str) -> Error {
        Error::Syntax(format!("{message} at pattern unit {}", self.position))
    }
    fn step(&mut self) -> Result<(), Error> {
        self.fuel = self
            .fuel
            .checked_sub(1)
            .ok_or_else(|| limit("compile work"))?;
        Ok(())
    }
    fn peek(&self) -> Option<u16> {
        self.pattern.get(self.position).copied()
    }
    fn eat(&mut self, character: u8) -> bool {
        if self.peek() == Some(character as u16) {
            self.position += 1;
            true
        } else {
            false
        }
    }
    fn take(&mut self) -> Result<u16, Error> {
        self.step()?;
        let unit = self
            .peek()
            .ok_or_else(|| self.syntax("Unexpected end of pattern"))?;
        self.position += 1;
        Ok(unit)
    }
    fn node(&mut self, node: Node) -> Result<usize, Error> {
        self.step()?;
        if self.nodes.len() >= MAX_NODES {
            return Err(limit("node count"));
        }
        let id = self.nodes.len();
        self.nodes.push(node);
        Ok(id)
    }
    fn disjunction(&mut self) -> Result<usize, Error> {
        if self.depth >= MAX_DEPTH {
            return Err(limit("pattern nesting"));
        }
        self.depth += 1;
        let mut alternatives = vec![self.sequence()?];
        while self.eat(b'|') {
            alternatives.push(self.sequence()?);
        }
        self.depth -= 1;
        if alternatives.len() == 1 {
            Ok(alternatives[0])
        } else {
            self.node(Node::Alternative(alternatives))
        }
    }
    fn sequence(&mut self) -> Result<usize, Error> {
        let mut items = Vec::new();
        while self
            .peek()
            .is_some_and(|u| u != b'|' as u16 && u != b')' as u16)
        {
            items.push(self.term()?);
        }
        match items.len() {
            0 => self.node(Node::Empty),
            1 => Ok(items[0]),
            _ => self.node(Node::Sequence(items)),
        }
    }
    fn term(&mut self) -> Result<usize, Error> {
        let first_capture = self.groups + 1;
        let (child, assertion) = self.atom()?;
        let capture_end = self.groups + 1;
        let quantifier = match self.peek() {
            Some(42) => {
                self.position += 1;
                Some((0, None))
            }
            Some(43) => {
                self.position += 1;
                Some((1, None))
            }
            Some(63) => {
                self.position += 1;
                Some((0, Some(1)))
            }
            Some(123) => {
                self.position += 1;
                let min = self.number()?;
                let max = if self.eat(b',') {
                    if self.peek() == Some(b'}' as u16) {
                        None
                    } else {
                        Some(self.number()?)
                    }
                } else {
                    Some(min)
                };
                if !self.eat(b'}') {
                    return Err(self.syntax("Unclosed quantifier"));
                }
                if max.is_some_and(|max| max < min) {
                    return Err(self.syntax("Quantifier range is reversed"));
                }
                Some((min, max))
            }
            _ => None,
        };
        if let Some((min, max)) = quantifier {
            if assertion {
                return Err(self.syntax("Quantified assertions are unsupported"));
            }
            let greedy = !self.eat(b'?');
            self.node(Node::Repeat {
                child,
                min,
                max,
                greedy,
                first_capture,
                capture_end,
            })
        } else {
            Ok(child)
        }
    }
    fn atom(&mut self) -> Result<(usize, bool), Error> {
        let character = self.take()?;
        let mut assertion = false;
        let node = match character {
            46 => Node::Dot,
            94 => {
                assertion = true;
                Node::Start
            }
            36 => {
                assertion = true;
                Node::End
            }
            91 => Node::Class(self.class()?),
            40 => {
                let (capture, look) = if self.eat(b'?') {
                    if self.eat(b':') {
                        (None, None)
                    } else if self.eat(b'=') {
                        (None, Some(true))
                    } else if self.eat(b'!') {
                        (None, Some(false))
                    } else {
                        return Err(self.syntax("Unsupported group construct"));
                    }
                } else {
                    if self.groups >= MAX_GROUPS {
                        return Err(limit("capture count"));
                    }
                    self.groups += 1;
                    (Some(self.groups), None)
                };
                let child = self.disjunction()?;
                if !self.eat(b')') {
                    return Err(self.syntax("Unclosed group"));
                }
                if let Some(index) = capture {
                    Node::Group { child, index }
                } else if let Some(positive) = look {
                    assertion = true;
                    Node::Look { child, positive }
                } else {
                    return Ok((child, false));
                }
            }
            92 => {
                let escaped = self.take()?;
                match escaped {
                    98 => {
                        assertion = true;
                        Node::Boundary(true)
                    }
                    66 => {
                        assertion = true;
                        Node::Boundary(false)
                    }
                    49..=57 => {
                        let mut index = (escaped - 48) as usize;
                        while self.peek().is_some_and(|u| (48..=57).contains(&u)) {
                            let digit = self.take()? - 48;
                            index = index.saturating_mul(10).saturating_add(digit as usize);
                            if index > MAX_GROUPS {
                                return Err(
                                    self.syntax("Backreference exceeds supported capture count")
                                );
                            }
                        }
                        Node::Backref(index)
                    }
                    _ => match self.escape(escaped)? {
                        ClassAtom::Unit(unit) => Node::Character(unit),
                        ClassAtom::Builtin(kind, invert) => Node::Builtin(kind, invert),
                    },
                }
            }
            41 | 42 | 43 | 63 | 123 | 125 | 93 => {
                return Err(self.syntax("Unexpected metacharacter"));
            }
            _ => Node::Character(character),
        };
        Ok((self.node(node)?, assertion))
    }
    fn number(&mut self) -> Result<u32, Error> {
        let mut result = 0u32;
        let start = self.position;
        while self.peek().is_some_and(|u| (48..=57).contains(&u)) {
            result = result
                .saturating_mul(10)
                .saturating_add((self.take()? - 48) as u32);
            if result > MAX_REPEAT {
                return Err(limit("quantifier count"));
            }
        }
        if start == self.position {
            return Err(self.syntax("Expected quantifier number"));
        }
        Ok(result)
    }
    fn hex(&mut self, count: usize) -> Result<u16, Error> {
        let mut result = 0u16;
        for _ in 0..count {
            let digit = match self.take()? {
                u @ 48..=57 => u - 48,
                u @ 65..=70 => u - 65 + 10,
                u @ 97..=102 => u - 97 + 10,
                _ => return Err(self.syntax("Invalid hexadecimal escape")),
            };
            result = result * 16 + digit;
        }
        Ok(result)
    }
    fn escape(&mut self, escaped: u16) -> Result<ClassAtom, Error> {
        let unit = match escaped {
            100 => return Ok(ClassAtom::Builtin(Builtin::Digit, false)),
            68 => return Ok(ClassAtom::Builtin(Builtin::Digit, true)),
            115 => return Ok(ClassAtom::Builtin(Builtin::Space, false)),
            83 => return Ok(ClassAtom::Builtin(Builtin::Space, true)),
            119 => return Ok(ClassAtom::Builtin(Builtin::Word, false)),
            87 => return Ok(ClassAtom::Builtin(Builtin::Word, true)),
            116 => 9,
            110 => 10,
            118 => 11,
            102 => 12,
            114 => 13,
            48 => {
                if self.peek().is_some_and(|u| (48..=57).contains(&u)) {
                    return Err(self.syntax("Legacy octal escapes are unsupported"));
                }
                0
            }
            99 => {
                let letter = self.take()?;
                if !((65..=90).contains(&letter) || (97..=122).contains(&letter)) {
                    return Err(self.syntax("Invalid control escape"));
                }
                letter % 32
            }
            120 => self.hex(2)?,
            117 => self.hex(4)?,
            _ => {
                if (48..=57).contains(&escaped)
                    || char::from_u32(escaped as u32).is_some_and(|c| {
                        c.is_alphanumeric() || matches!(c, '_' | '$' | '\u{200c}' | '\u{200d}')
                    })
                {
                    return Err(self.syntax("Unsupported identity escape"));
                }
                escaped
            }
        };
        Ok(ClassAtom::Unit(unit))
    }
    fn class_atom(&mut self) -> Result<ClassAtom, Error> {
        let character = self.take()?;
        if character != 92 {
            return Ok(ClassAtom::Unit(character));
        }
        let escaped = self.take()?;
        if escaped == 98 {
            return Ok(ClassAtom::Unit(8));
        }
        if escaped == 66 || (49..=57).contains(&escaped) {
            return Err(self.syntax("Word-boundary/backreference escape is invalid inside a class"));
        }
        self.escape(escaped)
    }
    fn class(&mut self) -> Result<Class, Error> {
        if self.classes >= MAX_CLASSES {
            return Err(limit("character class count"));
        }
        self.classes += 1;
        let invert = self.eat(b'^');
        let mut bits = vec![0u64; 1024];
        while self.peek().is_some_and(|u| u != 93) {
            let left = self.class_atom()?;
            if self.peek() == Some(45) && self.pattern.get(self.position + 1).copied() != Some(93) {
                self.position += 1;
                let right = self.class_atom()?;
                let (ClassAtom::Unit(from), ClassAtom::Unit(to)) = (left, right) else {
                    return Err(self.syntax("Character range endpoints must be single characters"));
                };
                if from > to {
                    return Err(self.syntax("Character range is reversed"));
                }
                for unit in from..=to {
                    self.step()?;
                    set_bit(&mut bits, canonical(unit, self.ignore_case));
                }
            } else {
                match left {
                    ClassAtom::Unit(unit) => set_bit(&mut bits, canonical(unit, self.ignore_case)),
                    ClassAtom::Builtin(kind, negate) => {
                        for unit in 0..=u16::MAX {
                            self.step()?;
                            if builtin(kind, unit) != negate {
                                set_bit(&mut bits, canonical(unit, self.ignore_case));
                            }
                        }
                    }
                }
            }
        }
        if !self.eat(b']') {
            return Err(self.syntax("Unclosed character class"));
        }
        Ok(Class { bits, invert })
    }
}

fn bit(bits: &[u64], unit: u16) -> bool {
    bits[unit as usize / 64] & (1u64 << (unit % 64)) != 0
}
fn set_bit(bits: &mut [u64], unit: u16) {
    bits[unit as usize / 64] |= 1u64 << (unit % 64);
}
fn line(unit: u16) -> bool {
    matches!(unit, 10 | 13 | 0x2028 | 0x2029)
}
fn word(unit: u16) -> bool {
    matches!(unit, 48..=57 | 65..=90 | 97..=122 | 95)
}
fn builtin(kind: Builtin, unit: u16) -> bool {
    match kind {
        Builtin::Digit => (48..=57).contains(&unit),
        Builtin::Word => word(unit),
        Builtin::Space => {
            matches!(unit, 9..=13 | 32 | 0xa0 | 0x1680 | 0x180e | 0x2000..=0x200a | 0x2028 | 0x2029 | 0x202f | 0x205f | 0x3000 | 0xfeff)
        }
    }
}
fn canonical(unit: u16, ignore_case: bool) -> u16 {
    if !ignore_case {
        return unit;
    }
    let Some(character) = char::from_u32(unit as u32) else {
        return unit;
    };
    let mut upper = character.to_uppercase();
    let first = upper.next().unwrap();
    if upper.next().is_some()
        || first as u32 > u16::MAX as u32
        || unit >= 128 && (first as u32) < 128
    {
        unit
    } else {
        first as u16
    }
}
fn equal(a: u16, b: u16, ignore_case: bool) -> bool {
    canonical(a, ignore_case) == canonical(b, ignore_case)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn units(value: &str) -> Vec<u16> {
        value.encode_utf16().collect()
    }
    fn found(pattern: &str, flags: &str, input: &str) -> Option<Match> {
        Regex::compile(&units(pattern), flags)
            .unwrap()
            .find(&units(input), 0, &mut 1_000_000)
            .unwrap()
    }
    #[test]
    fn literals_classes_anchors_and_utf16_indices() {
        assert_eq!(found("café", "", "😀café").unwrap().start, 2);
        assert_eq!(found(".", "", "😀").unwrap().end, 1);
        assert!(found(".", "", "\n\r\u{2028}\u{2029}").is_none());
        assert!(found("[^]", "", "\n").is_some());
        assert!(found("[]", "", "anything").is_none());
        assert!(found("^b$", "m", "a\nb\nc").is_some());
        assert!(found("a$", "", "a\n").is_none());
        assert!(found(r"\b\w+\b", "", "---word---").is_some());
        assert!(found(r"^[\dA-F]+\s\S$", "", "09AF x").is_some());
    }
    #[test]
    fn greedy_lazy_alternative_and_capture_rollback() {
        assert_eq!(found("a+", "", "aaaa").unwrap().end, 4);
        assert_eq!(found("a+?", "", "aaaa").unwrap().end, 1);
        assert_eq!(found("a{2,3}?a", "", "aaaa").unwrap().end, 3);
        assert_eq!(found("a|aa", "", "aa").unwrap().end, 1);
        let m = found("(a|ab)(b|c)", "", "abc").unwrap();
        assert_eq!(m.captures, vec![Some((0, 2)), Some((0, 1)), Some((1, 2))]);
        let m = found("(a(b)?)+", "", "aba").unwrap();
        assert_eq!(m.captures[1], Some((2, 3)));
        assert_eq!(m.captures[2], None);
    }
    #[test]
    fn empty_repeats_and_backreferences_are_bounded_and_match_unset_groups() {
        assert_eq!(
            found("(a*)*", "", "b").unwrap().captures,
            vec![Some((0, 0)), None]
        );
        assert_eq!(
            found("(){2,}", "", "").unwrap().captures,
            vec![Some((0, 0)), Some((0, 0))]
        );
        assert!(found(r"^(a)?\1b$", "", "b").is_some());
        assert!(found(r"^\1(a)$", "", "a").is_some());
        assert!(found(r"^(ab)\1$", "i", "aBAb").is_some());
    }
    #[test]
    fn lookahead_is_atomic_and_captures_do_not_leak_from_negative_matches() {
        let m = found(r"(?=(a+))a*b\1", "", "baaabac").unwrap();
        assert_eq!(m.start, 3);
        assert_eq!(m.end, 6);
        assert_eq!(m.captures[1], Some((3, 4)));
        let m = found(r"^(?!(a))\1b$", "", "b").unwrap();
        assert_eq!(m.captures[1], None);
    }
    #[test]
    fn es5_ignore_case_does_not_apply_full_unicode_case_folding() {
        assert!(found("[a-z]+", "i", "ABC").is_some());
        assert!(found("[Σ]", "i", "ς").is_some());
        assert!(found("[a-z]", "i", "ſ").is_none());
        assert!(found("s", "i", "ſ").is_none());
        assert!(found("ß", "i", "SS").is_none());
        assert!(found("k", "i", "K").is_none());
    }
    #[test]
    fn invalid_patterns_and_unsupported_extensions_are_explicit() {
        for pattern in [
            "(",
            "[",
            "a{2,1}",
            "a**",
            "(?<=a)",
            "(?<name>a)",
            r"\p{L}",
            r"\u{1f600}",
            r"\2(a)",
            r"[\1]",
            r"[z-a]",
            r"[\d-a]",
            r"\01",
            "^*",
        ] {
            assert!(
                matches!(Regex::compile(&units(pattern), ""), Err(Error::Syntax(_))),
                "{pattern}"
            );
        }
        for flags in ["gg", "u", "s", "y", "d", "imx"] {
            assert!(matches!(Regex::compile(&[], flags), Err(Error::Syntax(_))));
        }
    }
    #[test]
    fn fuel_state_and_compile_limits_return_errors_without_recursive_matching() {
        let regex = Regex::compile(&units("^(a|aa)*b$"), "").unwrap();
        let mut fuel = 250;
        assert!(matches!(
            regex.find(&units("aaaaaaaaaaaaaaaaaaaa"), 0, &mut fuel),
            Err(Error::Limit(_))
        ));
        assert!(fuel < 250);
        assert!(matches!(
            Regex::compile(&units(&"(".repeat(100)), ""),
            Err(Error::Limit(_))
        ));
        assert!(matches!(
            Regex::compile(&vec![97; MAX_PATTERN + 1], ""),
            Err(Error::Limit(_))
        ));
        assert!(matches!(
            Regex::compile(&units("a{1000001}"), ""),
            Err(Error::Limit(_))
        ));
        let regex = Regex::compile(&[], "").unwrap();
        assert!(regex.find(&[], 1, &mut 1).unwrap().is_none());
        assert!(matches!(
            regex.find(&vec![97; MAX_INPUT + 1], 0, &mut 1),
            Err(Error::Limit(_))
        ));
    }
}
