use sdl2::libc;

use super::bus::{Bus, BusDevice, Port};

pub struct Ppu {
    z_buffer: [[u8; 160]; 144],
    chr_data: [[u8; 6144]; 2],
    bg_data1: [[u8; 1024]; 2],
    bg_data2: [[u8; 1024]; 2],
    objs: [u8; 40 * 4],
    dot: usize,
    dma_counter: usize,
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
            z_buffer: [[0; 160]; 144],
            chr_data: [[0xFF; 6144]; 2],
            bg_data1: [[0xFF; 1024]; 2],
            bg_data2: [[0xFF; 1024]; 2],
            objs: [0xFF; 40 * 4],
            dot: 0,
            dma_counter: 0,
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

    #[inline]
    fn bg_color(&self, bits: u8, attr: u8) -> (u32, u8) {
        // TODO: CGB BG priority
        let (index, z) = match bits {
            0 => ((self.bgp & 0x03) >> 0, 0x7F),
            1 => ((self.bgp & 0x0C) >> 2, 0x80),
            2 => ((self.bgp & 0x30) >> 4, 0x80),
            3 => ((self.bgp & 0xC0) >> 6, 0x80),
            _ => unreachable!(),
        };
        let color = match index {
            0 => 0xFFFFFFFF,
            1 => 0xAAAAAAFF,
            2 => 0x555555FF,
            3 => 0x000000FF,
            _ => unreachable!(),
        };
        (color, z)
    }

    #[inline]
    fn obj_color(&self, bits: u8, attr: u8) -> (u32, u8) {
        // first color is always transparent
        if bits == 0 {
            return (0, 0);
        }
        let obp = if (attr & 0x10) == 0 {
            self.obp0
        } else {
            self.obp1
        };
        let index = match bits {
            1 => (obp & 0x0C) >> 2,
            2 => (obp & 0x30) >> 4,
            3 => (obp & 0xC0) >> 6,
            _ => unreachable!(),
        };
        let z = if (attr & 0x80) == 0 { 0xFF } else { 0x7F };
        match index {
            0 => (0xFFFFFFFF, z),
            1 => (0xAAAAAAFF, z),
            2 => (0x555555FF, z),
            3 => (0x000000FF, z),
            _ => unreachable!(),
        }
    }

