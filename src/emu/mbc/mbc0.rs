use crate::emu::bus::{Bus, BusDevice};

pub struct Mbc0 {
    rom: Vec<u8>,
    sram: Vec<u8>,
}

impl Mbc0 {
    pub fn new(rom: Vec<u8>, sram: Vec<u8>) -> Self {
        Self { rom, sram }
    }
}

impl<B: Bus> BusDevice<B> for Mbc0 {
    fn reset(&mut self, _bus: &mut B) {}

    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x7FFF => self.rom[addr as usize],
            //0xA000..=0xBFFF => self.sram[(addr - 0xA000) as usize],
            _ => 0xFF,
        }
    }

    fn write(&mut self, addr: u16, value: u8) {
        match addr {
            //0xA000..=0xBFFF => self.sram[(addr - 0xA000) as usize] = value,
            _ => {}
        }
    }

    fn tick(&mut self, _bus: &mut B) -> usize {
        0
    }
}
