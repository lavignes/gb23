use std::{
    io::{self, ErrorKind, Read, Seek},
    marker::PhantomData,
    slice, str,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Dir(&'static str);

impl Dir {
    pub const ADJ: Self = Self("ADJ");
    pub const DB: Self = Self("DB");
    pub const DW: Self = Self("DW");
    pub const END: Self = Self("END");
    pub const IF: Self = Self("IF");
    pub const IFDEF: Self = Self("IFDEF");
    pub const IFNDEF: Self = Self("IFNDEF");
    pub const INCBIN: Self = Self("INCBIN");
    pub const INCLUDE: Self = Self("INCLUDE");
    pub const MACRO: Self = Self("MACRO");
    pub const PAD: Self = Self("PAD");
    pub const SEGMENT: Self = Self("SEGMENT");
}

impl AsRef<str> for Dir {
    fn as_ref(&self) -> &str {
        self.0
    }
}

const DIRECTIVES: &[Dir] = &[
    Dir::ADJ,
    Dir::DB,
    Dir::DW,
    Dir::END,
    Dir::IF,
    Dir::IFDEF,
    Dir::IFNDEF,
    Dir::INCBIN,
    Dir::INCLUDE,
    Dir::MACRO,
    Dir::PAD,
    Dir::SEGMENT,
];

#[derive(PartialEq, Eq)]
pub struct Mne(&'static str);

impl Mne {
    pub const ADC: Self = Self("ADC");
    pub const ADD: Self = Self("ADD");
    pub const AND: Self = Self("AND");
    pub const BIT: Self = Self("BIT");
    pub const CALL: Self = Self("CALL");
    pub const CCF: Self = Self("CCF");
    pub const CP: Self = Self("CP");
    pub const CPL: Self = Self("CPL");
    pub const DAA: Self = Self("DAA");
    pub const DEC: Self = Self("DEC");
    pub const DI: Self = Self("DI");
    pub const EI: Self = Self("EI");
    pub const HALT: Self = Self("HALT");
    pub const INC: Self = Self("INC");
    pub const JP: Self = Self("JP");
    pub const JR: Self = Self("JR");
    pub const LD: Self = Self("LD");
    pub const LDH: Self = Self("LDH");
    pub const NOP: Self = Self("NOP");
    pub const OR: Self = Self("OR");
    pub const POP: Self = Self("POP");
    pub const PUSH: Self = Self("PUSH");
    pub const RES: Self = Self("RES");
    pub const RET: Self = Self("RET");
    pub const RETI: Self = Self("RETI");
    pub const RL: Self = Self("RL");
    pub const RLA: Self = Self("RLA");
    pub const RLC: Self = Self("RLC");
    pub const RLCA: Self = Self("RLCA");
    pub const RR: Self = Self("RR");
    pub const RRA: Self = Self("RRA");
    pub const RRC: Self = Self("RRC");
    pub const RRCA: Self = Self("RRCA");
    pub const RST: Self = Self("RST");
    pub const SBC: Self = Self("SBC");
    pub const SCF: Self = Self("SCF");
    pub const SET: Self = Self("SET");
    pub const SLA: Self = Self("SLA");
    pub const SRA: Self = Self("SRA");
    pub const SRL: Self = Self("SRL");
    pub const STOP: Self = Self("STOP");
    pub const SUB: Self = Self("SUB");
    pub const SWAP: Self = Self("SWAP");
    pub const XOR: Self = Self("XOR");
}

impl AsRef<str> for Mne {
    fn as_ref(&self) -> &str {
        self.0
    }
}

const MNEMONICS: &[Mne] = &[
    Mne::ADC,
    Mne::ADD,
    Mne::AND,
    Mne::BIT,
    Mne::CALL,
    Mne::CCF,
    Mne::CP,
    Mne::CPL,
    Mne::DAA,
    Mne::DEC,
    Mne::DI,
    Mne::EI,
    Mne::HALT,
    Mne::INC,
    Mne::JP,
    Mne::JR,
    Mne::LD,
    Mne::LDH,
    Mne::NOP,
    Mne::OR,
    Mne::POP,
    Mne::PUSH,
    Mne::RES,
    Mne::RET,
    Mne::RETI,
    Mne::RL,
    Mne::RLA,
    Mne::RLC,
    Mne::RLCA,
    Mne::RR,
    Mne::RRA,
    Mne::RRC,
    Mne::RRCA,
    Mne::RST,
    Mne::SBC,
    Mne::SCF,
    Mne::SET,
    Mne::SLA,
    Mne::SRA,
    Mne::SRL,
    Mne::STOP,
    Mne::SUB,
    Mne::SWAP,
    Mne::XOR,
];

#[derive(Copy, Clone, PartialEq, Eq)]
pub struct Tok(u8);

#[rustfmt::skip]
impl Tok {
    pub const NEWLINE: Self = Self(b'\n');
    pub const MODULUS: Self = Self(b'%');
    pub const SOLIDUS: Self = Self(b'/');
    pub const STAR: Self = Self(b'*');
    pub const PLUS: Self = Self(b'+');
    pub const MINUS: Self = Self(b'-');
    pub const LT: Self = Self(b'<');
    pub const GT: Self = Self(b'>');
    pub const AMP: Self = Self(b'&');
    pub const CARET: Self = Self(b'^');
    pub const PIPE: Self = Self(b'|');
    pub const LPAREN: Self = Self(b'(');
    pub const RPAREN: Self = Self(b')');
    pub const LBRACK: Self = Self(b'[');
    pub const RBRACK: Self = Self(b']');
    pub const BANG: Self = Self(b'!');
    pub const TILDE: Self = Self(b'~');
    pub const COMMA: Self = Self(b',');
    pub const EQU: Self = Self(b'=');

    pub const A: Self = Self(b'A');
    pub const B: Self = Self(b'B');
    pub const C: Self = Self(b'C');
    pub const D: Self = Self(b'D');
    pub const E: Self = Self(b'E');
    pub const H: Self = Self(b'H');
    pub const L: Self = Self(b'L');
    pub const Z: Self = Self(b'Z');

    pub const EOF: Self = Self(0x80);
    pub const IDENT: Self = Self(0x81);
    pub const DIR: Self = Self(0x82);
    pub const MNE: Self = Self(0x83);
    pub const NUM: Self = Self(0x84);
    pub const STR: Self = Self(0x85);
    pub const ARG: Self = Self(0x86);

    pub const ASL: Self = Self(0x96); // <<
    pub const ASR: Self = Self(0x97); // >>
    pub const LSR: Self = Self(0x98); // ~>
    pub const LTE: Self = Self(0x99); // <=
    pub const GTE: Self = Self(0x9A); // >=
    pub const EQ: Self = Self(0x9B);  // ==
    pub const NEQ: Self = Self(0x9C); // !=
    pub const AND: Self = Self(0x9D); // &&
    pub const OR: Self = Self(0x9E);  // ||

    pub const AF: Self = Self(0xA0);
    pub const BC: Self = Self(0xA1);
    pub const DE: Self = Self(0xA2);
    pub const HL: Self = Self(0xA3);
    pub const SP: Self = Self(0xA4);
    pub const NC: Self = Self(0xA5);
    pub const NZ: Self = Self(0xA6);
}

const GRAPHEMES: &[(&[u8; 2], Tok)] = &[
    (b"<<", Tok::ASL),
    (b">>", Tok::ASR),
    (b"~>", Tok::LSR),
    (b"<=", Tok::LTE),
    (b">=", Tok::GTE),
    (b"==", Tok::EQ),
    (b"!=", Tok::NEQ),
    (b"&&", Tok::AND),
    (b"||", Tok::OR),
    (b"AF", Tok::AF),
    (b"BC", Tok::BC),
    (b"DE", Tok::DE),
    (b"HL", Tok::HL),
    (b"SP", Tok::SP),
    (b"NC", Tok::NC),
    (b"NZ", Tok::NZ),
];

#[derive(Clone, Copy)]
pub enum Op {
    Binary(Tok),
    Unary(Tok),
}

pub trait TokStream {
    fn err(&self, msg: &str) -> io::Error;

    fn peek(&mut self) -> io::Result<Tok>;

    fn eat(&mut self);

    fn rewind(&mut self) -> io::Result<()>;

    fn str(&self) -> &str;

    fn num(&self) -> i32;

    fn line(&self) -> usize;
}

pub struct StrInterner<'a> {
    storages: Vec<String>,
    marker: PhantomData<&'a ()>,
}

impl<'a> StrInterner<'a> {
    pub fn new() -> Self {
        Self {
            storages: Vec::new(),
            marker: PhantomData,
        }
    }

    pub fn intern(&mut self, string: &str) -> &'a str {
        let mut has_space = None;
        for (i, storage) in self.storages.iter().enumerate() {
            // pre-check if we have space for the string in case we have a cache miss
            if has_space.is_none() && ((storage.capacity() - storage.len()) >= string.len()) {
                has_space = Some(i);
            }
            if let Some(index) = storage.find(string) {
                // SAFETY: the assumption is that we never re-allocate storages
                unsafe {
                    return str::from_utf8_unchecked(slice::from_raw_parts(
                        storage.as_ptr().add(index),
                        string.len(),
                    ));
                }
            }
        }
        // cache miss, add to a storage if possible
        let storage = if let Some(index) = has_space {
            &mut self.storages[index]
        } else {
            self.storages.push(String::with_capacity(
                string.len().next_multiple_of(2).max(256),
            ));
            self.storages.last_mut().unwrap()
        };
        let index = storage.len();
        storage.push_str(string);
        // SAFETY: the assumption is that we never re-allocate storages
        unsafe {
            str::from_utf8_unchecked(slice::from_raw_parts(
                storage.as_ptr().add(index),
                string.len(),
            ))
        }
    }

    pub fn storages(&self) -> &[String] {
        &self.storages
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Label<'a> {
    scope: Option<&'a str>,
    string: &'a str,
}

impl<'a> Label<'a> {
    pub fn new(scope: Option<&'a str>, string: &'a str) -> Self {
        Self { scope, string }
    }

    pub fn string(&self) -> &'a str {
        self.string
    }
}

pub struct Lexer<R> {
    reader: PeekReader<R>,
    string: String,
    number: i32,
    stash: Option<Tok>,
    line: usize,
}

impl<R: Read + Seek> Lexer<R> {
    pub fn new(reader: R) -> Self {
        Self {
            reader: PeekReader::new(reader),
            string: String::new(),
            number: 0,
            stash: None,
            line: 1,
        }
    }
}

impl<R: Read + Seek> TokStream for Lexer<R> {
    fn err(&self, msg: &str) -> io::Error {
        io::Error::new(ErrorKind::InvalidData, format!("{}: {msg}", self.line))
    }

    fn peek(&mut self) -> io::Result<Tok> {
        if let Some(tok) = self.stash {
            return Ok(tok);
        }
        // skip whitespace
        while let Some(c) = self.reader.peek()? {
            if !b" \t\r".contains(&c) {
                break;
            }
            self.reader.eat();
        }
        // skip comment
        if let Some(b';') = self.reader.peek()? {
            while !matches!(self.reader.peek()?, Some(b'\n')) {
                self.reader.eat();
            }
        }
        match self.reader.peek()? {
            None => {
                self.reader.eat();
                self.stash = Some(Tok::EOF);
                Ok(Tok::EOF)
            }
            // macro argument
            Some(b'\\') => {
                self.reader.eat();
                while let Some(c) = self.reader.peek()? {
                    if !c.is_ascii_digit() {
                        break;
                    }
                    self.string.push(c as char);
                    self.reader.eat();
                }
                self.number =
                    i32::from_str_radix(&self.string, 10).map_err(|e| self.err(&e.to_string()))?;
                if self.number < 1 {
                    return Err(self.err("argument must be positive"));
                }
                self.stash = Some(Tok::ARG);
                Ok(Tok::ARG)
            }
            // number
            Some(c) if c.is_ascii_digit() || c == b'$' || c == b'%' => {
                let radix = match c {
                    b'$' => {
                        self.reader.eat();
                        16
                    }
                    b'%' => {
                        self.reader.eat();
                        2
                    }
                    _ => 10,
                };
                // edge case: modulus
                if (c == b'%') && self.reader.peek()?.is_some_and(|nc| !b"01".contains(&nc)) {
                    self.stash = Some(Tok::MODULUS);
                    return Ok(Tok::MODULUS);
                }
                // parse number
                while let Some(c) = self.reader.peek()? {
                    if c == b'_' {
                        continue; // allow '_' separators in numbers
                    }
                    if !c.is_ascii_alphanumeric() {
                        break;
                    }
                    self.string.push(c as char);
                    self.reader.eat();
                }
                self.number = i32::from_str_radix(&self.string, radix)
                    .map_err(|e| self.err(&e.to_string()))?;
                self.stash = Some(Tok::NUM);
                Ok(Tok::NUM)
            }
            // string
            Some(b'"') => {
                self.reader.eat();
                while let Some(c) = self.reader.peek()? {
                    if c == b'"' {
                        self.reader.eat();
                        break;
                    }
                    self.string.push(c as char);
                    self.reader.eat();
                }
                self.stash = Some(Tok::STR);
                Ok(Tok::STR)
            }
            // char
            Some(b'\'') => {
                self.reader.eat();
                if let Some(c) = self.reader.peek()? {
                    if c.is_ascii_graphic() {
                        self.reader.eat();
                        self.number = c as i32;
                        self.stash = Some(Tok::NUM);
                        return Ok(Tok::NUM);
                    }
                }
                Err(self.err("unexpected garbage"))
            }
            // idents and single chars
            Some(c) => {
                while let Some(c) = self.reader.peek()? {
                    if !c.is_ascii_alphanumeric() && !b"_.".contains(&c) {
                        break;
                    }
                    self.reader.eat();
                    self.string.push(c as char);
                }
                if self.string.len() > 1 {
                    if DIRECTIVES
                        .binary_search_by(|dir| dir.0.as_bytes().cmp(self.string.as_bytes()))
                        .is_ok()
                    {
                        self.stash = Some(Tok::DIR);
                        return Ok(Tok::DIR);
                    }
                    if MNEMONICS
                        .binary_search_by(|mne| mne.0.as_bytes().cmp(self.string.as_bytes()))
                        .is_ok()
                    {
                        self.stash = Some(Tok::MNE);
                        return Ok(Tok::MNE);
                    }
                    if self.string.len() > 16 {
                        return Err(self.err("label too long"));
                    }
                    self.stash = Some(Tok::IDENT);
                    return Ok(Tok::IDENT);
                }
                // the char wasn't an ident, so wasnt eaten
                if self.string.len() == 0 {
                    self.reader.eat();
                }
                // check for grapheme
                if let Some(nc) = self.reader.peek()? {
                    let s = &[c, nc];
                    if let Some(tok) = GRAPHEMES
                        .iter()
                        .find_map(|(gf, tok)| (*gf == s).then_some(tok))
                        .copied()
                    {
                        self.reader.eat();
                        self.stash = Some(tok);
                        return Ok(tok);
                    }
                }
                // else return an uppercase of whatever this char is
                self.stash = Some(Tok(c.to_ascii_uppercase()));
                Ok(Tok(c.to_ascii_uppercase()))
            }
        }
    }

    fn eat(&mut self) {
        self.string.clear();
        if let Some(Tok::NEWLINE) = self.stash.take() {
            self.line += 1;
        }
    }

    fn rewind(&mut self) -> io::Result<()> {
        self.string.clear();
        self.stash = None;
        self.line = 1;
        self.reader.rewind()
    }

    fn str(&self) -> &str {
        &self.string
    }

    fn num(&self) -> i32 {
        self.number
    }

    fn line(&self) -> usize {
        self.line
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MacroTok<'a> {
    Tok(Tok),
    Str(&'a str),
    Ident(&'a str),
    Dir(&'a str),
    Mne(&'a str),
    Num(i32),
    Arg(usize),
}

#[derive(Clone, Copy)]
pub struct Macro<'a> {
    name: &'a str,
    toks: &'a [MacroTok<'a>],
}

impl<'a> Macro<'a> {
    pub fn new(name: &'a str, toks: &'a [MacroTok<'a>]) -> Self {
        Self { name, toks }
    }

    pub fn name(&self) -> &'a str {
        self.name
    }
}

pub struct MacroInvocation<'a> {
    mac: Macro<'a>,
    line: usize,
    index: usize,
    args: Vec<MacroTok<'a>>,
}

impl<'a> MacroInvocation<'a> {
    pub fn new(mac: Macro<'a>, line: usize, args: Vec<MacroTok<'a>>) -> Self {
        Self {
            mac,
            line,
            index: 0,
            args,
        }
    }
}

impl<'a> TokStream for MacroInvocation<'a> {
    fn err(&self, msg: &str) -> io::Error {
        io::Error::new(
            ErrorKind::InvalidData,
            format!("{}:{}: {msg}", self.line, self.mac.name),
        )
    }

    fn peek(&mut self) -> io::Result<Tok> {
        match self.mac.toks[self.index] {
            MacroTok::Tok(tok) => Ok(tok),
            MacroTok::Str(_) => Ok(Tok::STR),
            MacroTok::Ident(_) => Ok(Tok::IDENT),
            MacroTok::Dir(_) => Ok(Tok::DIR),
            MacroTok::Mne(_) => Ok(Tok::MNE),
            MacroTok::Num(_) => Ok(Tok::NUM),
            MacroTok::Arg(index) => {
                if index >= self.args.len() {
                    return Err(self.err("argument is undefined"));
                }
                match self.args[index] {
                    MacroTok::Tok(tok) => Ok(tok),
                    MacroTok::Str(_) => Ok(Tok::STR),
                    MacroTok::Ident(_) => Ok(Tok::IDENT),
                    MacroTok::Dir(_) => Ok(Tok::DIR),
                    MacroTok::Mne(_) => Ok(Tok::MNE),
                    MacroTok::Num(_) => Ok(Tok::NUM),
                    _ => unreachable!(),
                }
            }
        }
    }

    fn eat(&mut self) {
        self.index += 1;
    }

    fn rewind(&mut self) -> io::Result<()> {
        self.index = 0;
        Ok(())
    }

    fn str(&self) -> &str {
        match self.mac.toks[self.index] {
            MacroTok::Str(string) => string,
            MacroTok::Ident(string) => string,
            MacroTok::Dir(string) => string,
            MacroTok::Mne(string) => string,
            MacroTok::Arg(index) => match self.args[index] {
                MacroTok::Str(string) => string,
                MacroTok::Ident(string) => string,
                MacroTok::Dir(string) => string,
                MacroTok::Mne(string) => string,
                _ => unreachable!(),
            },
            _ => unreachable!(),
        }
    }

    fn num(&self) -> i32 {
        match self.mac.toks[self.index] {
            MacroTok::Num(val) => val,
            MacroTok::Arg(index) => match self.args[index] {
                MacroTok::Num(val) => val,
                _ => unreachable!(),
            },
            _ => unreachable!(),
        }
    }

    fn line(&self) -> usize {
        self.line
    }
}

pub struct TokInterner<'a> {
    storages: Vec<Vec<MacroTok<'a>>>,
}

impl<'a> TokInterner<'a> {
    pub fn new() -> Self {
        Self {
            storages: Vec::new(),
        }
    }

    pub fn intern(&mut self, toks: &[MacroTok<'a>]) -> &'a [MacroTok<'a>] {
        let mut has_space = None;
        for (i, storage) in self.storages.iter().enumerate() {
            // pre-check if we have space for the toks in case we have a cache miss
            if has_space.is_none() && ((storage.capacity() - storage.len()) >= toks.len()) {
                has_space = Some(i);
            }
            if let Some(index) = storage.windows(toks.len()).position(|win| win == toks) {
                // SAFETY: the assumption is that we never re-allocate storages
                unsafe {
                    return slice::from_raw_parts(storage.as_ptr().add(index), toks.len());
                }
            }
        }
        // cache miss, add to a storage if possible
        let storage = if let Some(index) = has_space {
            &mut self.storages[index]
        } else {
            self.storages.push(Vec::with_capacity(toks.len().max(256)));
            self.storages.last_mut().unwrap()
        };
        let index = storage.len();
        storage.extend_from_slice(toks);
        // SAFETY: the assumption is that we never re-allocate storages
        unsafe { slice::from_raw_parts(storage.as_ptr().add(index), toks.len()) }
    }

    pub fn storages(&'a self) -> &[Vec<MacroTok<'a>>] {
        &self.storages
    }
}

struct PeekReader<R> {
    inner: R,
    stash: Option<u8>,
}

impl<R: Read + Seek> PeekReader<R> {
    fn new(reader: R) -> Self {
        Self {
            inner: reader,
            stash: None,
        }
    }

    fn peek(&mut self) -> io::Result<Option<u8>> {
        if self.stash.is_none() {
            let mut buf = [0];
            self.stash = self
                .inner
                .read(&mut buf)
                .map(|n| if n == 0 { None } else { Some(buf[0]) })?;
        }
        Ok(self.stash)
    }

    fn eat(&mut self) {
        self.stash.take();
    }

    fn rewind(&mut self) -> io::Result<()> {
        self.stash = None;
        self.inner.rewind()
    }
}
