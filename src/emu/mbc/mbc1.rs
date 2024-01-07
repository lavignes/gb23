use crate::emu::bus::{Bus, BusDevice};

pub struct Mbc1<'a> {
    rom: Vec<&'a [u8]>,
    sram: Vec<&'a mut [u8]>,
    rom_bank: u8,
    sram_bank: u8,
    bank_mode: u8,
}

impl<'a> Mbc1<'a> {
    pub fn new(rom: &'a [u8], sram: &'a mut [u8]) -> Self {
        Self {
            rom: rom.chunks(16384).collect(),
            sram: sram.chunks_mut(8192).collect(),
            rom_bank: 0,
            sram_bank: 0,
            bank_mode: 0,
        }
    }
}

impl<'a, B: Bus> BusDevice<B> for Mbc1<'a> {
    fn reset(&mut self, _bus: &mut B) {
        self.rom_bank = 0;
        self.sram_bank = 0;
        self.bank_mode = 0;
    }

    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x3FFF => self.rom[0][addr as usize],
            0x4000..=0x7FFF => self.rom[self.rom_bank as usize][(addr - 0x4000) as usize],
            0xA000..=0xBFFF => self.sram[self.sram_bank as usize][(addr - 0xA000) as usize],
            _ => 0xFF,
        }
    }

    fn write(&mut self, addr: u16, value: u8) {
        match addr {
            0x2000..=0x3FFF => {
                let lo = value & 0x15;
                // quirk to translate bank 0 (and some others) one bank up
                let lo = match lo {
                    0x00 => 0x01,
                    0x20 => 0x21,
                    0x40 => 0x41,
                    0x60 => 0x61,
                    _ => lo,
                };
                self.rom_bank = (self.rom_bank & 0xE0) | lo;
                // make sure bank wraps around actual rom size
                self.rom_bank &= (self.rom.len() - 1) as u8;
            }
            0x4000..=0x5FFF => {
                if self.bank_mode == 0 {
                    let hi = (value & 0x03) << 5;
                    self.rom_bank = (self.rom_bank & 0x1F) | hi;
                    // make sure bank wraps around actual rom size
                    self.rom_bank &= (self.rom.len() - 1) as u8;
                } else {
                    self.sram_bank = value & 0x03;
                    // make sure bank wraps around actual ram size
                    self.sram_bank &= (self.sram.len() - 1) as u8;
                }
            }
            0x6000..=0x7FFF => self.bank_mode = value & 0x01,
            0xA000..=0xBFFF => self.sram[self.sram_bank as usize][(addr - 0xA000) as usize] = value,
            _ => {}
        }
    }

    fn tick(&mut self, _bus: &mut B) -> usize {
        0
    }
}
