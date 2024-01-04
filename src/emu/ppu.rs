use super::bus::{Bus, BusDevice, Port};

pub struct Ppu {
    chr_data: [[u8; 6144]; 2],
    bg_data1: [[u8; 1024]; 2],
    bg_data2: [[u8; 1024]; 2],
    obj: [u8; 40 * 4],
    dot: usize,

    lcdc: u8,
    stat: u8,
    scy: u8,
    scx: u8,
    ly: u8,
    lyc: u8,
    dma: u8,
    bgp: u8,
    obp0: u8,
    obp1: u8,
    wy: u8,
    wx: u8,
    vbk: u8,
    hdma1: u8,
    hdma2: u8,
    hdma3: u8,
    hdma4: u8,
    hdma5: u8,
    bcps: u8,
    bcpd: u8,
    ocps: u8,
    ocpd: u8,
}

impl Ppu {
    pub fn new() -> Self {
        Self {
            chr_data: [[0xFF; 6144]; 2],
            bg_data1: [[0xFF; 1024]; 2],
            bg_data2: [[0xFF; 1024]; 2],
            obj: [0xFF; 40 * 4],
            dot: 0,
            lcdc: 0,
            stat: 0,
            scy: 0,
            scx: 0,
            ly: 0,
            lyc: 0,
            dma: 0,
            bgp: 0,
            obp0: 0,
            obp1: 0,
            wy: 0,
            wx: 0,
            vbk: 0,
            hdma1: 0,
            hdma2: 0,
            hdma3: 0,
            hdma4: 0,
            hdma5: 0,
            bcps: 0,
            bcpd: 0,
            ocps: 0,
            ocpd: 0,
        }
    }

    fn draw_line(&mut self, line: &mut [u32; 160]) {
        // line.fill(u32::from_le_bytes([self.ly, self.ly, self.ly, self.ly]));
        let bg_data = if (self.lcdc & 0x08) == 0 {
            &self.bg_data1
        } else {
            &self.bg_data2
        };
        let screen_x = self.scx as usize;
        let screen_y = (self.ly as usize) + (self.scy as usize);
        // offset into the 8 2bpp bytes on the current line (assuming no flip)
        let chr_y = 2 * (screen_y % 8);
        // find leftmost bg tile on screen
        let mut bg_tile_idx = (screen_x / 8) + (screen_y / 8) * 32;
        let mut chr_x = screen_x % 8;
        let mut dot = 0;
        // only 20 tiles are visible per line
        for _ in 0..20 {
            let chr_idx = bg_data[0][bg_tile_idx];
            let attr = bg_data[1][bg_tile_idx];
            let chr_data_offset = if (self.lcdc & 0x10) == 0 {
                chr_idx as usize * 16
            } else {
                0x1000usize.wrapping_add_signed(chr_idx as i8 as isize * 16)
            };
            let lo = self.chr_data[0][chr_data_offset + chr_y];
            let hi = self.chr_data[0][chr_data_offset + chr_y + 1];
            for i in 0..8 {
                let color = (((hi >> i) << 1) | (lo >> i)) & 0b11;
                line[dot + i] = match color {
                    0 => 0xFFFFFFFF,
                    1 => 0xAAAAAAFF,
                    2 => 0x555555FF,
                    3 => 0x000000FF,
                    _ => unreachable!(),
                };
            }
            bg_tile_idx += 1;
            chr_x += 8;
            dot += 8;
        }
    }
}

