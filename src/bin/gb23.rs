use core::slice;
use std::{
    error::Error,
    fs::File,
    io::{self, Read},
    mem,
    path::PathBuf,
    process::ExitCode,
    time::{Duration, Instant},
};

use clap::Parser;
use gb23::emu::{mbc::null::Null, Emu};
use sdl2::{pixels::PixelFormatEnum, rect::Rect};
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
        tracing::error!("{}", e.into());
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn main_real(args: Args) -> Result<(), impl Into<Box<dyn Error>>> {
    let mut rom_data = Vec::new();
    File::open(&args.rom)
        .map_err(|e| format!("failed to open ROM file: {e}"))?
        .read_to_end(&mut rom_data)
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
    let mbc = Null::new(rom_data, Vec::new());
    let mut emu = Emu::new(bios_data, mbc);
    emu.reset();
    let window = video
        .window("gb23", 160 * 4, 144 * 4)
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
        .create_texture_streaming(PixelFormatEnum::RGBA32, 256, 256)
        .map_err(|e| format!("failed to create texture: {e}"))?;
    let mut start = Instant::now();
    let mut frames = 0;
    let mut cycles = 0;
    loop {
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
    Ok::<(), String>(())
}
