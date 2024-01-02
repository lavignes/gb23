use crate::emu::bus::{Bus, BusDevice};

pub struct Null {
    rom: Vec<u8>,
    ram: Vec<u8>,
}

impl Null {
    pub fn new(rom: Vec<u8>, ram: Vec<u8>) -> Self {
        Self { rom, ram }
    }
}

impl<B: Bus> BusDevice<B> for Null {
    fn reset(&mut self, _bus: &mut B) {}

    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x7FFF => self.rom[addr as usize],
            0xA000..=0xBFFF => self.ram[(addr - 0xA000) as usize],
            _ => 0xFF,
        }
    }

    fn write(&mut self, addr: u16, value: u8) {
        match addr {
            0xA000..=0xBFFF => self.ram[(addr - 0xA000) as usize] = value,
            _ => {}
        }
    }

    fn tick(&mut self, _bus: &mut B) -> usize {
        0
    }
}