impl<B: Bus> BusDevice<B> for Ppu {
    fn reset(&mut self, _bus: &mut B) {
        self.dot = 0;
        self.vbk = 0;
    }

    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            0x8000..=0x97FF => self.chr_data[self.vbk as usize][(addr - 0x8000) as usize],
            0x9800..=0x9BFF => self.bg_data1[self.vbk as usize][(addr - 0x9800) as usize],
            0x9C00..=0x9FFF => self.bg_data2[self.vbk as usize][(addr - 0x9C00) as usize],
            0xFE00..=0xFE9F => self.obj[(addr - 0xFE00) as usize],
            Port::LCDC => self.lcdc,
            Port::STAT => self.stat,
            Port::SCY => self.scy,
            Port::SCX => self.scx,
            Port::LY => self.ly,
            Port::LYC => self.lyc,
            Port::DMA => 0xFF,
            Port::BGP => 0xFF,
            Port::OBP0 => 0xFF,
            Port::OBP1 => 0xFF,
            Port::WY => self.wy,
            Port::WX => self.wx,
            Port::VBK => self.vbk,
            Port::HMDA1 => 0xFF,
            Port::HMDA2 => 0xFF,
            Port::HMDA3 => 0xFF,
            Port::HMDA4 => 0xFF,
            Port::HMDA5 => 0xFF,
            Port::BCPS => self.bcps,
            Port::BCPD => self.bcpd, // TODO: palettes are an array that increments
            Port::OCPS => self.ocps,
            Port::OCPD => self.ocpd,
            _ => unreachable!(),
        }
    }

    fn write(&mut self, addr: u16, value: u8) {
        match addr {
            0x8000..=0x97FF => self.chr_data[self.vbk as usize][(addr - 0x8000) as usize] = value,
            0x9800..=0x9BFF => self.bg_data1[self.vbk as usize][(addr - 0x9800) as usize] = value,
            0x9C00..=0x9FFF => self.bg_data2[self.vbk as usize][(addr - 0x9C00) as usize] = value,
            0xFE00..=0xFE9F => self.obj[(addr - 0xFE00) as usize] = value,
            Port::LCDC => self.lcdc = value | 0x01, // cant turn off BG in CGB
            Port::STAT => {
                // a write to LYC match flag resets it for some reason
                let value = if (value & 0x04) != 0 {
                    value ^ 0x04
                } else {
                    value
                };
                self.stat = (value & 0b1111_1100) | (self.stat & 0b0000_0011);
            }
            Port::SCY => self.scy = value,
            Port::SCX => self.scx = value,
            Port::LY => {}
            Port::LYC => self.lyc = value,
            Port::DMA => todo!(),
            Port::BGP => self.bgp = value,
            Port::OBP0 => self.obp0 = value,
            Port::OBP1 => self.obp1 = value,
            Port::WY => self.wy = value,
            Port::WX => self.wx = value,
            Port::VBK => self.vbk = value,
            Port::HMDA1 => todo!(),
            Port::HMDA2 => todo!(),
            Port::HMDA3 => todo!(),
            Port::HMDA4 => todo!(),
            Port::HMDA5 => todo!(),
            Port::BCPS => todo!(),
            Port::BCPD => todo!(),
            Port::OCPS => todo!(),
            Port::OCPD => todo!(),
            _ => unreachable!(),
        }
    }

    fn tick(&mut self, bus: &mut B) -> usize {
        if self.dot == 0 {
            if self.ly == self.lyc {
                self.stat |= 0x03;
                // if LYC interrupt enabled, set the stat flag
                if (self.stat & 0b000_0100) != 0 {
                    // TODO: should probably expose direct access to IF on the bus
                    let iflags = bus.read(Port::IF);
                    bus.write(Port::IF, iflags | 0x02);
                }
            } else {
                self.stat &= !0x03;
            }
        }
        // before vblank
        if self.ly < 144 {
            // oam scan
            if self.dot == 0 {
                // switch to mode 2
                self.stat = (self.stat & 0b1111_1100) | 0x02;
                // if mode 2 interrupt enabled, set the stat flag
                if (self.stat & 0b0010_0000) != 0 {
                    let iflags = bus.read(Port::IF);
                    bus.write(Port::IF, iflags | 0x02);
                }
            // drawing mode
            } else if self.dot == 80 {
                // switch to mode 3
                self.stat = (self.stat & 0b1111_1100) | 0x03;
                // if mode 3 interrupt enabled, set the stat flag
                if (self.stat & 0b0100_0000) != 0 {
                    let iflags = bus.read(Port::IF);
                    bus.write(Port::IF, iflags | 0x02);
                }
                self.draw_line(&mut bus.lcd_mut()[self.ly as usize]);
            // hblank mode
            } else if self.dot == 370 {
                // hblank mode
                // switch to mode 0
                self.stat = self.stat & 0b1111_1100;
                // if mode 0 interrupt enabled, set the stat flag
                if (self.stat & 0b0000_1000) != 0 {
                    let iflags = bus.read(Port::IF);
                    bus.write(Port::IF, iflags | 0x02);
                }
            }
            self.dot += 1;
            if self.dot == 456 {
                self.dot = 0;
                self.ly += 1;
            }
            return 0;
        }
        // in vblank
        // vblank start
        let vblank = if (self.ly == 144) && (self.dot == 0) {
            // switch to mode 1
            self.stat = (self.stat & 0b1111_1100) | 0x01;
            // set vblank flag
            let mut iflags = bus.read(Port::IF) | 0x01;
            // if mode 1 interrupt enabled, set the stat flag
            if (self.stat & 0b0001_0000) != 0 {
                iflags |= 0x02;
            }
            bus.write(Port::IF, iflags);
            1
        } else {
            0
        };
        self.dot += 1;
        if self.dot == 456 {
            self.dot = 0;
            self.ly += 1;
            if self.ly == 155 {
                self.ly = 0;
            }
        }
        vblank
    }
}