    fn draw_line(&mut self, line: &mut [u32; 160]) {
        // reset z-buffer
        self.z_buffer[self.ly as usize].fill(0);
        {
            let bg_data = if (self.lcdc & 0x08) == 0 {
                &self.bg_data1
            } else {
                &self.bg_data2
            };
            let bg_y = ((self.ly as usize) + (self.scy as usize)) % 256;
            // we multiply by two because each line of pixles is 2 bytes
            let chr_line_offset = 2 * (bg_y % 8);
            // TODO: This is a crappy but working implementation that
            // looks up and renders each dot one at a time.
            // A better impl would render in batches of 8 pixes
            for dot in 0..160 {
                let bg_x = (dot + (self.scx as usize)) % 256;
                let bg_tile_idx = (bg_x / 8) + ((bg_y / 8) * 32);
                let chr_idx = bg_data[0][bg_tile_idx];
                let attr = bg_data[1][bg_tile_idx];
                let chr_data_offset = if (self.lcdc & 0x10) != 0 {
                    chr_idx as usize * 16
                } else {
                    0x1000usize.wrapping_add_signed(chr_idx as i8 as isize * 16)
                };
                let chr_x = bg_x % 8;
                let lo = self.chr_data[0][chr_data_offset + chr_line_offset];
                let hi = self.chr_data[0][chr_data_offset + chr_line_offset + 1];
                // TODO yuck
                let bitlo = ((lo & ((0x80 >> chr_x) as u8)) != 0) as u8;
                let bithi = ((hi & ((0x80 >> chr_x) as u8)) != 0) as u8;
                let bits = (bithi << 1) | bitlo;
                let (color, z) = self.bg_color(bits, attr);
                if z >= self.z_buffer[self.ly as usize][dot] {
                    self.z_buffer[self.ly as usize][dot] = z;
                    line[dot] = color;
                }
            }
        }
        // sprites?
        if (self.lcdc & 0x02) != 0 {
            let height = if (self.lcdc & 0x04) != 0 { 16 } else { 8 };
            // TODO change this so we search OAM for the first 10 objs
            // on the current line and then iterate over them. the search only looks at Y
            // sprites offscreen in X still count against it
            // Also want to sort them since sprite priority is based on lowest X coord
            for obj in self.objs.chunks(4) {
                // this is the OAM filter algorithm:
                let y = obj[0];
                if ((self.ly + 16) < y) || ((self.ly + 16 - height) >= y) {
                    continue;
                }
                // sprite origins are in the bottom right on gameboy
                // we translate it to make the math simpler
                let y = y.wrapping_sub(16);
                // TODO i think there is a bug here. In 16 height mode,
                // the index of the chr's final bit should always be masked out
                // to zero. I think if I do that it will fix some subtle sprite bugs
                let chr_idx = obj[2] as usize;
                let attr = obj[3];
                // y offset within the sprite intersecting with ly
                let obj_y = self.ly.wrapping_sub(y) % height;
                // y-flip
                let chr_line_offset = if (attr & 0x40) == 0 {
                    // we multiply by two because each line of pixles is 2 bytes
                    2 * (obj_y as usize)
                } else {
                    2 * ((height as usize) - (obj_y as usize) - 1)
                };
                let chr_data_offset = chr_idx as usize * 16;
                let mut lo = self.chr_data[0][chr_data_offset + chr_line_offset];
                let mut hi = self.chr_data[0][chr_data_offset + chr_line_offset + 1];
                // x-flip
                if (attr & 0x20) != 0 {
                    lo = lo.reverse_bits();
                    hi = hi.reverse_bits();
                }
                let x = obj[1].wrapping_sub(8) as usize;
                for i in 0..8 {
                    let dot = (i as usize).wrapping_add(x) % 256;
                    if dot >= 160 {
                        continue;
                    }
                    // TODO yuck
                    let bitlo = ((lo & ((0x80 >> i) as u8)) != 0) as u8;
                    let bithi = ((hi & ((0x80 >> i) as u8)) != 0) as u8;
                    let bits = (bithi << 1) | bitlo;
                    let (color, z) = self.obj_color(bits, attr);
                    if z >= self.z_buffer[self.ly as usize][dot] {
                        self.z_buffer[self.ly as usize][dot] = z;
                        line[dot] = color;
                    }
                }
            }
        }
        // window?
        if (self.lcdc & 0x20) != 0 {
            if self.ly < self.wy {
                return;
            }
            let win_data = if (self.lcdc & 0x40) == 0 {
                &self.bg_data1
            } else {
                &self.bg_data2
            };
            let win_y = (self.ly - self.wy) as usize;
            // offset into the 8 2bpp bytes on the current line (assuming no flip)
            let chr_line_offset = 2 * (win_y % 8);
            for dot in 0..160 {
                // kinda gross, but a WX=7 means its on the very
                // left of the screen
                let win_x = if self.wx < 7 {
                    dot + (7 - (self.wx as usize))
                } else {
                    if dot < ((self.wx as usize) - 7) {
                        continue;
                    }
                    dot - ((self.wx as usize) - 7)
                };
                let win_tile_idx = (win_x / 8) + ((win_y / 8) * 32);
                let chr_idx = win_data[0][win_tile_idx];
                let attr = win_data[1][win_tile_idx];
                let chr_data_offset = if (self.lcdc & 0x10) != 0 {
                    chr_idx as usize * 16
                } else {
                    0x1000usize.wrapping_add_signed(chr_idx as i8 as isize * 16)
                };
                let chr_x = win_x % 8;
                let lo = self.chr_data[0][chr_data_offset + chr_line_offset];
                let hi = self.chr_data[0][chr_data_offset + chr_line_offset + 1];
                // TODO yuck
                let bitlo = ((lo & ((0x80 >> chr_x) as u8)) != 0) as u8;
                let bithi = ((hi & ((0x80 >> chr_x) as u8)) != 0) as u8;
                let bits = (bithi << 1) | bitlo;
                let (color, z) = self.bg_color(bits, attr);
                // window uses is always above bg layer
                let z = z + 1;
                if z >= self.z_buffer[self.ly as usize][dot] {
                    self.z_buffer[self.ly as usize][dot] = z;
                    line[dot] = color;
                }
            }
        }
    }
}

