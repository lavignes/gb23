use std::io;

use self::{
    bus::{Bus, BusDevice},
    cpu::Cpu,
    mbc::null::Null,
};

mod apu;
mod bus;
mod cpu;
mod mbc;
mod ppu;

pub struct Emu {
    bios: Vec<u8>,

    cpu: Cpu,
    mbc: Box<dyn BusDevice<MbcView>>,
    wram: [[u8; 4096]; 8],
    hram: [u8; 256],
    wram_hi_bank: u8,
    ie: u8,
}

impl Emu {
    pub fn new(bios: Vec<u8>, rom: Vec<u8>) -> io::Result<Self> {
        let cpu = Cpu::new();
        // TODO: parse rom to determine MBC
        let mbc = Box::new(Null::new(rom, Vec::new()));
        Ok(Self {
            bios,
            cpu,
            mbc,
            wram: [[0xFF; 4096]; 8],
            hram: [0xFF; 256],
            wram_hi_bank: 0,
            ie: 0,
        })
    }

    pub fn reset(&mut self) {
        let Self {
            ref bios,
            ref mut cpu,
            ref mut mbc,
            ref mut wram,
            ref mut hram,
            ref mut wram_hi_bank,
            ref mut ie,
        } = self;
        let mut cpu_view = CpuView {
            bios,
            mbc: mbc.as_mut(),
            wram,
            hram,
            wram_hi_bank,
            ie,
        };
        cpu.reset(&mut cpu_view);
        mbc.reset(&mut MbcView {});
    }

    pub fn tick(&mut self) {
        let Self {
            ref bios,
            ref mut cpu,
            ref mut mbc,
            ref mut wram,
            ref mut hram,
            ref mut wram_hi_bank,
            ref mut ie,
        } = self;
        let mut cpu_view = CpuView {
            bios,
            mbc: mbc.as_mut(),
            wram,
            hram,
            wram_hi_bank,
            ie,
        };
        cpu.tick(&mut cpu_view);
        mbc.tick(&mut MbcView {});
    }
}

struct CpuView<'a> {
    bios: &'a [u8],

    mbc: &'a mut dyn BusDevice<MbcView>,
    wram: &'a mut [[u8; 4096]; 8],
    hram: &'a mut [u8; 256],
    wram_hi_bank: &'a mut u8,
    ie: &'a mut u8,
}

impl<'a> Bus for CpuView<'a> {
    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x7FFF => self.mbc.read(addr),
            // 0x8000..=0x9FFF => self.ppu.read(addr), VRAM
            0xA000..=0xBFFF => self.mbc.read(addr),
            0xC000..=0xCFFF => self.wram[0][(addr - 0xC000) as usize],
            0xD000..=0xDFFF if *self.wram_hi_bank < 2 => self.wram[0][(addr - 0xD000) as usize],
            0xD000..=0xDFFF => self.wram[*self.wram_hi_bank as usize][(addr - 0xD000) as usize],

            // shadow area
            0xE000..=0xEFFF => self.wram[0][(addr - 0xE000) as usize],
            0xF000..=0xFDFF if *self.wram_hi_bank < 2 => self.wram[0][(addr - 0xF000) as usize],
            0xF000..=0xFDFF => self.wram[0][(addr - 0xF000) as usize],

            // 0xFE00..=0xFE9F => self.ppu.read(addr), OAM
            0xFEA0..=0xFEFF => 0xFF,
            // 0xFF00..=0xFF7F => IO registers
            0xFF80..=0xFFFE => self.hram[(addr - 0xFF80) as usize],
            0xFFFF => *self.ie,

            _ => todo!(),
        }
    }

    fn write(&mut self, _addr: u16, _value: u8) {}
}

struct MbcView {}

impl Bus for MbcView {
    fn read(&mut self, _addr: u16) -> u8 {
        0xFF
    }

    fn write(&mut self, _addr: u16, _value: u8) {}
}
