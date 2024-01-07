use self::{
    bus::{Bus, BusDevice, Port},
    cpu::Cpu,
    ppu::Ppu,
};

mod apu;
mod bus;
pub mod cpu;
pub mod mbc;
mod ppu;

pub struct Emu<M, P> {
    bios_data: Vec<u8>,
    vblanked: bool,
    cpu: Cpu,
    mbc: M,
    ppu: P,
    lcd: [[u32; 160]; 144],
    wram: [[u8; 4096]; 8],
    hram: [u8; 256],
    iflags: u8,
    bios: u8,
    svbk: u8,
    p1: u8,
    sc: u8,
    div: u8,
    tima: u8,
    tma: u8,
    tac: u8,
    ie: u8,

    div_counter: usize,
    tima_counter: usize,
}

impl<M: BusDevice<MbcView>> Emu<M, Ppu> {
    pub fn new(bios_data: Vec<u8>, mbc: M) -> Self {
        let cpu = Cpu::new();
        let ppu = Ppu::new();
        let lcd = [[0; 160]; 144];
        Self {
            bios_data,
            vblanked: false,
            cpu,
            mbc,
            ppu,
            lcd,
            wram: [[0xFF; 4096]; 8],
            hram: [0xFF; 256],
            iflags: 0,
            bios: 0,
            svbk: 0,
            p1: 0,
            sc: 0,
            div: 0,
            tima: 0,
            tma: 0,
            tac: 0,
            ie: 0,
            div_counter: 0,
            tima_counter: 0,
        }
    }

    pub fn reset(&mut self) {
        let Self {
            ref bios_data,
            ref mut cpu,
            ref mut mbc,
            ref mut ppu,
            ref mut lcd,
            ref mut wram,
            ref mut hram,
            ref mut iflags,
            ref mut bios,
            ref mut svbk,
            ref mut p1,
            ref mut sc,
            ref mut div,
            ref mut tima,
            ref mut tma,
            ref mut tac,
            ref mut ie,
            ..
        } = self;
        let mut cpu_view = CpuView {
            bios_data,
            mbc,
            ppu,
            wram,
            hram,
            iflags,
            bios,
            svbk,
            p1,
            sc,
            div,
            tima,
            tma,
            tac,
            ie,
        };
        cpu.reset(&mut cpu_view);
        mbc.reset(&mut MbcView {});
        ppu.reset(&mut PpuView { lcd, iflags });
        self.vblanked = false;
        self.iflags = 0;
        self.svbk = 0;
        self.p1 = 0;
        self.sc = 0;
        self.div = 0;
        self.tima = 0;
        self.tma = 0;
        self.tac = 0;
        self.ie = 0;
    }

    pub fn tick(&mut self) -> usize {
        let Self {
            ref bios_data,
            ref mut cpu,
            ref mut mbc,
            ref mut ppu,
            ref mut lcd,
            ref mut wram,
            ref mut hram,
            ref mut iflags,
            ref mut bios,
            ref mut svbk,
            ref mut p1,
            ref mut sc,
            ref mut div,
            ref mut tima,
            ref mut tma,
            ref mut tac,
            ref mut ie,
            ..
        } = self;
        let mut cpu_view = CpuView {
            bios_data,
            mbc,
            ppu,
            wram,
            hram,
            iflags,
            bios,
            svbk,
            p1,
            sc,
            div,
            tima,
            tma,
            tac,
            ie,
        };
        let cycles = cpu.tick(&mut cpu_view);
        // TODO: mbc tick?
        let mut ppu_view = PpuView { lcd, iflags };
        let mut vblank = 0;
        for _ in 0..cycles {
            vblank += ppu.tick(&mut ppu_view);
        }
        if vblank != 0 {
            self.vblanked = true;
        }
        // timers
        self.div_counter += cycles;
        // TODO: verify this value needs to be 1024 vs 256
        if self.div_counter >= 1024 {
            self.div_counter -= 1024;
            self.div = self.div.wrapping_add(1);
        }
        if (self.tac & 0x04) != 0 {
            self.tima_counter += cycles;
            let freq = match self.tac & 0x03 {
                0x00 => 4096,
                0x01 => 262144,
                0x02 => 65536,
                0x03 => 16384,
                _ => unreachable!(),
            };
            let period = 4194304 / freq;
            while self.tima_counter >= period {
                let (result, carry) = self.tima.overflowing_add(1);
                // timer interrupt
                if carry {
                    self.iflags |= 0x04;
                    self.tima = self.tma;
                } else {
                    self.tima = result;
                }
                self.tima_counter = self.tima_counter.wrapping_sub(period);
            }
        }
        cycles
    }

