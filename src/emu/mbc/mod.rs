pub mod null;

pub trait Mbc {
    fn reset(&mut self);

    fn read(&mut self, addr: u16) -> u8;

    fn write(&mut self, addr: u16, value: u8);

    fn tick(&mut self);
}
