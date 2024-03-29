use core::slice;
use std::{
    fs::File,
    io::{self, Read},
    mem,
    path::PathBuf,
    process::ExitCode,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use clap::Parser;
use gb23::emu::{
    bus::{Bus, BusDevice, Port},
    cpu::{Flag, WideRegister},
    mbc::mbc1::Mbc1,
    Emu,
};
use rustyline::{
    completion::Completer, error::ReadlineError, hint::HistoryHinter, Completer, Config, Context,
    Editor, Helper, Highlighter, Hinter, Validator,
};
use sdl2::{
    audio::{AudioQueue, AudioSpecDesired},
    keyboard::Scancode,
    pixels::PixelFormatEnum,
    rect::Rect,
    EventPump,
};
use tracing::Level;

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Args {
    /// Path to ROM file
    rom: PathBuf,

    /// Path to BIOS/BOOT ROM file
    #[arg(short, long)]
    boot: Option<PathBuf>,

    /// One of `TRACE`, `DEBUG`, `INFO`, `WARN`, or `ERROR`
    #[arg(short, long, default_value_t = Level::INFO)]
    log_level: Level,

    /// Start with debugger enabled
    #[arg(short, long)]
    debug: bool,

    /// Debugger symbol file
    #[arg(short, long)]
    sym: Option<PathBuf>,
}

fn main() -> ExitCode {
    let args = Args::parse();
    tracing_subscriber::fmt()
        .with_max_level(args.log_level)
        .with_writer(io::stderr)
        .init();
    if let Err(e) = main_real(args) {
        tracing::error!("{e}");
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

struct LineCompleter {
    completions: Vec<String>,
}

impl LineCompleter {
    fn new() -> Self {
        Self {
            completions: Vec::new(),
        }
    }

    fn add<S: ToString>(&mut self, string: S) {
        self.completions.push(string.to_string());
    }
}

impl Completer for LineCompleter {
    type Candidate = String;

    fn complete(
        &self,
        line: &str,
        _pos: usize,
        _ctx: &Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Self::Candidate>)> {
        let words = line.split_whitespace();
        if let Some(last) = words.last() {
            let mut all_completions = Vec::new();
            for completion in self.completions.iter() {
                if completion.starts_with(last) {
                    all_completions.push(completion.clone());
                }
            }
            if !all_completions.is_empty() {
                let pos = line.rfind(last).unwrap();
                return Ok((pos, all_completions));
            }
        }
        Ok((0, Vec::new()))
    }
}

#[derive(Helper, Completer, Hinter, Highlighter, Validator)]
struct LineHelper {
    #[rustyline(Hinter)]
    hinter: HistoryHinter,
    #[rustyline(Completer)]
    completer: LineCompleter,
}

fn main_real(args: Args) -> Result<(), String> {
    let mut rom = Vec::new();
    File::open(&args.rom)
        .map_err(|e| format!("failed to open ROM file: {e}"))?
        .read_to_end(&mut rom)
        .map_err(|e| format!("failed to read ROM file: {e}"))?;
    let mut boot_data = Vec::new();
    if let Some(boot) = &args.boot {
        File::open(boot)
            .map_err(|e| format!("failed to open BIOS file: {e}"))?
            .read_to_end(&mut boot_data)
            .map_err(|e| format!("failed to read BIOS file: {e}"))?;
    }
    let sdl = sdl2::init().map_err(|e| format!("failed to initialize SDL2: {e}"))?;
    let event_pump = sdl
        .event_pump()
        .map_err(|e| format!("failed to initialize SDL2 events: {e}"))?;
    let video = sdl
        .video()
        .map_err(|e| format!("failed to initialize SDL2 video: {e}"))?;

    let audio = sdl
        .audio()
        .map_err(|e| format!("failed to initialize SDL2 audio: {e}"))?;
    let audio_queue: AudioQueue<f32> = audio
        .open_queue(
            None,
            &AudioSpecDesired {
                freq: Some(22050),
                channels: Some(2),
                samples: Some(512),
            },
        )
        .map_err(|e| format!("failed to open audio device: {e}"))?;
    let mut buf = Vec::new();
    for i in 0..(4096 * 5) {
        buf.push(((i as f32) * 0.05).sin() * 0.1);
    }
    audio_queue.queue_audio(&buf).unwrap();
    audio_queue.resume();

    let window = video
        .window("gb23", 160 * 8, 144 * 8)
        .allow_highdpi()
        .position_centered()
        .build()
        .map_err(|e| format!("failed to create window: {e}"))?;
    let mut canvas = window
        .into_canvas()
        .accelerated()
        .present_vsync() // TODO: using the vsync to sync the emulator right now
        .build()
        .map_err(|e| format!("failed to map window to canvas: {e}"))?;
    let texture_creator = canvas.texture_creator();
    let mut texture = texture_creator
        .create_texture_streaming(PixelFormatEnum::RGBA8888, 256, 256)
        .map_err(|e| format!("failed to create texture: {e}"))?;

    let mut sram = vec![0; 8192 * 4];
    let mbc = Mbc1::new(&rom, &mut sram);
    let mut emu = Emu::new(boot_data, mbc, Input::new(event_pump));
    emu.reset();
    if args.boot.is_none() {
        // skip boot rom
        let (cpu, mut cpu_view) = emu.cpu_view();
        cpu.set_wide_register(WideRegister::PC, 0x100);
        cpu_view.write(Port::BOOT, 0x01);
        cpu_view.write(Port::LCDC, 0x81);
    }

    let debug_mode = Arc::new(AtomicBool::new(args.debug));
    signal_hook::flag::register(signal_hook::consts::SIGUSR1, debug_mode.clone())
        .map_err(|e| {
            tracing::warn!("external debugger unavailable: failed to install SIGUSR1 handler: {e}")
        })
        .ok();
    let mut breakpoints = Vec::new();

    let mut rl = Editor::with_config(Config::builder().auto_add_history(true).build())
        .map_err(|e| format!("failed to initialize line editor: {e}"))?;
    rl.set_helper(Some(LineHelper {
        hinter: HistoryHinter::new(),
        completer: LineCompleter::new(),
    }));
    // TODO: add all ports and symbols
    rl.helper_mut().unwrap().completer.add("SCX");
    let mut start = Instant::now();
    let mut frames = 0;
    let mut cycles = 0;
    'da_loop: loop {
        if breakpoints.contains(&emu.cpu().wide_register(WideRegister::PC)) {
            debug_mode.store(true, Ordering::Relaxed);
        }
        if debug_mode.load(Ordering::Relaxed) {
            loop {
                #[rustfmt::skip]
                println!(
                    "PC={:04X} AF={:04X} BC={:04X} DE={:04X} HL={:04X} SP={:04X} [{}{}{}{}]",
                    emu.cpu().wide_register(WideRegister::PC),
                    emu.cpu().wide_register(WideRegister::AF),
                    emu.cpu().wide_register(WideRegister::BC),
                    emu.cpu().wide_register(WideRegister::DE),
                    emu.cpu().wide_register(WideRegister::HL),
                    emu.cpu().wide_register(WideRegister::SP),
                    if emu.cpu().flag(Flag::Zero) { 'Z' } else { '-' },
                    if emu.cpu().flag(Flag::Negative) { 'N' } else { '-' },
                    if emu.cpu().flag(Flag::HalfCarry) { 'H' } else { '-' },
                    if emu.cpu().flag(Flag::Carry) { 'C' } else { '-' },
                );
                match rl.readline("> ") {
                    Ok(line) => {
                        let line = if line.is_empty() {
                            if let Some(line) = rl.history().iter().last() {
                                line
                            } else {
                                continue;
                            }
                        } else {
                            &line
                        };
                        let parts = line
                            .split_whitespace()
                            .map(String::from)
                            .collect::<Vec<String>>();
                        match parts[0].as_str() {
                            "s" => {
                                emu.tick();
                            }
                            "b" => {
                                if parts.len() > 1 {
                                    if let Ok(addr) = u16::from_str_radix(&parts[1], 16) {
                                        breakpoints.push(addr);
                                        continue;
                                    }
                                }
                                println!("?");
                            }
                            "d" => {
                                if parts.len() > 1 {
                                    if let Ok(n) = usize::from_str_radix(&parts[1], 10) {
                                        if n < breakpoints.len() {
                                            breakpoints.remove(n);
                                            continue;
                                        }
                                    }
                                }
                                println!("?");
                            }
                            "c" => {
                                debug_mode.store(false, Ordering::Relaxed);
                                break;
                            }
                            "x" => {
                                if parts.len() > 1 {
                                    if let Ok(addr) = u16::from_str_radix(&parts[1], 16) {
                                        let (_, mut cpu_view) = emu.cpu_view();
                                        let value = cpu_view.read(addr);
                                        println!("{value:02X}");
                                        continue;
                                    }
                                }
                                println!("?");
                            }
                            "p" => {
                                if parts.len() > 2 {
                                    if let Ok(addr) = u16::from_str_radix(&parts[1], 16) {
                                        if let Ok(value) = u8::from_str_radix(&parts[2], 16) {
                                            let (_, mut cpu_view) = emu.cpu_view();
                                            cpu_view.write(addr, value);
                                            continue;
                                        }
                                    }
                                }
                                println!("?");
                            }
                            "i" => {
                                if parts.len() > 1 {
                                    match parts[1].as_str() {
                                        "b" => {
                                            for (i, breakpoint) in breakpoints.iter().enumerate() {
                                                println!("{i:03}: {breakpoint:04X}");
                                            }
                                        }
                                        _ => println!("?"),
                                    }
                                    continue;
                                }
                                println!("?");
                            }
                            "q" => {
                                break 'da_loop;
                            }
                            _ => println!("?"),
                        }
                    }
                    Err(ReadlineError::Eof) => {
                        break 'da_loop;
                    }
                    Err(ReadlineError::Io(e)) => {
                        return Err(format!("could not read line: {e}"));
                    }
                    Err(ReadlineError::Errno(e)) => {
                        return Err(format!("could not read line: {}", e.desc()));
                    }
                    Err(_) => {}
                }
            }
        }
        let now = Instant::now();
        cycles += emu.tick();
        if emu.vblanked() {
            let rect = Rect::new(0, 0, 160, 144);
            texture
                .update(
                    rect,
                    // bytemuck unfortunately doesnt like casting *BIG* 2D arrays
                    unsafe {
                        slice::from_raw_parts(
                            emu.lcd().as_ptr() as *const u8,
                            160 * 144 * mem::size_of::<u32>(),
                        )
                    },
                    160 * mem::size_of::<u32>(),
                )
                .map_err(|e| format!("failed to lock texture: {e}"))?;
            canvas
                .copy(&texture, rect, None)
                .map_err(|e| format!("failed to copy texture: {e}"))?;
            canvas.present();
            frames += 1;
        }
        if emu.input_mut().debug() {
            debug_mode.store(true, Ordering::Relaxed);
        }
        if emu.input_mut().escape() {
            break 'da_loop;
        }
        if now.duration_since(start) > Duration::from_secs(1) {
            let mhz = (cycles as f64) / 1_000_000.0;
            canvas
                .window_mut()
                .set_title(&format!("gb23 :: {mhz:.03} MHz :: {frames} fps"))
                .map_err(|e| format!("failed to update window title: {e}"))?;
            start = now;
            frames = 0;
            cycles = 0;
        }
    }
    Ok(())
}

struct Input {
    event_pump: EventPump,
    p1: u8,
    counter: usize,
    debug: bool,
    escape: bool,
}

impl Input {
    fn new(event_pump: EventPump) -> Self {
        Self {
            event_pump,
            p1: 0x3F,
            counter: 0,
            debug: false,
            escape: false,
        }
    }

    pub fn debug(&mut self) -> bool {
        if self.debug {
            self.debug = false;
            return true;
        }
        false
    }

    pub fn escape(&self) -> bool {
        self.escape
    }
}

impl<B: Bus> BusDevice<B> for Input {
    fn reset(&mut self, _bus: &mut B) {
        self.p1 = 0x3F;
        self.counter = 0;
    }

    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            Port::P1 => self.p1,
            _ => unreachable!(),
        }
    }

    fn write(&mut self, addr: u16, value: u8) {
        match addr {
            Port::P1 => {
                if (value & 0x30) == 0x20 {
                    let keyboard = self.event_pump.keyboard_state();
                    self.p1 |= 0x0F;
                    if keyboard.is_scancode_pressed(Scancode::Down) {
                        self.p1 &= 0x27;
                    }
                    if keyboard.is_scancode_pressed(Scancode::Up) {
                        self.p1 &= 0x2B;
                    }
                    if keyboard.is_scancode_pressed(Scancode::Left) {
                        self.p1 &= 0x2D;
                    }
                    if keyboard.is_scancode_pressed(Scancode::Right) {
                        self.p1 &= 0x2E;
                    }
                    return;
                }
                if (value & 0x30) == 0x10 {
                    let keyboard = self.event_pump.keyboard_state();
                    self.p1 |= 0x0F;
                    if keyboard.is_scancode_pressed(Scancode::Return) {
                        self.p1 &= 0x17;
                    }
                    if keyboard.is_scancode_pressed(Scancode::RShift) {
                        self.p1 &= 0x1B;
                    }
                    if keyboard.is_scancode_pressed(Scancode::Z) {
                        self.p1 &= 0x1D;
                    }
                    if keyboard.is_scancode_pressed(Scancode::X) {
                        self.p1 &= 0x1E;
                    }
                    return;
                }
                self.p1 |= 0x3F;
            }
            _ => unreachable!(),
        }
    }

    fn tick(&mut self, _bus: &mut B) -> usize {
        self.counter += 1;
        // we read the keyboard around every frame
        if self.counter > (4194304 / 60) {
            self.counter = 0;
            self.event_pump.pump_events();
            let keyboard = self.event_pump.keyboard_state();
            if keyboard.is_scancode_pressed(Scancode::F1) {
                self.debug = true;
            }
            if keyboard.is_scancode_pressed(Scancode::Escape) {
                self.escape = true;
            }
        }
        0
    }
}
