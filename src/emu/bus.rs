pub enum Port {}

impl Port {
    pub const P1: u16 = 0xFF00;
    pub const SB: u16 = 0xFF01;
    pub const SC: u16 = 0xFF02;

    pub const DIV: u16 = 0xFF04;
    pub const TIMA: u16 = 0xFF05;
    pub const TMA: u16 = 0xFF06;
    pub const TAC: u16 = 0xFF07;

    pub const IF: u16 = 0xFF0F;

    pub const NR10: u16 = 0xFF10;
    pub const NR11: u16 = 0xFF11;
    pub const NR12: u16 = 0xFF12;
    pub const NR13: u16 = 0xFF13;
    pub const NR14: u16 = 0xFF14;

    pub const NR21: u16 = 0xFF16;
    pub const NR22: u16 = 0xFF17;
    pub const NR23: u16 = 0xFF18;
    pub const NR24: u16 = 0xFF19;

    pub const LCDC: u16 = 0xFF40;
    pub const STAT: u16 = 0xFF41;
    pub const SCY: u16 = 0xFF42;
    pub const SCX: u16 = 0xFF43;
    pub const LY: u16 = 0xFF44;
    pub const LYC: u16 = 0xFF45;
    pub const DMA: u16 = 0xFF46;
    pub const BGP: u16 = 0xFF47;
    pub const OBP0: u16 = 0xFF48;
    pub const OBP1: u16 = 0xFF49;
    pub const WY: u16 = 0xFF4A;
    pub const WX: u16 = 0xFF4B;

    pub const KEY1: u16 = 0xFF4D;
    pub const VBK: u16 = 0xFF4F;
    pub const BIOS: u16 = 0xFF50;

    pub const HMDA1: u16 = 0xFF51;
    pub const HMDA2: u16 = 0xFF52;
    pub const HMDA3: u16 = 0xFF53;
    pub const HMDA4: u16 = 0xFF54;
    pub const HMDA5: u16 = 0xFF55;

    pub const BCPS: u16 = 0xFF68;
    pub const BCPD: u16 = 0xFF69;
    pub const OCPS: u16 = 0xFF6A;
    pub const OCPD: u16 = 0xFF6B;
    pub const SVBK: u16 = 0xFF70;

    pub const IE: u16 = 0xFFFF;
}

pub trait Bus {
    fn lcd_mut(&mut self) -> &mut [[u32; 160]; 144] {
        unreachable!()
    }

    fn read(&mut self, _addr: u16) -> u8 {
        unreachable!()
    }

    fn write(&mut self, _addr: u16, _value: u8) {
        unreachable!()
    }
}

pub trait BusDevice<B: Bus> {
    fn reset(&mut self, bus: &mut B);

    fn read(&mut self, _addr: u16) -> u8 {
        unreachable!()
    }

    fn write(&mut self, _addr: u16, _value: u8) {
        unreachable!()
    }

    fn tick(&mut self, bus: &mut B) -> usize;
}