    pub fn vblanked(&mut self) -> bool {
        let value = self.vblanked;
        self.vblanked = false;
        value
    }

    pub fn lcd(&self) -> &[[u32; 160]; 144] {
        &self.lcd
    }

    pub fn cpu(&self) -> &Cpu {
        &self.cpu
    }

    pub fn cpu_read(&mut self, addr: u16) -> u8 {
        let Self {
            ref bios_data,
            ref mut mbc,
            ref mut ppu,
            ref mut wram,
            ref mut hram,
            ref mut iflags,
            ref mut bios,
            ref mut svbk,
            ref mut p1,
            ref mut ie,
            ref mut sc,
            ref mut div,
            ref mut tima,
            ref mut tma,
            ref mut tac,
            ..
        } = self;
        let mut cpu_view = CpuView {
            bios_data,
            mbc,
            ppu,
            wram,
            hram,
            iflags,
            bios,
            svbk,
            p1,
            sc,
            div,
            tima,
            tma,
            tac,
            ie,
        };
        cpu_view.read(addr)
    }
}

struct CpuView<'a, M, P> {
    bios_data: &'a [u8],

    mbc: &'a mut M,
    ppu: &'a mut P,
    wram: &'a mut [[u8; 4096]; 8],
    hram: &'a mut [u8; 256],
    iflags: &'a mut u8,
    bios: &'a mut u8,
    svbk: &'a mut u8,
    p1: &'a mut u8,
    sc: &'a mut u8,
    div: &'a mut u8,
    tima: &'a mut u8,
    tma: &'a mut u8,
    tac: &'a mut u8,
    ie: &'a mut u8,
}