impl<B: Bus> BusDevice<B> for Ppu {
    fn reset(&mut self, _bus: &mut B) {
        // TODO: use real random API
        for b in self.chr_data[0].iter_mut() {
            *b = unsafe { libc::rand() as u8 };
        }
        for b in self.bg_data1[0].iter_mut() {
            *b = unsafe { libc::rand() as u8 };
        }
        for b in self.bg_data2[0].iter_mut() {
            *b = unsafe { libc::rand() as u8 };
        }
        self.dot = 0;
        self.dma_counter = 0;
        self.lcdc = 0;
        self.stat = 0;
        self.scy = 0;
        self.scx = 0;
        self.ly = 0;
        self.lyc = 0;
        self.dma = 0;
        self.bgp = 0;
        self.obp0 = 0;
        self.obp1 = 0;
        self.wy = 0;
        self.wx = 0;
        self.vbk = 0;
        self.hdma1 = 0;
        self.hdma2 = 0;
        self.hdma3 = 0;
        self.hdma4 = 0;
        self.hdma5 = 0;
        self.bcps = 0;
        self.bcpd = 0;
        self.ocps = 0;
        self.ocpd = 0;
    }

    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            0x8000..=0x97FF => self.chr_data[self.vbk as usize][(addr - 0x8000) as usize],
            0x9800..=0x9BFF => self.bg_data1[self.vbk as usize][(addr - 0x9800) as usize],
            0x9C00..=0x9FFF => self.bg_data2[self.vbk as usize][(addr - 0x9C00) as usize],
            0xFE00..=0xFE9F => self.objs[(addr - 0xFE00) as usize],
            Port::LCDC => self.lcdc,
            Port::STAT => self.stat,
            Port::SCY => self.scy,
            Port::SCX => self.scx,
            Port::LY => self.ly,
            Port::LYC => self.lyc,
            Port::DMA => self.dma,
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
            0xFE00..=0xFE9F => self.objs[(addr - 0xFE00) as usize] = value,
            Port::LCDC => self.lcdc = value,
            Port::STAT => {
                // a write to LYC match flag resets it for some reason
                let value = if (value & 0x04) != 0 {
                    value ^ 0x04
                } else {
                    value
                };
                self.stat = (value & 0x7C) | (self.stat & 0x03);
            }
            Port::SCY => self.scy = value,
            Port::SCX => self.scx = value,
            Port::LY => {}
            Port::LYC => self.lyc = value,
            Port::DMA => {
                self.dma = value;
                self.dma_counter = self.objs.len(); // neat
            }
            Port::BGP => self.bgp = value,
            Port::OBP0 => self.obp0 = value,
            Port::OBP1 => self.obp1 = value,
            Port::WY => self.wy = value,
            Port::WX => self.wx = value,
            Port::VBK => self.vbk = value & 0x01,
            Port::HMDA1 => {} //todo!(),
            Port::HMDA2 => {} // todo!(),
            Port::HMDA3 => {} //todo!(),
            Port::HMDA4 => {} // todo!(),
            Port::HMDA5 => {} // todo!(),
            Port::BCPS => {}  //todo!(),
            Port::BCPD => {}  //todo!(),
            Port::OCPS => {}  //todo!(),
            Port::OCPD => {}  // todo!(),
            _ => unreachable!(),
        }
    }

    fn tick(&mut self, bus: &mut B) -> usize {
        // dma active?
        if self.dma_counter > 0 {
            self.dma_counter -= 1;
            // TODO: Need to emulate bus-conflicts for CGB
            // WRAM or ROM must be locked depending
            let addr = ((self.dma as u16) << 8) + (self.dma_counter as u16);
            self.objs[self.dma_counter] = bus.read(addr);
            return 0;
        }
        if (self.lcdc & 0x80) == 0 {
            // TODO: need to emulate blanking the screen when off
            // turned off
            self.stat &= !0x03;
            self.ly = 0;
            self.dot = 0;
            return 0;
        }
        if self.dot == 0 {
            if self.ly == self.lyc {
                self.stat |= 0x04;
                // if LYC interrupt enabled, set the stat flag
                if (self.stat & 0x40) != 0 {
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
                self.stat = (self.stat & 0xFC) | 0x02;
                // if mode 2 interrupt enabled, set the stat flag
                if (self.stat & 0x20) != 0 {
                    let iflags = bus.read(Port::IF);
                    bus.write(Port::IF, iflags | 0x02);
                }
            // drawing mode
            } else if self.dot == 80 {
                // switch to mode 3
                self.stat = (self.stat & 0xFC) | 0x03;
                self.draw_line(&mut bus.lcd_mut()[self.ly as usize]);
            // hblank mode
            } else if self.dot == 370 {
                // hblank mode
                // switch to mode 0
                self.stat = self.stat & 0xFC;
                // if mode 0 interrupt enabled, set the stat flag
                if (self.stat & 0x08) != 0 {
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
            self.stat = (self.stat & 0xFC) | 0x01;
            // set vblank flag
            let mut iflags = bus.read(Port::IF) | 0x01;
            // if mode 1 interrupt enabled, set the stat flag
            if (self.stat & 0x10) != 0 {
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
