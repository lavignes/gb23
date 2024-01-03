use std::{
    error::Error,
    fs::File,
    io::{self, Read},
    path::PathBuf,
    process::ExitCode,
};

use clap::Parser;
use tracing::Level;

use gb23::emu::{mbc::null::Null, Emu};

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

    /// Start with GDB server at address
    #[arg(short, long, value_name = "ADDRESS:PORT")]
    debug: Option<String>,
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
    let emu = Emu::new(bios_data, mbc);

    Ok::<(), String>(())
}