impl<'a, M: BusDevice<MbcView>> Bus for CpuView<'a, M, Ppu> {
    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            // BIOS
            0x0000..=0x00FF if *self.bios == 0 => self.bios_data[addr as usize],
            // cart
            0x0000..=0x7FFF => self.mbc.read(addr),
            // VRAM
            0x8000..=0x9FFF => <Ppu as BusDevice<PpuView>>::read(self.ppu, addr),
            // cart
            0xA000..=0xBFFF => self.mbc.read(addr),
            // WRAM
            0xC000..=0xCFFF => self.wram[0][(addr - 0xC000) as usize],
            0xD000..=0xDFFF if *self.svbk < 2 => self.wram[1][(addr - 0xD000) as usize],
            0xD000..=0xDFFF => self.wram[*self.svbk as usize][(addr - 0xD000) as usize],
            // shadow area
            0xE000..=0xEFFF => self.wram[0][(addr - 0xE000) as usize],
            0xF000..=0xFDFF if *self.svbk < 2 => self.wram[1][(addr - 0xF000) as usize],
            0xF000..=0xFDFF => self.wram[*self.svbk as usize][(addr - 0xF000) as usize],
            // OAM
            0xFE00..=0xFE9F => <Ppu as BusDevice<PpuView>>::read(self.ppu, addr),
            // reserved
            0xFEA0..=0xFEFF => 0xFF,
            Port::P1 => *self.p1,
            Port::SB => todo!(),
            Port::SC => *self.sc,
            0xFF03 => 0xFF,
            Port::DIV => *self.div,
            Port::TIMA => *self.tima,
            Port::TMA => *self.tma,
            Port::TAC => *self.tac,
            Port::IF => *self.iflags,
            // PPU IO ports
            Port::LCDC..=Port::WX => <Ppu as BusDevice<PpuView>>::read(self.ppu, addr),
            // 0xFF4D => // KEY1 (cpu speed switch)
            Port::BIOS => *self.bios,
            // more PPU IO ports
            Port::VBK | Port::HMDA1..=Port::HMDA5 => {
                <Ppu as BusDevice<PpuView>>::read(self.ppu, addr)
            }
            // 0xFF56 => // IR port
            // even more PPU IO ports
            Port::BCPS..=Port::OCPD => <Ppu as BusDevice<PpuView>>::read(self.ppu, addr),
            Port::SVBK => *self.svbk,
            // HRAM
            0xFF80..=0xFFFE => self.hram[(addr - 0xFF80) as usize],
            Port::IE => *self.ie,
            _ => 0xFF, // TODO
        }
    }

    fn write(&mut self, addr: u16, value: u8) {
        match addr {
            // cart
            0x0000..=0x7FFF => self.mbc.write(addr, value),
            // VRAM
            0x8000..=0x9FFF => <Ppu as BusDevice<PpuView>>::write(self.ppu, addr, value),
            // cart
            0xA000..=0xBFFF => self.mbc.write(addr, value),
            // WRAM
            0xC000..=0xCFFF => self.wram[0][(addr - 0xC000) as usize] = value,
            0xD000..=0xDFFF if *self.svbk < 2 => self.wram[1][(addr - 0xD000) as usize] = value,
            0xD000..=0xDFFF => self.wram[*self.svbk as usize][(addr - 0xD000) as usize] = value,
            // shadow area
            0xE000..=0xEFFF => self.wram[0][(addr - 0xE000) as usize] = value,
            0xF000..=0xFDFF if *self.svbk < 2 => self.wram[1][(addr - 0xF000) as usize] = value,
            0xF000..=0xFDFF => self.wram[*self.svbk as usize][(addr - 0xF000) as usize] = value,
            // OAM
            0xFE00..=0xFE9F => <Ppu as BusDevice<PpuView>>::write(self.ppu, addr, value),
            // reserved
            0xFEA0..=0xFEFF => {}
            Port::P1 => *self.p1 = value & 0x3F,
            Port::SB => eprint!("{}", value as char),
            Port::SC => *self.sc = value & 0x03,
            0xFF03 => {}
            Port::DIV => *self.div = 0,
            Port::TIMA => *self.tima = value,
            Port::TMA => *self.tma = value,
            Port::TAC => *self.tac = value & 0x07,
            Port::IF => *self.iflags = value & 0x1F,
            // PPU IO ports
            Port::LCDC..=Port::WX => <Ppu as BusDevice<PpuView>>::write(self.ppu, addr, value),
            // 0xFF4D => // KEY1 (cpu speed switch)
            Port::BIOS => *self.bios = value,
            // more PPU IO ports
            Port::VBK | Port::HMDA1..=Port::HMDA5 => {
                <Ppu as BusDevice<PpuView>>::write(self.ppu, addr, value)
            }
            // 0xFF56 => // IR port
            // even more PPU IO ports
            Port::BCPS..=Port::OCPD => <Ppu as BusDevice<PpuView>>::write(self.ppu, addr, value),
            Port::SVBK => *self.svbk = value & 0x07,
            // HRAM
            0xFF80..=0xFFFE => self.hram[(addr - 0xFF80) as usize] = value,
            Port::IE => *self.ie = value,
            _ => {} // TODO
        }
    }
}

pub struct MbcView {}

impl Bus for MbcView {}

pub struct PpuView<'a> {
    lcd: &'a mut [[u32; 160]; 144],
    iflags: &'a mut u8,
    // TODO: DMA access for PPU
}

impl<'a> Bus for PpuView<'a> {
    fn lcd_mut(&mut self) -> &mut [[u32; 160]; 144] {
        self.lcd
    }

    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            Port::IF => *self.iflags,
            _ => unreachable!(),
        }
    }

    fn write(&mut self, addr: u16, value: u8) {
        match addr {
            Port::IF => *self.iflags = value,
            _ => unreachable!(),
        }
    }
}
