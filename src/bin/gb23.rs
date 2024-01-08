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
    mbc::{mbc0::Mbc0, mbc1::Mbc1},
    Emu,
};
use rustyline::{error::ReadlineError, Config, DefaultEditor};
use sdl2::{keyboard::Scancode, pixels::PixelFormatEnum, rect::Rect, EventPump};
use tracing::Level;

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Args {
    /// Path to ROM file
    rom: PathBuf,

    /// Path to BIOS file
    #[arg(short, long)]
    bios: Option<PathBuf>,

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

fn main_real(args: Args) -> Result<(), String> {
    let mut rom = Vec::new();
    File::open(&args.rom)
        .map_err(|e| format!("failed to open ROM file: {e}"))?
        .read_to_end(&mut rom)
        .map_err(|e| format!("failed to read ROM file: {e}"))?;
    let mut bios_data = Vec::new();
    if let Some(bios) = args.bios {
        File::open(&bios)
            .map_err(|e| format!("failed to open BIOS file: {e}"))?
            .read_to_end(&mut bios_data)
            .map_err(|e| format!("failed to read BIOS file: {e}"))?;
    }
    let sdl = sdl2::init().map_err(|e| format!("failed to initialize SDL2: {e}"))?;
    let event_pump = sdl
        .event_pump()
        .map_err(|e| format!("failed to initialize SDL2 events: {e}"))?;
    let video = sdl
        .video()
        .map_err(|e| format!("failed to initialize SDL2 video: {e}"))?;
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
    let mut emu = Emu::new(bios_data, mbc, Input::new(event_pump));
    emu.reset();

    let debug_mode = Arc::new(AtomicBool::new(args.debug));
    signal_hook::flag::register(signal_hook::consts::SIGUSR1, debug_mode.clone())
        .map_err(|e| {
            tracing::warn!("external debugger unavailable: failed to install SIGUSR1 handler: {e}")
        })
        .ok();
    let mut breakpoints = Vec::new();

    let mut start = Instant::now();
    let mut frames = 0;
    let mut cycles = 0;
    'da_loop: loop {
        if breakpoints.contains(&emu.cpu().wide_register(WideRegister::PC)) {
            debug_mode.store(true, Ordering::Relaxed);
        }
        if debug_mode.load(Ordering::Relaxed) {
            let mut rl =
                DefaultEditor::with_config(Config::builder().auto_add_history(true).build())
                    .map_err(|e| format!("failed to initialize line editor: {e}"))?;
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
                                        let value = emu.cpu_read(addr);
                                        println!("{value:02X}");
                                        continue;
                                    }
                                }
                                println!("?");
                            }
                            "q" => {
                                break 'da_loop;
                            }
                            "info" => {
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
}

impl Input {
    fn new(event_pump: EventPump) -> Self {
        Self {
            event_pump,
            p1: 0x3F,
            counter: 0,
            debug: false,
        }
    }

    pub fn debug(&mut self) -> bool {
        if self.debug {
            self.debug = false;
            return true;
        }
        false
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
        }
        0
    }
}
