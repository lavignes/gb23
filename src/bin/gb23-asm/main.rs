use std::{
    error::Error,
    fs::File,
    io::{self, Read, Seek, Write},
    mem,
    path::PathBuf,
    process::ExitCode,
};

use clap::Parser;
use lex::{
    Dir, Label, Lexer, Macro, MacroInvocation, MacroTok, Op, StrInterner, Tok, TokInterner,
    TokStream,
};

mod lex;

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Args {
    /// Input file
    input: PathBuf,

    /// Output file (default: stdout)
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Symbol file
    #[arg(short, long)]
    sym: Option<PathBuf>,
}

fn main() -> ExitCode {
    if let Err(e) = main_real() {
        eprintln!("{e}");
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn main_real() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    let file = File::open(args.input).map_err(|e| format!("cant open file: {e}"))?;
    let lexer = Lexer::new(file);
    let output: Box<dyn Write> = match args.output {
        Some(path) => Box::new(
            File::options()
                .write(true)
                .create(true)
                .truncate(true)
                .open(path)
                .map_err(|e| format!("cant open file: {e}"))?,
        ),
        None => Box::new(io::stdout()),
    };

    let mut asm = Asm::new(lexer, output);

    eprint!("pass1: ");
    asm.pass()?;
    eprintln!("ok");

    eprint!("pass2: ");
    asm.rewind()?;
    asm.pass()?;
    eprintln!("ok");

    eprintln!("== stats ==");
    eprintln!("symbols: {}", asm.syms.len());
    eprintln!(
        "string heap: {}/{} bytes",
        asm.str_int
            .storages()
            .iter()
            .fold(0, |accum, storage| accum + storage.len()),
        asm.str_int
            .storages()
            .iter()
            .fold(0, |accum, storage| accum + storage.capacity())
    );
    eprintln!(
        "macro heap: {}/{} bytes",
        asm.tok_int.storages().iter().fold(0, |accum, storage| accum
            + (storage.len() * mem::size_of::<MacroTok>())),
        asm.tok_int.storages().iter().fold(0, |accum, storage| accum
            + (storage.capacity() * mem::size_of::<MacroTok>()))
    );
    Ok(())
}

#[derive(Clone, Copy)]
enum Segment {
    ROM(u16),  // ROM0 $0000-$3FFF, ROMX $4000-$7FFF
    WRAM(u16), // WRAM0 $C000-$CFFF, WRAMX $D000-$DFFF
    SRAM(u16), // $A000-$BFFF
    VRAM(u16), // $8000-$9FFF
    HRAM,      // $FF00-$FFFF
}

#[derive(Clone, Copy)]
struct Sym {
    value: i32,
    bank: u16,
}

struct Asm<'a> {
    toks: Vec<Box<dyn TokStream + 'a>>,
    syms: Vec<(Label<'a>, Sym)>,
    str_int: StrInterner<'a>,
    tok_int: TokInterner<'a>,
    output: Box<dyn Write>,
    pc: u16,
    pc_end: bool,
    dat: u16,
    dat_end: bool,
    segment: Segment,

    scope: Option<&'a str>,
    emit: bool,
    if_level: usize,

    macros: Vec<Macro<'a>>,
    values: Vec<i32>,
    operators: Vec<Op>,
}

impl<'a> Asm<'a> {
    fn new<R: Read + Seek + 'static>(lexer: Lexer<R>, output: Box<dyn Write>) -> Self {
        Self {
            toks: vec![Box::new(lexer)],
            syms: Vec::new(),
            str_int: StrInterner::new(),
            tok_int: TokInterner::new(),
            output,
            pc: 0,
            pc_end: false,
            dat: 0,
            dat_end: false,
            segment: Segment::ROM(0),
            scope: None,
            emit: false,
            if_level: 0,
            macros: Vec::new(),
            values: Vec::new(),
            operators: Vec::new(),
        }
    }

    fn rewind(&mut self) -> io::Result<()> {
        self.toks.last_mut().unwrap().rewind()?;
        self.pc = 0;
        self.pc_end = false;
        self.dat = 0;
        self.dat_end = false;
        self.segment = Segment::ROM(0);
        self.scope = None;
        self.emit = true;
        self.if_level = 0;
        self.macros.clear();
        Ok(())
    }

    fn pass(&mut self) -> io::Result<()> {
        loop {
            if self.peek()? == Tok::EOF {
                if self.toks.len() <= 1 {
                    break;
                }
                self.toks.pop();
            }
            // special case, setting the PC
            if self.peek()? == Tok::STAR {
                self.eat();
                if self.peek()? != Tok::EQU {
                    return Err(self.err("expected ="));
                }
                self.eat();
                let expr = self.expr()?;
                self.set_pc(self.const_16(expr)?);
                self.eol()?;
                continue;
            }
            // is this a label?
            if self.peek()? == Tok::IDENT {
                // is this a macro?
                if let Some(mac) = self
                    .macros
                    .iter()
                    .find(|mac| self.str() == mac.name())
                    .copied()
                {
                    let line = self.tok().line();
                    self.eat();
                    let mut args = Vec::new();
                    if self.peek()? == Tok::LPAREN {
                        self.eat();
                        loop {
                            match self.peek()? {
                                Tok::RPAREN => break,
                                Tok::IDENT => args.push(MacroTok::Ident(self.str_intern())),
                                Tok::DIR => args.push(MacroTok::Dir(self.str_intern())),
                                Tok::MNE => args.push(MacroTok::Mne(self.str_intern())),
                                Tok::STR => args.push(MacroTok::Str(self.str_intern())),
                                Tok::NUM => args.push(MacroTok::Num(self.tok().num())),
                                tok => args.push(MacroTok::Tok(tok)),
                            }
                            self.eat();
                            if self.peek()? != Tok::COMMA {
                                break;
                            }
                            self.eat();
                        }
                        self.eat();
                    }
                    self.toks
                        .push(Box::new(MacroInvocation::new(mac, line, args)));
                    continue;
                }
                let string = self.str_intern();
                let label = if !self.str().starts_with(".") {
                    self.scope.replace(string);
                    Label::new(None, string)
                } else {
                    Label::new(self.scope, string)
                };
                self.eat();
                // is this label being defined to a macro?
                if (self.peek()? == Tok::DIR) && self.str_like(Dir::MACRO) {
                    if label.string().starts_with(".") {
                        return Err(self.err("macro must be global"));
                    }
                    self.eat();
                    self.macrodef(label)?;
                    self.eol()?;
                    continue;
                }
                let index = if let Some((index, _)) = self
                    .syms
                    .iter()
                    .enumerate()
                    .find(|(_, item)| item.0 == label)
                {
                    // allowed to redef during second pass
                    // TODO: should test if value didnt change
                    if !self.emit {
                        return Err(self.err("symbol already defined"));
                    }
                    index
                } else {
                    // save in the symbol table with default value
                    let index = self.syms.len();
                    self.syms.push((
                        label,
                        Sym {
                            value: 0,
                            bank: self.bank(),
                        },
                    ));
                    index
                };
                // being defined to value?
                if self.peek()? == Tok::EQU {
                    self.eat();
                    let expr = self.expr()?;
                    if self.emit {
                        self.syms[index].1 = Sym {
                            value: self.const_expr(expr)?,
                            bank: self.bank(),
                        };
                    } else if let Some(value) = expr {
                        self.syms[index].1 = Sym {
                            value,
                            bank: self.bank(),
                        };
                    } else {
                        // not solved, remove it for now
                        self.syms.pop();
                    }
                    self.eol()?;
                    continue;
                }
                // otherwise it is a pointer to the current PC
                self.syms[index].1 = Sym {
                    value: self.pc() as u32 as i32,
                    bank: self.bank(),
                };
                continue;
            }
            // directive?
            if self.peek()? == Tok::DIR {
                self.directive()?;
                self.eol()?;
                continue;
            }
            // must be mnemonic
            if self.peek()? == Tok::MNE {
                self.mnemonic()?;
            }
            self.eol()?;
        }
        Ok(())
    }

    fn peek(&mut self) -> io::Result<Tok> {
        self.tok_mut().peek()
    }

    fn eat(&mut self) {
        self.tok_mut().eat();
    }

    fn tok(&self) -> &dyn TokStream {
        self.toks.last().unwrap().as_ref()
    }

    fn tok_mut(&mut self) -> &mut dyn TokStream {
        self.toks.last_mut().unwrap().as_mut()
    }

    fn err(&self, msg: &str) -> io::Error {
        self.tok().err(msg)
    }

    fn str(&self) -> &str {
        self.tok().str()
    }

    fn str_like<S: AsRef<str>>(&self, string: S) -> bool {
        self.tok().str().eq_ignore_ascii_case(string.as_ref())
    }

    fn str_intern(&mut self) -> &'a str {
        let Self {
            ref mut str_int,
            toks,
            ..
        } = self;
        let string = toks.last().unwrap().str();
        str_int.intern(string)
    }

    fn eol(&mut self) -> io::Result<()> {
        match self.peek()? {
            Tok::NEWLINE => {
                self.eat();
                Ok(())
            }
            Tok::EOF => {
                if self.toks.len() > 1 {
                    self.toks.pop();
                }
                Ok(())
            }
            _ => Err(self.err("expected end of line")),
        }
    }

    fn pc(&self) -> u16 {
        match self.segment {
            Segment::ROM(_) => self.pc,
            _ => self.dat,
        }
    }

    fn set_pc(&mut self, val: u16) {
        match self.segment {
            Segment::ROM(_) => self.pc = val,
            _ => self.dat = val,
        }
    }

    fn bank(&self) -> u16 {
        match self.segment {
            Segment::ROM(bank)
            | Segment::WRAM(bank)
            | Segment::SRAM(bank)
            | Segment::VRAM(bank) => bank,
            Segment::HRAM => 0,
        }
    }

    fn const_expr(&self, expr: Option<i32>) -> io::Result<i32> {
        expr.ok_or_else(|| self.err("expression unsolved"))
    }

    fn const_16(&self, expr: Option<i32>) -> io::Result<u16> {
        let expr = self.const_expr(expr)?;
        if (expr as u32) > (u16::MAX as u32) {
            return Err(self.err("expression >2 bytes"));
        }
        Ok(expr as u16)
    }

    fn const_8(&self, expr: Option<i32>) -> io::Result<u8> {
        let expr = self.const_expr(expr)?;
        if (expr as u32) > (u8::MAX as u32) {
            return Err(self.err("expression >1 byte"));
        }
        Ok(expr as u8)
    }

    fn expr_precedence(&self, op: Op) -> u8 {
        match op {
            Op::Unary(Tok::LPAREN) => 0xFF, // lparen is lowest precedence
            Op::Unary(_) => 0,              // other unary is highest precedence
            Op::Binary(Tok::SOLIDUS | Tok::MODULUS | Tok::STAR) => 1,
            Op::Binary(Tok::PLUS | Tok::MINUS) => 2,
            Op::Binary(Tok::ASL | Tok::ASR | Tok::LSR) => 3,
            Op::Binary(Tok::LT | Tok::LTE | Tok::GT | Tok::GTE) => 4,
            Op::Binary(Tok::EQ | Tok::NEQ) => 5,
            Op::Binary(Tok::AMP) => 6,
            Op::Binary(Tok::CARET) => 7,
            Op::Binary(Tok::PIPE) => 8,
            Op::Binary(Tok::AND) => 9,
            Op::Binary(Tok::OR) => 10,
            _ => unreachable!(),
        }
    }

    fn expr_apply(&mut self, op: Op) {
        let rhs = self.values.pop().unwrap();
        match op {
            Op::Unary(Tok::PLUS) => self.values.push(rhs),
            Op::Unary(Tok::MINUS) => self.values.push(-rhs),
            Op::Unary(Tok::TILDE) => self.values.push(!rhs),
            Op::Unary(Tok::BANG) => self.values.push((rhs == 0) as i32),
            Op::Unary(Tok::LT) => self.values.push(((rhs as u32) & 0xFF) as i32),
            Op::Unary(Tok::GT) => self.values.push((((rhs as u32) & 0xFF00) >> 8) as i32),
            Op::Binary(tok) => {
                let lhs = self.values.pop().unwrap();
                match tok {
                    Tok::PLUS => self.values.push(lhs.wrapping_add(rhs)),
                    Tok::MINUS => self.values.push(lhs.wrapping_sub(rhs)),
                    Tok::STAR => self.values.push(lhs.wrapping_mul(rhs)),
                    Tok::SOLIDUS => self.values.push(lhs.wrapping_div(rhs)),
                    Tok::MODULUS => self.values.push(lhs.wrapping_rem(rhs)),
                    Tok::ASL => self.values.push(lhs.wrapping_shl(rhs as u32)),
                    Tok::ASR => self.values.push(lhs.wrapping_shr(rhs as u32)),
                    Tok::LSR => self
                        .values
                        .push((lhs as u32).wrapping_shl(rhs as u32) as i32),
                    Tok::LT => self.values.push((lhs < rhs) as i32),
                    Tok::LTE => self.values.push((lhs <= rhs) as i32),
                    Tok::GT => self.values.push((lhs > rhs) as i32),
                    Tok::GTE => self.values.push((lhs >= rhs) as i32),
                    Tok::EQ => self.values.push((lhs == rhs) as i32),
                    Tok::NEQ => self.values.push((lhs != rhs) as i32),
                    Tok::AMP => self.values.push(lhs & rhs),
                    Tok::PIPE => self.values.push(lhs | rhs),
                    Tok::CARET => self.values.push(lhs ^ rhs),
                    Tok::AND => self.values.push(((lhs != 0) && (rhs != 0)) as i32),
                    Tok::OR => self.values.push(((lhs != 0) || (rhs != 0)) as i32),
                    _ => unreachable!(),
                }
            }
            _ => unreachable!(),
        }
    }

    fn expr_push_apply(&mut self, op: Op) {
        while let Some(top) = self.operators.last() {
            if self.expr_precedence(*top) > self.expr_precedence(op) {
                break;
            }
            self.expr_apply(*top);
            self.operators.pop();
        }
        self.operators.push(op);
    }

    fn expr(&mut self) -> io::Result<Option<i32>> {
        self.values.clear();
        self.operators.clear();
        let mut seen_val = false;
        let mut paren_depth = 0;
        let mut seen_unknown_label = false;
        loop {
            match self.peek()? {
                // star is multiply or the PC
                Tok::STAR => {
                    if !seen_val {
                        self.values.push(self.pc() as u32 as i32);
                        seen_val = true;
                        self.eat();
                        continue;
                    }
                    self.expr_push_apply(Op::Binary(Tok::STAR));
                    seen_val = false;
                    self.eat();
                    continue;
                }
                // these are optionally unary
                tok @ (Tok::PLUS | Tok::MINUS | Tok::LT | Tok::GT) => {
                    if seen_val {
                        self.expr_push_apply(Op::Binary(tok));
                    } else {
                        self.expr_push_apply(Op::Unary(tok));
                    }
                    seen_val = false;
                    self.eat();
                    continue;
                }
                // always unary
                tok @ (Tok::BANG | Tok::TILDE) => {
                    if !seen_val {
                        return Err(self.err("expected value"));
                    }
                    self.expr_push_apply(Op::Unary(tok));
                    seen_val = false;
                    self.eat();
                    continue;
                }
                #[rustfmt::skip]
                tok @ (Tok::PIPE | Tok::AND | Tok::OR | Tok::SOLIDUS | Tok::MODULUS | Tok::ASL
                      | Tok::ASR | Tok::LSR | Tok::LTE | Tok::GTE | Tok::EQ | Tok::NEQ) => {
                    if !seen_val {
                        return Err(self.err("expected value"));
                    }
                    self.expr_push_apply(Op::Binary(tok));
                    seen_val = false;
                    self.eat();
                    continue;
                }
                Tok::NUM => {
                    if seen_val {
                        return Err(self.err("expected operator"));
                    }
                    self.values.push(self.tok().num());
                    seen_val = true;
                    self.eat();
                    continue;
                }
                Tok::LPAREN => {
                    if seen_val {
                        return Err(self.err("expected operator"));
                    }
                    paren_depth += 1;
                    self.operators.push(Op::Unary(Tok::LPAREN));
                    seen_val = false;
                    self.eat();
                    continue;
                }
                Tok::RPAREN => {
                    // this rparen is probably part of the indirect address
                    if self.operators.is_empty() && (paren_depth == 0) {
                        break;
                    }
                    paren_depth -= 1;
                    if !seen_val {
                        return Err(self.err("expected value"));
                    }
                    loop {
                        if let Some(op) = self.operators.pop() {
                            // we apply ops until we see the start of this grouping
                            match op {
                                Op::Binary(tok) | Op::Unary(tok) if tok == Tok::LPAREN => {
                                    break;
                                }
                                _ => {}
                            }
                            self.expr_apply(op);
                        } else {
                            return Err(self.err("unbalanced parens"));
                        }
                    }
                    self.eat();
                    continue;
                }
                Tok::IDENT => {
                    let string = self.str_intern();
                    let label = if !self.str().starts_with(".") {
                        Label::new(None, string)
                    } else {
                        Label::new(self.scope, string)
                    };
                    if let Some(sym) = self.syms.iter().find(|sym| &sym.0 == &label).copied() {
                        if seen_val {
                            return Err(self.err("expected operator"));
                        }
                        self.values.push(sym.1.value);
                        seen_val = true;
                        self.eat();
                        continue;
                    }
                    seen_unknown_label = true;
                    if seen_val {
                        return Err(self.err("expected operator"));
                    }
                    self.values.push(1);
                    seen_val = true;
                    self.eat();
                    continue;
                }
                _ => break,
            }
        }
        while let Some(top) = self.operators.pop() {
            self.expr_apply(top);
        }
        if seen_unknown_label {
            return Ok(None);
        }
        if let Some(value) = self.values.pop() {
            return Ok(Some(value));
        }
        Err(self.err("expected value"))
    }

    fn macrodef(&mut self, label: Label<'a>) -> io::Result<()> {
        self.eol()?;
        let mut toks = Vec::new();
        let mut if_level = 0;
        loop {
            if self.peek()? == Tok::DIR {
                if self.str_like(Dir::IF)
                    || self.str_like(Dir::IFDEF)
                    || self.str_like(Dir::IFNDEF)
                    || self.str_like(Dir::MACRO)
                {
                    if_level += 1;
                } else if self.str_like(Dir::END) {
                    if if_level == 0 {
                        self.eat();
                        toks.push(MacroTok::Tok(Tok::EOF));
                        break;
                    }
                    if_level -= 1;
                }
            }
            match self.peek()? {
                Tok::EOF => return Err(self.err("unexpected end of file")),
                Tok::IDENT => toks.push(MacroTok::Ident(self.str_intern())),
                Tok::DIR => toks.push(MacroTok::Dir(self.str_intern())),
                Tok::MNE => toks.push(MacroTok::Mne(self.str_intern())),
                Tok::STR => toks.push(MacroTok::Str(self.str_intern())),
                Tok::NUM => toks.push(MacroTok::Num(self.tok().num())),
                Tok::ARG => toks.push(MacroTok::Arg((self.tok().num() as usize) - 1)),
                tok => toks.push(MacroTok::Tok(tok)),
            }
            self.eat();
        }
        let toks = self.tok_int.intern(&toks);
        self.macros.push(Macro::new(label.string(), toks));
        Ok(())
    }

    fn directive(&mut self) -> io::Result<()> {
        if self.str_like(Dir::ADJ) {
            self.eat();
            let expr = self.expr()?;
            let expr = self.const_16(expr)?;
            self.set_pc(expr);
            return Ok(());
        }
        if self.str_like(Dir::DB) {
            self.eat();
            loop {
                if self.peek()? == Tok::STR {
                    let string = self.str_intern();
                    self.eat();
                    if self.emit {
                        for b in string.bytes() {}
                    }
                } else {
                    let expr = self.expr()?;
                    if self.emit {
                        let expr = self.const_8(expr)?;
                    }
                }
                if self.peek()? != Tok::COMMA {
                    break;
                }
                self.eat();
            }
        }
        Ok(())
    }
}
