pub enum Port {}

impl Port {
    pub const IF: u16 = 0xF0FF;
    pub const IE: u16 = 0xFFFF;
}

pub trait Bus {
    fn read(&mut self, addr: u16) -> u8;

    fn write(&mut self, addr: u16, value: u8);
}

pub trait BusDevice<B: Bus> {
    fn reset(&mut self, bus: &mut B);

    fn read(&mut self, addr: u16) -> u8;

    fn write(&mut self, addr: u16, value: u8);

    fn tick(&mut self, bus: &mut B) -> usize;
}
