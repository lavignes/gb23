//! SM83 (GBZ80) emulation

use super::bus::{Bus, BusDevice, Port};

#[derive(Default)]
pub struct Cpu {
    pc: u16,
    sp: u16,
    af: [u8; 2],
    bc: [u8; 2],
    de: [u8; 2],
    hl: [u8; 2],

    irq: bool,
    ime: bool,
    stopped: bool,
    halted: bool,
}

#[derive(Copy, Clone)]
enum WideRegister {
    PC,
    SP,
    AF,
    BC,
    DE,
    HL,
}

#[derive(Copy, Clone)]
enum Register {
    A,
    B,
    C,
    D,
    E,
    H,
    L,
}

#[derive(Copy, Clone)]
enum Flag {
    Zero = 0x80,
    Negative = 0x40,
    HalfCarry = 0x20,
    Carry = 0x10,
}

#[derive(Copy, Clone)]
enum Condition {
    Zero,
    NotZero,
    Carry,
    NotCarry,
}

impl Cpu {
    pub fn new() -> Self {
        Self::default()
    }

    #[inline(always)]
    fn flag(&self, flag: Flag) -> bool {
        (self.af[0] & (flag as u8)) != 0
    }

    #[inline(always)]
    fn set_flag(&mut self, flag: Flag, value: bool) {
        if value {
            self.af[0] |= flag as u8;
        } else {
            self.af[0] &= !(flag as u8);
        }
    }

    #[inline(always)]
    fn read<B: Bus>(&mut self, bus: &mut B) -> u8 {
        let value = bus.read(self.pc);
        self.pc = self.pc.wrapping_add(1);
        value
    }

    #[inline(always)]
    fn read_wide<B: Bus>(&mut self, bus: &mut B) -> u16 {
        (self.read(bus) as u16) | ((self.read(bus) as u16) << 8)
    }

    #[inline(always)]
    fn register(&self, reg: Register) -> u8 {
        match reg {
            Register::A => self.af[1],
            Register::B => self.bc[1],
            Register::C => self.bc[0],
            Register::D => self.de[1],
            Register::E => self.de[0],
            Register::H => self.hl[1],
            Register::L => self.hl[0],
        }
    }

    #[inline(always)]
    fn set_register(&mut self, reg: Register, value: u8) {
        match reg {
            Register::A => self.af[1] = value,
            Register::B => self.bc[1] = value,
            Register::C => self.bc[0] = value,
            Register::D => self.de[1] = value,
            Register::E => self.de[0] = value,
            Register::H => self.hl[1] = value,
            Register::L => self.hl[0] = value,
        }
    }

    #[inline(always)]
    fn copy_register(&mut self, dest: Register, src: Register) -> usize {
        let value = self.register(src);
        self.set_register(dest, value);
        4
    }

    #[inline(always)]
    fn wide_register(&self, reg: WideRegister) -> u16 {
        match reg {
            WideRegister::PC => self.pc,
            WideRegister::SP => self.sp,
            WideRegister::AF => u16::from_le_bytes(self.af),
            WideRegister::BC => u16::from_le_bytes(self.bc),
            WideRegister::DE => u16::from_le_bytes(self.de),
            WideRegister::HL => u16::from_le_bytes(self.hl),
        }
    }

    #[inline(always)]
    fn set_wide_register(&mut self, reg: WideRegister, value: u16) {
        match reg {
            WideRegister::PC => self.pc = value,
            WideRegister::SP => self.sp = value,
            WideRegister::AF => self.af = value.to_le_bytes(),
            WideRegister::BC => self.bc = value.to_le_bytes(),
            WideRegister::DE => self.de = value.to_le_bytes(),
            WideRegister::HL => self.hl = value.to_le_bytes(),
        }
    }

    #[inline(always)]
    fn nop(&mut self) -> usize {
        4
    }

    #[inline(always)]
    fn read_wide_immediate<B: Bus>(&mut self, bus: &mut B, reg: WideRegister) -> usize {
        let value = self.read_wide(bus);
        self.set_wide_register(reg, value);
        12
    }

    #[inline(always)]
    fn read_immediate<B: Bus>(&mut self, bus: &mut B, reg: Register) -> usize {
        let value = self.read(bus);
        self.set_register(reg, value);
        8
    }

    #[inline(always)]
    fn write_register<B: Bus>(&self, bus: &mut B, address: WideRegister, reg: Register) -> usize {
        bus.write(self.wide_register(address), self.register(reg));
        8
    }

    #[inline(always)]
    fn inc_wide(&mut self, reg: WideRegister) -> usize {
        let value = self.wide_register(reg).wrapping_add(1);
        self.set_wide_register(reg, value);
        8
    }

    #[inline(always)]
    fn dec_wide(&mut self, reg: WideRegister) -> usize {
        let value = self.wide_register(reg).wrapping_sub(1);
        self.set_wide_register(reg, value);
        8
    }

    #[inline(always)]
    fn inc(&mut self, reg: Register) -> usize {
        // TODO: inc/dec half-carry might be wrong
        let mut value = self.register(reg);
        self.set_flag(Flag::HalfCarry, (value & 0x0F) == 0x0F);
        value = value.wrapping_add(1);
        self.set_register(reg, value);
        self.set_flag(Flag::Zero, value == 0x00);
        self.set_flag(Flag::Negative, false);
        4
    }

    #[inline(always)]
    fn dec(&mut self, reg: Register) -> usize {
        let mut value = self.register(reg);
        self.set_flag(Flag::HalfCarry, (value & 0x0F) == 0x00);
        value = value.wrapping_sub(1);
        self.set_register(reg, value);
        self.set_flag(Flag::Zero, value == 0x00);
        self.set_flag(Flag::Negative, true);
        4
    }

    #[inline(always)]
    fn rlc_value(&mut self, value: u8) -> u8 {
        self.set_flag(Flag::Negative, false);
        self.set_flag(Flag::HalfCarry, false);
        let carry = (value & 0x80) >> 7;
        let value = (value << 1) | carry;
        self.set_flag(Flag::Carry, carry != 0);
        self.set_flag(Flag::Zero, value == 0x00);
        value
    }

    #[inline(always)]
    fn rlc(&mut self, reg: Register) -> usize {
        let mut value = self.register(reg);
        value = self.rlc_value(value);
        self.set_register(reg, value);
        8
    }

    #[inline(always)]
    fn rlc_mem<B: Bus>(&mut self, bus: &mut B) -> usize {
        let address = self.wide_register(WideRegister::HL);
        let mut value = bus.read(address);
        value = self.rlc_value(value);
        bus.write(address, value);
        16
    }

    #[inline(always)]
    fn rl_value(&mut self, value: u8) -> u8 {
        self.set_flag(Flag::Negative, false);
        self.set_flag(Flag::HalfCarry, false);
        let carry = (value & 0x80) >> 7;
        let value = (value << 1) | if self.flag(Flag::Carry) { 0x01 } else { 0x00 };
        self.set_flag(Flag::Carry, carry != 0);
        self.set_flag(Flag::Zero, value == 0x00);
        value
    }

    #[inline(always)]
    fn rl(&mut self, reg: Register) -> usize {
        let mut value = self.register(reg);
        value = self.rl_value(value);
        self.set_register(reg, value);
        8
    }

    #[inline(always)]
    fn rl_mem<B: Bus>(&mut self, bus: &mut B) -> usize {
        let address = self.wide_register(WideRegister::HL);
        let mut value = bus.read(address);
        value = self.rl_value(value);
        bus.write(address, value);
        16
    }

    #[inline(always)]
    fn rrc_value(&mut self, value: u8) -> u8 {
        self.set_flag(Flag::Negative, false);
        self.set_flag(Flag::HalfCarry, false);
        let carry = (value & 0x01) << 7;
        let value = (value >> 1) | carry;
        self.set_flag(Flag::Carry, carry != 0);
        self.set_flag(Flag::Zero, value == 0x00);
        value
    }

    #[inline(always)]
    fn rrc(&mut self, reg: Register) -> usize {
        let mut value = self.register(reg);
        value = self.rrc_value(value);
        self.set_register(reg, value);
        8
    }

    #[inline(always)]
    fn rrc_mem<B: Bus>(&mut self, bus: &mut B) -> usize {
        let address = self.wide_register(WideRegister::HL);
        let mut value = bus.read(address);
        value = self.rrc_value(value);
        bus.write(address, value);
        16
    }

    #[inline(always)]
    fn rr_value(&mut self, value: u8) -> u8 {
        self.set_flag(Flag::Negative, false);
        self.set_flag(Flag::HalfCarry, false);
        let carry = (value & 0x01) << 7;
        let value = (value >> 1) | if self.flag(Flag::Carry) { 0x80 } else { 0x00 };
        self.set_flag(Flag::Carry, carry != 0);
        self.set_flag(Flag::Zero, value == 0x00);
        value
    }

    #[inline(always)]
    fn rr(&mut self, reg: Register) -> usize {
        let mut value = self.register(reg);
        value = self.rr_value(value);
        self.set_register(reg, value);
        8
    }

    #[inline(always)]
    fn rr_mem<B: Bus>(&mut self, bus: &mut B) -> usize {
        let address = self.wide_register(WideRegister::HL);
        let mut value = bus.read(address);
        value = self.rr_value(value);
        bus.write(address, value);
        16
    }

    #[inline(always)]
    fn rlca(&mut self) -> usize {
        let mut value = self.register(Register::A);
        value = self.rlc_value(value);
        self.set_register(Register::A, value);
        self.set_flag(Flag::Zero, false);
        4
    }

    #[inline(always)]
    fn rla(&mut self) -> usize {
        let mut value = self.register(Register::A);
        value = self.rl_value(value);
        self.set_register(Register::A, value);
        self.set_flag(Flag::Zero, false);
        4
    }

    #[inline(always)]
    fn rrca(&mut self) -> usize {
        let mut value = self.register(Register::A);
        value = self.rrc_value(value);
        self.set_register(Register::A, value);
        self.set_flag(Flag::Zero, false);
        4
    }

    #[inline(always)]
    fn rra(&mut self) -> usize {
        let mut value = self.register(Register::A);
        value = self.rr_value(value);
        self.set_register(Register::A, value);
        self.set_flag(Flag::Zero, false);
        4
    }

    #[inline(always)]
    fn sla_value(&mut self, value: u8) -> u8 {
        self.set_flag(Flag::Negative, false);
        self.set_flag(Flag::HalfCarry, false);
        let carry = value & 0x80;
        let value = value << 1;
        self.set_flag(Flag::Carry, carry != 0);
        self.set_flag(Flag::Zero, value == 0x00);
        value
    }

    #[inline(always)]
    fn sla(&mut self, reg: Register) -> usize {
        let mut value = self.register(reg);
        value = self.sla_value(value);
        self.set_register(reg, value);
        8
    }

    #[inline(always)]
    fn sla_mem<B: Bus>(&mut self, bus: &mut B) -> usize {
        let address = self.wide_register(WideRegister::HL);
        let mut value = bus.read(address);
        value = self.sla_value(value);
        bus.write(address, value);
        16
    }

    #[inline(always)]
    fn sra_value(&mut self, value: u8) -> u8 {
        self.set_flag(Flag::Negative, false);
        self.set_flag(Flag::HalfCarry, false);
        let value = (value as i8) >> 1;
        self.set_flag(Flag::Carry, false);
        self.set_flag(Flag::Zero, value == 0x00);
        value as u8
    }

    #[inline(always)]
    fn sra(&mut self, reg: Register) -> usize {
        let mut value = self.register(reg);
        value = self.sra_value(value);
        self.set_register(reg, value);
        8
    }

    #[inline(always)]
    fn sra_mem<B: Bus>(&mut self, bus: &mut B) -> usize {
        let address = self.wide_register(WideRegister::HL);
        let mut value = bus.read(address);
        value = self.sra_value(value);
        bus.write(address, value);
        16
    }

    #[inline(always)]
    fn srl_value(&mut self, value: u8) -> u8 {
        self.set_flag(Flag::Negative, false);
        self.set_flag(Flag::HalfCarry, false);
        let value = value >> 1;
        self.set_flag(Flag::Carry, false);
        self.set_flag(Flag::Zero, value == 0x00);
        value
    }

    #[inline(always)]
    fn srl(&mut self, reg: Register) -> usize {
        let mut value = self.register(reg);
        value = self.srl_value(value);
        self.set_register(reg, value);
        8
    }

    #[inline(always)]
    fn srl_mem<B: Bus>(&mut self, bus: &mut B) -> usize {
        let address = self.wide_register(WideRegister::HL);
        let mut value = bus.read(address);
        value = self.srl_value(value);
        bus.write(address, value);
        16
    }

    #[inline(always)]
    fn write_wide<B: Bus>(&self, bus: &mut B, address: u16, value: u16) {
        bus.write(address, (value & 0x00FF) as u8);
        bus.write(address.wrapping_add(1), ((value >> 8) & 0x00FF) as u8);
    }

    #[inline(always)]
    fn write_stack_immediate<B: Bus>(&mut self, bus: &mut B) -> usize {
        let address = self.read_wide(bus);
        self.write_wide(bus, address, self.sp);
        20
    }

    #[inline(always)]
    fn add_wide(&mut self, reg: WideRegister) -> usize {
        // TODO: half carry??
        let hl = u16::from_le_bytes(self.hl) as u32;
        let value = hl.wrapping_add(self.wide_register(reg) as u32);
        self.set_flag(Flag::Negative, false);
        self.set_flag(Flag::Carry, value > 0xFFFF);
        self.set_flag(Flag::HalfCarry, (value & 0x0FFF) < (hl & 0x0FFF));
        self.hl = (value as u16).to_le_bytes();
        8
    }

    #[inline(always)]
    fn read_register<B: Bus>(
        &mut self,
        bus: &mut B,
        address: WideRegister,
        reg: Register,
    ) -> usize {
        let value = bus.read(self.wide_register(address));
        self.set_register(reg, value);
        8
    }

    #[inline(always)]
    fn stop<B: Bus>(&mut self, bus: &mut B) -> usize {
        self.stopped = true;
        self.read(bus);
        4
    }

    #[inline(always)]
    fn jr<B: Bus>(&mut self, bus: &mut B) -> usize {
        // TODO: How does wrapping work here?
        let pc = (self.pc as i16).wrapping_add((self.read(bus) as i8) as i16);
        self.pc = pc as u16;
        12
    }

    #[inline(always)]
    fn jr_condition<B: Bus>(&mut self, bus: &mut B, condition: Condition) -> usize {
        // TODO: How does wrapping work here?
        let pc = (self.pc as i16).wrapping_add((self.read(bus) as i8) as i16);
        let met = match condition {
            Condition::Zero => self.flag(Flag::Zero),
            Condition::NotZero => !self.flag(Flag::Zero),
            Condition::Carry => self.flag(Flag::Carry),
            Condition::NotCarry => !self.flag(Flag::Carry),
        };
        if met {
            self.pc = pc as u16;
            12
        } else {
            8
        }
    }

    #[inline(always)]
    fn pop_value<B: Bus>(&mut self, bus: &mut B) -> u8 {
        let value = bus.read(self.sp);
        self.sp = self.sp.wrapping_add(1);
        value
    }

    #[inline(always)]
    fn pop_wide_value<B: Bus>(&mut self, bus: &mut B) -> u16 {
        (self.pop_value(bus) as u16) | ((self.pop_value(bus) as u16) << 8)
    }

    #[inline(always)]
    fn push_value<B: Bus>(&mut self, bus: &mut B, value: u8) {
        bus.write(self.sp, value);
        self.sp = self.sp.wrapping_sub(1);
    }

    #[inline(always)]
    fn push_wide_value<B: Bus>(&mut self, bus: &mut B, value: u16) {
        self.sp = self.sp.wrapping_sub(2);
        self.write_wide(bus, self.sp, value);
    }

    #[inline(always)]
    fn ret<B: Bus>(&mut self, bus: &mut B) -> usize {
        self.pc = self.pop_wide_value(bus);
        16
    }

    #[inline(always)]
    fn ret_condition<B: Bus>(&mut self, bus: &mut B, condition: Condition) -> usize {
        let met = match condition {
            Condition::Zero => self.flag(Flag::Zero),
            Condition::NotZero => !self.flag(Flag::Zero),
            Condition::Carry => self.flag(Flag::Carry),
            Condition::NotCarry => !self.flag(Flag::Carry),
        };
        if met {
            self.pc = self.pop_wide_value(bus);
            20
        } else {
            8
        }
    }

    #[inline(always)]
    fn daa(&mut self) -> usize {
        unimplemented!("DAA");
        // 4
    }

    #[inline(always)]
    fn scf(&mut self) -> usize {
        self.set_flag(Flag::HalfCarry, false);
        self.set_flag(Flag::Negative, false);
        self.set_flag(Flag::Carry, true);
        4
    }

    #[inline(always)]
    fn ccf(&mut self) -> usize {
        self.set_flag(Flag::HalfCarry, false);
        self.set_flag(Flag::Negative, false);
        let carry = self.flag(Flag::Carry);
        self.set_flag(Flag::Carry, !carry);
        4
    }

    #[inline(always)]
    fn write_a_hli<B: Bus>(&mut self, bus: &mut B) -> usize {
        let address = self.wide_register(WideRegister::HL);
        bus.write(address, self.register(Register::A));
        self.set_wide_register(WideRegister::HL, address.wrapping_add(1));
        8
    }

    #[inline(always)]
    fn write_a_hld<B: Bus>(&mut self, bus: &mut B) -> usize {
        let address = self.wide_register(WideRegister::HL);
        bus.write(address, self.register(Register::A));
        self.set_wide_register(WideRegister::HL, address.wrapping_sub(1));
        8
    }

    #[inline(always)]
    fn read_a_hli<B: Bus>(&mut self, bus: &mut B) -> usize {
        let address = self.wide_register(WideRegister::HL);
        let value = bus.read(address);
        self.set_register(Register::A, value);
        self.set_wide_register(WideRegister::HL, address.wrapping_add(1));
        8
    }

    #[inline(always)]
    fn read_a_hld<B: Bus>(&mut self, bus: &mut B) -> usize {
        let address = self.wide_register(WideRegister::HL);
        let value = bus.read(address);
        self.set_register(Register::A, value);
        self.set_wide_register(WideRegister::HL, address.wrapping_sub(1));
        8
    }

    #[inline(always)]
    fn cpl(&mut self) -> usize {
        let a = self.register(Register::A);
        self.set_register(Register::A, !a);
        4
    }

    #[inline(always)]
    fn inc_mem<B: Bus>(&mut self, bus: &mut B) -> usize {
        let address = self.wide_register(WideRegister::HL);
        let mut value = bus.read(address);
        self.set_flag(Flag::HalfCarry, (value & 0x0F) == 0x0F);
        value = value.wrapping_add(1);
        bus.write(address, value);
        self.set_flag(Flag::Zero, value == 0x00);
        self.set_flag(Flag::Negative, false);
        12
    }

    #[inline(always)]
    fn dec_mem<B: Bus>(&mut self, bus: &mut B) -> usize {
        let address = self.wide_register(WideRegister::HL);
        let mut value = bus.read(address);
        self.set_flag(Flag::HalfCarry, (value & 0x0F) == 0x00);
        value = value.wrapping_sub(1);
        bus.write(address, value);
        self.set_flag(Flag::Zero, value == 0x00);
        self.set_flag(Flag::Negative, true);
        12
    }

    #[inline(always)]
    fn write_mem_immediate<B: Bus>(&mut self, bus: &mut B) -> usize {
        let value = self.read(bus);
        bus.write(self.wide_register(WideRegister::HL), value);
        12
    }

    #[inline(always)]
    fn write_register_immediate<B: Bus>(&mut self, bus: &mut B, reg: Register) -> usize {
        let address = self.read_wide(bus);
        let value = self.register(reg);
        bus.write(address, value);
        16
    }

    #[inline(always)]
    fn read_register_immediate<B: Bus>(&mut self, bus: &mut B, reg: Register) -> usize {
        let address = self.read_wide(bus);
        let value = bus.read(address);
        self.set_register(reg, value);
        16
    }

    #[inline(always)]
    fn halt(&mut self) -> usize {
        self.halted = true;
        4
    }

    #[inline(always)]
    fn add_value(&mut self, value: u8, carry: bool) {
        let a = self.register(Register::A) as u16;
        let value = value as u16;
        let carry = if carry { 1u16 } else { 0u16 };
        let overflow = a + value + carry;
        self.set_flag(
            Flag::HalfCarry,
            ((a & 0x000F) + (value & 0x000F) + carry) > 0x000F,
        );
        self.set_flag(Flag::Carry, overflow > 0x00FF);
        self.set_flag(Flag::Negative, false);
        let result = (overflow & 0x00FF) as u8;
        self.set_register(Register::A, result);
        self.set_flag(Flag::Zero, result == 0x00);
    }

    #[inline(always)]
    fn add(&mut self, reg: Register) -> usize {
        let value = self.register(reg);
        self.add_value(value, false);
        4
    }

    #[inline(always)]
    fn add_mem<B: Bus>(&mut self, bus: &mut B) -> usize {
        let address = self.wide_register(WideRegister::HL);
        let value = bus.read(address);
        self.add_value(value, false);
        8
    }

    #[inline(always)]
    fn add_carry(&mut self, reg: Register) -> usize {
        let value = self.register(reg);
        let carry = self.flag(Flag::Carry);
        self.add_value(value, carry);
        4
    }

    #[inline(always)]
    fn add_carry_mem<B: Bus>(&mut self, bus: &mut B) -> usize {
        let address = self.wide_register(WideRegister::HL);
        let value = bus.read(address);
        let carry = self.flag(Flag::Carry);
        self.add_value(value, carry);
        8
    }

    #[inline(always)]
    fn sub_value(&mut self, value: u8, carry: bool) {
        let a = self.register(Register::A) as i16;
        let value = value as i16;
        let carry = if carry { 1i16 } else { 0i16 };
        let overflow = a - value - carry;
        self.set_flag(
            Flag::HalfCarry,
            ((a & 0x000F) - (value & 0x000F) - carry) < 0x0000,
        );
        self.set_flag(Flag::Carry, overflow < 0x0000);
        self.set_flag(Flag::Negative, true);
        let result = (overflow & 0x00FF) as u8;
        self.set_register(Register::A, result);
        self.set_flag(Flag::Zero, result == 0x00);
    }

    #[inline(always)]
    fn sub(&mut self, reg: Register) -> usize {
        let value = self.register(reg);
        self.sub_value(value, false);
        4
    }

    #[inline(always)]
    fn sub_mem<B: Bus>(&mut self, bus: &mut B) -> usize {
        let address = self.wide_register(WideRegister::HL);
        let value = bus.read(address);
        self.sub_value(value, false);
        8
    }

    #[inline(always)]
    fn sub_carry(&mut self, reg: Register) -> usize {
        let value = self.register(reg);
        let carry = self.flag(Flag::Carry);
        self.sub_value(value, carry);
        4
    }

    #[inline(always)]
    fn sub_carry_mem<B: Bus>(&mut self, bus: &mut B) -> usize {
        let address = self.wide_register(WideRegister::HL);
        let value = bus.read(address);
        let carry = self.flag(Flag::Carry);
        self.sub_value(value, carry);
        8
    }

    #[inline(always)]
    fn and_value(&mut self, value: u8) {
        let mut a = self.register(Register::A);
        a &= value;
        self.set_register(Register::A, a);
        self.set_flag(Flag::Zero, a == 0x00);
        self.set_flag(Flag::Carry, false);
        self.set_flag(Flag::HalfCarry, true);
        self.set_flag(Flag::Carry, false);
    }

    #[inline(always)]
    fn and(&mut self, reg: Register) -> usize {
        let value = self.register(reg);
        self.and_value(value);
        4
    }

    #[inline(always)]
    fn and_mem<B: Bus>(&mut self, bus: &mut B) -> usize {
        let address = self.wide_register(WideRegister::HL);
        let value = bus.read(address);
        self.and_value(value);
        8
    }

    #[inline(always)]
    fn and_immediate<B: Bus>(&mut self, bus: &mut B) -> usize {
        let value = self.read(bus);
        self.and_value(value);
        8
    }

    #[inline(always)]
    fn xor_value(&mut self, value: u8) {
        let mut a = self.register(Register::A);
        a ^= value;
        self.set_register(Register::A, a);
        self.set_flag(Flag::Zero, a == 0x00);
        self.set_flag(Flag::Carry, false);
        self.set_flag(Flag::HalfCarry, false);
        self.set_flag(Flag::Carry, false);
    }

    #[inline(always)]
    fn xor(&mut self, reg: Register) -> usize {
        let value = self.register(reg);
        self.xor_value(value);
        4
    }

    #[inline(always)]
    fn xor_mem<B: Bus>(&mut self, bus: &mut B) -> usize {
        let address = self.wide_register(WideRegister::HL);
        let value = bus.read(address);
        self.xor_value(value);
        8
    }

    #[inline(always)]
    fn xor_immediate<B: Bus>(&mut self, bus: &mut B) -> usize {
        let value = self.read(bus);
        self.xor_value(value);
        8
    }

    #[inline(always)]
    fn or_value(&mut self, value: u8) {
        let mut a = self.register(Register::A);
        a |= value;
        self.set_register(Register::A, a);
        self.set_flag(Flag::Zero, a == 0x00);
        self.set_flag(Flag::Carry, false);
        self.set_flag(Flag::HalfCarry, false);
        self.set_flag(Flag::Carry, false);
    }

    #[inline(always)]
    fn or(&mut self, reg: Register) -> usize {
        let value = self.register(reg);
        self.or_value(value);
        4
    }

    #[inline(always)]
    fn or_mem<B: Bus>(&mut self, bus: &mut B) -> usize {
        let address = self.wide_register(WideRegister::HL);
        let value = bus.read(address);
        self.or_value(value);
        8
    }

    #[inline(always)]
    fn or_immediate<B: Bus>(&mut self, bus: &mut B) -> usize {
        let value = self.read(bus);
        self.or_value(value);
        8
    }

    #[inline(always)]
    fn cp_value(&mut self, value: u8, carry: bool) {
        let a = self.register(Register::A) as i16;
        let value = value as i16;
        let carry = if carry { 1i16 } else { 0i16 };
        let overflow = a - value - carry;
        self.set_flag(
            Flag::HalfCarry,
            ((a & 0x000F) - (value & 0x000F) - carry) < 0x0000,
        );
        self.set_flag(Flag::Carry, overflow < 0x0000);
        self.set_flag(Flag::Negative, true);
        let result = (overflow & 0x00FF) as u8;
        self.set_flag(Flag::Zero, result == 0x00);
    }

    #[inline(always)]
    fn cp(&mut self, reg: Register) -> usize {
        let value = self.register(reg);
        self.cp_value(value, false);
        4
    }

    #[inline(always)]
    fn cp_mem<B: Bus>(&mut self, bus: &mut B) -> usize {
        let address = self.wide_register(WideRegister::HL);
        let value = bus.read(address);
        self.cp_value(value, false);
        8
    }

    #[inline(always)]
    fn cp_immediate<B: Bus>(&mut self, bus: &mut B) -> usize {
        let value = self.read(bus);
        self.cp_value(value, false);
        8
    }

    #[inline(always)]
    fn pop_wide<B: Bus>(&mut self, bus: &mut B, reg: WideRegister) -> usize {
        let value = self.pop_wide_value(bus);
        self.set_wide_register(reg, value);
        12
    }

    #[inline(always)]
    fn push_wide<B: Bus>(&mut self, bus: &mut B, reg: WideRegister) -> usize {
        let value = self.wide_register(reg);
        self.push_wide_value(bus, value);
        16
    }

    #[inline(always)]
    fn jmp<B: Bus>(&mut self, bus: &mut B) -> usize {
        self.pc = self.read_wide(bus);
        16
    }

    #[inline(always)]
    fn jmp_condition<B: Bus>(&mut self, bus: &mut B, condition: Condition) -> usize {
        let address = self.read_wide(bus);
        let met = match condition {
            Condition::Zero => self.flag(Flag::Zero),
            Condition::NotZero => !self.flag(Flag::Zero),
            Condition::Carry => self.flag(Flag::Carry),
            Condition::NotCarry => !self.flag(Flag::Carry),
        };
        if met {
            self.pc = address;
            16
        } else {
            12
        }
    }

    #[inline(always)]
    fn call<B: Bus>(&mut self, bus: &mut B) -> usize {
        let address = self.read_wide(bus);
        self.push_wide(bus, WideRegister::PC);
        self.pc = address;
        24
    }

    #[inline(always)]
    fn call_condition<B: Bus>(&mut self, bus: &mut B, condition: Condition) -> usize {
        let address = self.read_wide(bus);
        let met = match condition {
            Condition::Zero => self.flag(Flag::Zero),
            Condition::NotZero => !self.flag(Flag::Zero),
            Condition::Carry => self.flag(Flag::Carry),
            Condition::NotCarry => !self.flag(Flag::Carry),
        };
        if met {
            self.push_wide(bus, WideRegister::PC);
            self.pc = address;
            24
        } else {
            12
        }
    }

    #[inline(always)]
    fn add_immediate<B: Bus>(&mut self, bus: &mut B) -> usize {
        let value = self.read(bus);
        self.add_value(value, false);
        8
    }

    #[inline(always)]
    fn add_carry_immediate<B: Bus>(&mut self, bus: &mut B) -> usize {
        let value = self.read(bus);
        let carry = self.flag(Flag::Carry);
        self.add_value(value, carry);
        8
    }

    #[inline(always)]
    fn sub_immediate<B: Bus>(&mut self, bus: &mut B) -> usize {
        let value = self.read(bus);
        self.sub_value(value, false);
        8
    }

    #[inline(always)]
    fn sub_carry_immediate<B: Bus>(&mut self, bus: &mut B) -> usize {
        let value = self.read(bus);
        let carry = self.flag(Flag::Carry);
        self.sub_value(value, carry);
        8
    }

    #[inline(always)]
    fn rst<B: Bus>(&mut self, bus: &mut B, address: u16) -> usize {
        self.push_wide(bus, WideRegister::PC);
        self.pc = address;
        16
    }

    #[inline(always)]
    fn reti<B: Bus>(&mut self, bus: &mut B) -> usize {
        self.ime = true;
        self.ret(bus)
    }

    #[inline(always)]
    fn write_high_offset<B: Bus>(&mut self, bus: &mut B, offset: u8, value: u8) {
        let address = 0xFF00 + (offset as u16);
        bus.write(address, value);
    }

    #[inline(always)]
    fn write_high_immediate<B: Bus>(&mut self, bus: &mut B, reg: Register) -> usize {
        let offset = self.read(bus);
        let value = self.register(reg);
        self.write_high_offset(bus, offset, value);
        12
    }

    #[inline(always)]
    fn write_high_register<B: Bus>(&mut self, bus: &mut B, off: Register, reg: Register) -> usize {
        let offset = self.register(off);
        let value = self.register(reg);
        self.write_high_offset(bus, offset, value);
        8
    }

    #[inline(always)]
    fn read_high_offset<B: Bus>(&mut self, bus: &mut B, offset: u8) -> u8 {
        let address = 0xFF00 + (offset as u16);
        bus.read(address)
    }

    #[inline(always)]
    fn read_high_immediate<B: Bus>(&mut self, bus: &mut B, reg: Register) -> usize {
        let offset = self.read(bus);
        let value = self.read_high_offset(bus, offset);
        self.set_register(reg, value);
        12
    }

    #[inline(always)]
    fn read_high_register<B: Bus>(&mut self, bus: &mut B, off: Register, reg: Register) -> usize {
        let offset = self.register(off);
        let value = self.read_high_offset(bus, offset);
        self.set_register(reg, value);
        8
    }

    #[inline(always)]
    fn add_sp<B: Bus>(&mut self, bus: &mut B) -> usize {
        let sp = self.sp as i32;
        let value = (self.read(bus) as i8) as i32;
        let overflow = sp + value;
        self.set_flag(Flag::Negative, false);
        self.set_flag(Flag::Zero, false);
        self.set_flag(Flag::HalfCarry, (overflow & 0x0FFF) < (sp & 0x0FFFF));
        self.sp = (overflow & 0xFFFF) as u16;
        self.set_flag(Flag::Carry, overflow > 0xFFFF);
        16
    }

    #[inline(always)]
    fn jmp_hl(&mut self) -> usize {
        self.pc = u16::from_le_bytes(self.hl);
        4
    }

    #[inline(always)]
    fn di(&mut self) -> usize {
        self.ime = false;
        4
    }

    #[inline(always)]
    fn ei(&mut self) -> usize {
        self.ime = true;
        4
    }

    #[inline(always)]
    fn copy_wide_register(&mut self, dest: WideRegister, src: WideRegister) -> usize {
        let value = self.wide_register(src);
        self.set_wide_register(dest, value);
        8
    }

    #[inline(always)]
    fn swap_value(&mut self, value: u8) -> u8 {
        self.set_flag(Flag::Carry, false);
        self.set_flag(Flag::HalfCarry, false);
        self.set_flag(Flag::Negative, false);
        let value = ((value << 4) & 0xF0) | ((value >> 4) & 0x0F);
        self.set_flag(Flag::Zero, value == 0x00);
        value
    }

    #[inline(always)]
    fn swap(&mut self, reg: Register) -> usize {
        let mut value = self.register(reg);
        value = self.swap_value(value);
        self.set_register(reg, value);
        8
    }

    #[inline(always)]
    fn swap_mem<B: Bus>(&mut self, bus: &mut B) -> usize {
        let address = self.wide_register(WideRegister::HL);
        let mut value = bus.read(address);
        value = self.swap_value(value);
        bus.write(address, value);
        16
    }

    #[inline(always)]
    fn bit_value(&mut self, bit: u8, value: u8) {
        self.set_flag(Flag::Negative, false);
        self.set_flag(Flag::HalfCarry, true);
        self.set_flag(Flag::Zero, (value & (1 << bit)) == 0x00);
    }

    #[inline(always)]
    fn bit(&mut self, bit: u8, reg: Register) -> usize {
        let value = self.register(reg);
        self.bit_value(bit, value);
        8
    }

    #[inline(always)]
    fn bit_mem<B: Bus>(&mut self, bus: &mut B, bit: u8) -> usize {
        let address = self.wide_register(WideRegister::HL);
        let value = bus.read(address);
        self.bit_value(bit, value);
        16
    }

    #[inline(always)]
    fn reset_bit_value(&mut self, bit: u8, value: u8) -> u8 {
        value & !(1 << bit)
    }

    #[inline(always)]
    fn reset_bit(&mut self, bit: u8, reg: Register) -> usize {
        let mut value = self.register(reg);
        value = self.reset_bit_value(bit, value);
        self.set_register(reg, value);
        8
    }

    #[inline(always)]
    fn reset_bit_mem<B: Bus>(&mut self, bus: &mut B, bit: u8) -> usize {
        let address = self.wide_register(WideRegister::HL);
        let mut value = bus.read(address);
        value = self.reset_bit_value(bit, value);
        bus.write(address, value);
        16
    }

    #[inline(always)]
    fn set_bit_value(&mut self, bit: u8, value: u8) -> u8 {
        value | (1 << bit)
    }

    #[inline(always)]
    fn set_bit(&mut self, bit: u8, reg: Register) -> usize {
        let mut value = self.register(reg);
        value = self.set_bit_value(bit, value);
        self.set_register(reg, value);
        8
    }

    #[inline(always)]
    fn set_bit_mem<B: Bus>(&mut self, bus: &mut B, bit: u8) -> usize {
        let address = self.wide_register(WideRegister::HL);
        let mut value = bus.read(address);
        value = self.set_bit_value(bit, value);
        bus.write(address, value);
        16
    }

    fn cb<B: Bus>(&mut self, bus: &mut B) -> usize {
        let opcode = self.read(bus);
        match opcode {
            0x00 => self.rlc(Register::B),
            0x01 => self.rlc(Register::C),
            0x02 => self.rlc(Register::D),
            0x03 => self.rlc(Register::E),
            0x04 => self.rlc(Register::H),
            0x05 => self.rlc(Register::L),
            0x06 => self.rlc_mem(bus),
            0x07 => self.rlc(Register::A),
            0x08 => self.rrc(Register::B),
            0x09 => self.rrc(Register::C),
            0x0A => self.rrc(Register::D),
            0x0B => self.rrc(Register::E),
            0x0C => self.rrc(Register::H),
            0x0D => self.rrc(Register::L),
            0x0E => self.rrc_mem(bus),
            0x0F => self.rrc(Register::A),

            0x10 => self.rl(Register::B),
            0x11 => self.rl(Register::C),
            0x12 => self.rl(Register::D),
            0x13 => self.rl(Register::E),
            0x14 => self.rl(Register::H),
            0x15 => self.rl(Register::L),
            0x16 => self.rl_mem(bus),
            0x17 => self.rl(Register::A),
            0x18 => self.rr(Register::B),
            0x19 => self.rr(Register::C),
            0x1A => self.rr(Register::D),
            0x1B => self.rr(Register::E),
            0x1C => self.rr(Register::H),
            0x1D => self.rr(Register::L),
            0x1E => self.rr_mem(bus),
            0x1F => self.rr(Register::A),

            0x20 => self.sla(Register::B),
            0x21 => self.sla(Register::C),
            0x22 => self.sla(Register::D),
            0x23 => self.sla(Register::E),
            0x24 => self.sla(Register::H),
            0x25 => self.sla(Register::L),
            0x26 => self.sla_mem(bus),
            0x27 => self.sla(Register::A),
            0x28 => self.sra(Register::B),
            0x29 => self.sra(Register::C),
            0x2A => self.sra(Register::D),
            0x2B => self.sra(Register::E),
            0x2C => self.sra(Register::H),
            0x2D => self.sra(Register::L),
            0x2E => self.sra_mem(bus),
            0x2F => self.sra(Register::A),

            0x30 => self.swap(Register::B),
            0x31 => self.swap(Register::C),
            0x32 => self.swap(Register::D),
            0x33 => self.swap(Register::E),
            0x34 => self.swap(Register::H),
            0x35 => self.swap(Register::L),
            0x36 => self.swap_mem(bus),
            0x37 => self.swap(Register::A),
            0x38 => self.srl(Register::B),
            0x39 => self.srl(Register::C),
            0x3A => self.srl(Register::D),
            0x3B => self.srl(Register::E),
            0x3C => self.srl(Register::H),
            0x3D => self.srl(Register::L),
            0x3E => self.srl_mem(bus),
            0x3F => self.srl(Register::A),

            0x40 => self.bit(0x00, Register::B),
            0x41 => self.bit(0x00, Register::C),
            0x42 => self.bit(0x00, Register::D),
            0x43 => self.bit(0x00, Register::E),
            0x44 => self.bit(0x00, Register::H),
            0x45 => self.bit(0x00, Register::L),
            0x46 => self.bit_mem(bus, 0x00),
            0x47 => self.bit(0x00, Register::A),
            0x48 => self.bit(0x01, Register::B),
            0x49 => self.bit(0x01, Register::C),
            0x4A => self.bit(0x01, Register::D),
            0x4B => self.bit(0x01, Register::E),
            0x4C => self.bit(0x01, Register::H),
            0x4D => self.bit(0x01, Register::L),
            0x4E => self.bit_mem(bus, 0x01),
            0x4F => self.bit(0x01, Register::A),

            0x50 => self.bit(0x02, Register::B),
            0x51 => self.bit(0x02, Register::C),
            0x52 => self.bit(0x02, Register::D),
            0x53 => self.bit(0x02, Register::E),
            0x54 => self.bit(0x02, Register::H),
            0x55 => self.bit(0x02, Register::L),
            0x56 => self.bit_mem(bus, 0x02),
            0x57 => self.bit(0x02, Register::A),
            0x58 => self.bit(0x03, Register::B),
            0x59 => self.bit(0x03, Register::C),
            0x5A => self.bit(0x03, Register::D),
            0x5B => self.bit(0x03, Register::E),
            0x5C => self.bit(0x03, Register::H),
            0x5D => self.bit(0x03, Register::L),
            0x5E => self.bit_mem(bus, 0x03),
            0x5F => self.bit(0x03, Register::A),

            0x60 => self.bit(0x04, Register::B),
            0x61 => self.bit(0x04, Register::C),
            0x62 => self.bit(0x04, Register::D),
            0x63 => self.bit(0x04, Register::E),
            0x64 => self.bit(0x04, Register::H),
            0x65 => self.bit(0x04, Register::L),
            0x66 => self.bit_mem(bus, 0x04),
            0x67 => self.bit(0x04, Register::A),
            0x68 => self.bit(0x05, Register::B),
            0x69 => self.bit(0x05, Register::C),
            0x6A => self.bit(0x05, Register::D),
            0x6B => self.bit(0x05, Register::E),
            0x6C => self.bit(0x05, Register::H),
            0x6D => self.bit(0x05, Register::L),
            0x6E => self.bit_mem(bus, 0x05),
            0x6F => self.bit(0x05, Register::A),

            0x70 => self.bit(0x06, Register::B),
            0x71 => self.bit(0x06, Register::C),
            0x72 => self.bit(0x06, Register::D),
            0x73 => self.bit(0x06, Register::E),
            0x74 => self.bit(0x06, Register::H),
            0x75 => self.bit(0x06, Register::L),
            0x76 => self.bit_mem(bus, 0x06),
            0x77 => self.bit(0x06, Register::A),
            0x78 => self.bit(0x07, Register::B),
            0x79 => self.bit(0x07, Register::C),
            0x7A => self.bit(0x07, Register::D),
            0x7B => self.bit(0x07, Register::E),
            0x7C => self.bit(0x07, Register::H),
            0x7D => self.bit(0x07, Register::L),
            0x7E => self.bit_mem(bus, 0x07),
            0x7F => self.bit(0x07, Register::A),

            0x80 => self.reset_bit(0x00, Register::B),
            0x81 => self.reset_bit(0x00, Register::C),
            0x82 => self.reset_bit(0x00, Register::D),
            0x83 => self.reset_bit(0x00, Register::E),
            0x84 => self.reset_bit(0x00, Register::H),
            0x85 => self.reset_bit(0x00, Register::L),
            0x86 => self.reset_bit_mem(bus, 0x00),
            0x87 => self.reset_bit(0x00, Register::A),
            0x88 => self.reset_bit(0x01, Register::B),
            0x89 => self.reset_bit(0x01, Register::C),
            0x8A => self.reset_bit(0x01, Register::D),
            0x8B => self.reset_bit(0x01, Register::E),
            0x8C => self.reset_bit(0x01, Register::H),
            0x8D => self.reset_bit(0x01, Register::L),
            0x8E => self.reset_bit_mem(bus, 0x01),
            0x8F => self.reset_bit(0x01, Register::A),

            0x90 => self.reset_bit(0x02, Register::B),
            0x91 => self.reset_bit(0x02, Register::C),
            0x92 => self.reset_bit(0x02, Register::D),
            0x93 => self.reset_bit(0x02, Register::E),
            0x94 => self.reset_bit(0x02, Register::H),
            0x95 => self.reset_bit(0x02, Register::L),
            0x96 => self.reset_bit_mem(bus, 0x02),
            0x97 => self.reset_bit(0x02, Register::A),
            0x98 => self.reset_bit(0x03, Register::B),
            0x99 => self.reset_bit(0x03, Register::C),
            0x9A => self.reset_bit(0x03, Register::D),
            0x9B => self.reset_bit(0x03, Register::E),
            0x9C => self.reset_bit(0x03, Register::H),
            0x9D => self.reset_bit(0x03, Register::L),
            0x9E => self.reset_bit_mem(bus, 0x03),
            0x9F => self.reset_bit(0x03, Register::A),

            0xA0 => self.reset_bit(0x04, Register::B),
            0xA1 => self.reset_bit(0x04, Register::C),
            0xA2 => self.reset_bit(0x04, Register::D),
            0xA3 => self.reset_bit(0x04, Register::E),
            0xA4 => self.reset_bit(0x04, Register::H),
            0xA5 => self.reset_bit(0x04, Register::L),
            0xA6 => self.reset_bit_mem(bus, 0x04),
            0xA7 => self.reset_bit(0x04, Register::A),
            0xA8 => self.reset_bit(0x05, Register::B),
            0xA9 => self.reset_bit(0x05, Register::C),
            0xAA => self.reset_bit(0x05, Register::D),
            0xAB => self.reset_bit(0x05, Register::E),
            0xAC => self.reset_bit(0x05, Register::H),
            0xAD => self.reset_bit(0x05, Register::L),
            0xAE => self.reset_bit_mem(bus, 0x05),
            0xAF => self.reset_bit(0x05, Register::A),

            0xB0 => self.reset_bit(0x06, Register::B),
            0xB1 => self.reset_bit(0x06, Register::C),
            0xB2 => self.reset_bit(0x06, Register::D),
            0xB3 => self.reset_bit(0x06, Register::E),
            0xB4 => self.reset_bit(0x06, Register::H),
            0xB5 => self.reset_bit(0x06, Register::L),
            0xB6 => self.reset_bit_mem(bus, 0x06),
            0xB7 => self.reset_bit(0x06, Register::A),
            0xB8 => self.reset_bit(0x07, Register::B),
            0xB9 => self.reset_bit(0x07, Register::C),
            0xBA => self.reset_bit(0x07, Register::D),
            0xBB => self.reset_bit(0x07, Register::E),
            0xBC => self.reset_bit(0x07, Register::H),
            0xBD => self.reset_bit(0x07, Register::L),
            0xBE => self.reset_bit_mem(bus, 0x07),
            0xBF => self.reset_bit(0x07, Register::A),

            0xC0 => self.set_bit(0x00, Register::B),
            0xC1 => self.set_bit(0x00, Register::C),
            0xC2 => self.set_bit(0x00, Register::D),
            0xC3 => self.set_bit(0x00, Register::E),
            0xC4 => self.set_bit(0x00, Register::H),
            0xC5 => self.set_bit(0x00, Register::L),
            0xC6 => self.set_bit_mem(bus, 0x00),
            0xC7 => self.set_bit(0x00, Register::A),
            0xC8 => self.set_bit(0x01, Register::B),
            0xC9 => self.set_bit(0x01, Register::C),
            0xCA => self.set_bit(0x01, Register::D),
            0xCB => self.set_bit(0x01, Register::E),
            0xCC => self.set_bit(0x01, Register::H),
            0xCD => self.set_bit(0x01, Register::L),
            0xCE => self.set_bit_mem(bus, 0x01),
            0xCF => self.set_bit(0x01, Register::A),

            0xD0 => self.set_bit(0x02, Register::B),
            0xD1 => self.set_bit(0x02, Register::C),
            0xD2 => self.set_bit(0x02, Register::D),
            0xD3 => self.set_bit(0x02, Register::E),
            0xD4 => self.set_bit(0x02, Register::H),
            0xD5 => self.set_bit(0x02, Register::L),
            0xD6 => self.set_bit_mem(bus, 0x02),
            0xD7 => self.set_bit(0x02, Register::A),
            0xD8 => self.set_bit(0x03, Register::B),
            0xD9 => self.set_bit(0x03, Register::C),
            0xDA => self.set_bit(0x03, Register::D),
            0xDB => self.set_bit(0x03, Register::E),
            0xDC => self.set_bit(0x03, Register::H),
            0xDD => self.set_bit(0x03, Register::L),
            0xDE => self.set_bit_mem(bus, 0x03),
            0xDF => self.set_bit(0x03, Register::A),

            0xE0 => self.set_bit(0x04, Register::B),
            0xE1 => self.set_bit(0x04, Register::C),
            0xE2 => self.set_bit(0x04, Register::D),
            0xE3 => self.set_bit(0x04, Register::E),
            0xE4 => self.set_bit(0x04, Register::H),
            0xE5 => self.set_bit(0x04, Register::L),
            0xE6 => self.set_bit_mem(bus, 0x04),
            0xE7 => self.set_bit(0x04, Register::A),
            0xE8 => self.set_bit(0x05, Register::B),
            0xE9 => self.set_bit(0x05, Register::C),
            0xEA => self.set_bit(0x05, Register::D),
            0xEB => self.set_bit(0x05, Register::E),
            0xEC => self.set_bit(0x05, Register::H),
            0xED => self.set_bit(0x05, Register::L),
            0xEE => self.set_bit_mem(bus, 0x05),
            0xEF => self.set_bit(0x05, Register::A),

            0xF0 => self.set_bit(0x06, Register::B),
            0xF1 => self.set_bit(0x06, Register::C),
            0xF2 => self.set_bit(0x06, Register::D),
            0xF3 => self.set_bit(0x06, Register::E),
            0xF4 => self.set_bit(0x06, Register::H),
            0xF5 => self.set_bit(0x06, Register::L),
            0xF6 => self.set_bit_mem(bus, 0x06),
            0xF7 => self.set_bit(0x06, Register::A),
            0xF8 => self.set_bit(0x07, Register::B),
            0xF9 => self.set_bit(0x07, Register::C),
            0xFA => self.set_bit(0x07, Register::D),
            0xFB => self.set_bit(0x07, Register::E),
            0xFC => self.set_bit(0x07, Register::H),
            0xFD => self.set_bit(0x07, Register::L),
            0xFE => self.set_bit_mem(bus, 0x07),
            0xFF => self.set_bit(0x07, Register::A),
        }
    }
}

impl<B: Bus> BusDevice<B> for Cpu {
    fn reset(&mut self, _bus: &mut B) {
        self.pc = 0x0000;
        self.sp = 0x1234; // inject garbo
        self.af = [0x56, 0x78];
        self.bc = [0x9A, 0xBC];
        self.de = [0xDE, 0xF0];
        self.hl = [0x12, 0x34];
        self.irq = false;
        self.ime = false;
        self.stopped = false;
        self.halted = false;
    }

    fn read(&mut self, _addr: u16) -> u8 {
        0xFF
    }

    fn write(&mut self, _addr: u16, _value: u8) {}

    fn tick(&mut self, bus: &mut B) -> usize {
        if self.halted {
            // TODO: exit halt state
            return 4;
        }
        // handle interrupts
        if self.irq && self.ime {
            let int_flags = bus.read(Port::IF);
            let int_masked = bus.read(Port::IE) & int_flags;
            if int_masked != 0 {
                if (int_masked & 0x01) != 0x00 {
                    self.rst(bus, 0x0040);
                    bus.write(Port::IF, int_flags ^ 0x01);
                } else if (int_masked & 0x02) != 0x00 {
                    self.rst(bus, 0x0048);
                    bus.write(Port::IF, int_flags ^ 0x02);
                } else if (int_masked & 0x04) != 0x00 {
                    self.rst(bus, 0x0050);
                    bus.write(Port::IF, int_flags ^ 0x04);
                } else if (int_masked & 0x08) != 0x00 {
                    self.rst(bus, 0x0058);
                    bus.write(Port::IF, int_flags ^ 0x08);
                } else if (int_masked & 0x10) != 0x00 {
                    self.rst(bus, 0x0060);
                    bus.write(Port::IF, int_flags ^ 0x10);
                }
                self.ime = false;
                return 20;
            }
        }
        let opcode = self.read(bus);
        match opcode {
            0x00 => self.nop(),
            0x01 => self.read_wide_immediate(bus, WideRegister::BC),
            0x02 => self.write_register(bus, WideRegister::BC, Register::A),
            0x03 => self.inc_wide(WideRegister::BC),
            0x04 => self.inc(Register::B),
            0x05 => self.dec(Register::B),
            0x06 => self.read_immediate(bus, Register::B),
            0x07 => self.rlca(),
            0x08 => self.write_stack_immediate(bus),
            0x09 => self.add_wide(WideRegister::BC),
            0x0A => self.read_register(bus, WideRegister::BC, Register::A),
            0x0B => self.dec_wide(WideRegister::BC),
            0x0C => self.inc(Register::C),
            0x0D => self.dec(Register::C),
            0x0E => self.read_immediate(bus, Register::C),
            0x0F => self.rrca(),

            0x10 => self.stop(bus),
            0x11 => self.read_wide_immediate(bus, WideRegister::DE),
            0x12 => self.write_register(bus, WideRegister::DE, Register::A),
            0x13 => self.inc_wide(WideRegister::DE),
            0x14 => self.inc(Register::D),
            0x15 => self.dec(Register::D),
            0x16 => self.read_immediate(bus, Register::D),
            0x17 => self.rla(),
            0x18 => self.jr(bus),
            0x19 => self.add_wide(WideRegister::DE),
            0x1A => self.read_register(bus, WideRegister::DE, Register::A),
            0x1B => self.dec_wide(WideRegister::DE),
            0x1C => self.inc(Register::E),
            0x1D => self.dec(Register::E),
            0x1E => self.read_immediate(bus, Register::E),
            0x1F => self.rra(),

            0x20 => self.jr_condition(bus, Condition::NotZero),
            0x21 => self.read_wide_immediate(bus, WideRegister::HL),
            0x22 => self.write_a_hli(bus),
            0x23 => self.inc_wide(WideRegister::HL),
            0x24 => self.inc(Register::H),
            0x25 => self.dec(Register::H),
            0x26 => self.read_immediate(bus, Register::H),
            0x27 => self.daa(),
            0x28 => self.jr_condition(bus, Condition::Zero),
            0x29 => self.add_wide(WideRegister::HL),
            0x2A => self.read_a_hli(bus),
            0x2B => self.dec_wide(WideRegister::HL),
            0x2C => self.inc(Register::L),
            0x2D => self.dec(Register::L),
            0x2E => self.read_immediate(bus, Register::L),
            0x2F => self.cpl(),

            0x30 => self.jr_condition(bus, Condition::NotCarry),
            0x31 => self.read_wide_immediate(bus, WideRegister::SP),
            0x32 => self.write_a_hld(bus),
            0x33 => self.inc_wide(WideRegister::SP),
            0x34 => self.inc_mem(bus),
            0x35 => self.dec_mem(bus),
            0x36 => self.write_mem_immediate(bus),
            0x37 => self.scf(),
            0x38 => self.jr_condition(bus, Condition::Carry),
            0x39 => self.add_wide(WideRegister::SP),
            0x3A => self.read_a_hld(bus),
            0x3B => self.dec_wide(WideRegister::SP),
            0x3C => self.inc(Register::A),
            0x3D => self.dec(Register::A),
            0x3E => self.read_immediate(bus, Register::A),
            0x3F => self.ccf(),

            0x40 => self.copy_register(Register::B, Register::B),
            0x41 => self.copy_register(Register::B, Register::C),
            0x42 => self.copy_register(Register::B, Register::D),
            0x43 => self.copy_register(Register::B, Register::E),
            0x44 => self.copy_register(Register::B, Register::H),
            0x45 => self.copy_register(Register::B, Register::L),
            0x46 => self.read_register(bus, WideRegister::HL, Register::B),
            0x47 => self.copy_register(Register::B, Register::A),
            0x48 => self.copy_register(Register::C, Register::B),
            0x49 => self.copy_register(Register::C, Register::C),
            0x4A => self.copy_register(Register::C, Register::D),
            0x4B => self.copy_register(Register::C, Register::E),
            0x4C => self.copy_register(Register::C, Register::H),
            0x4D => self.copy_register(Register::C, Register::L),
            0x4E => self.read_register(bus, WideRegister::HL, Register::C),
            0x4F => self.copy_register(Register::C, Register::A),

            0x50 => self.copy_register(Register::D, Register::B),
            0x51 => self.copy_register(Register::D, Register::C),
            0x52 => self.copy_register(Register::D, Register::D),
            0x53 => self.copy_register(Register::D, Register::E),
            0x54 => self.copy_register(Register::D, Register::H),
            0x55 => self.copy_register(Register::D, Register::L),
            0x56 => self.read_register(bus, WideRegister::HL, Register::D),
            0x57 => self.copy_register(Register::D, Register::A),
            0x58 => self.copy_register(Register::E, Register::B),
            0x59 => self.copy_register(Register::E, Register::C),
            0x5A => self.copy_register(Register::E, Register::D),
            0x5B => self.copy_register(Register::E, Register::E),
            0x5C => self.copy_register(Register::E, Register::H),
            0x5D => self.copy_register(Register::E, Register::L),
            0x5E => self.read_register(bus, WideRegister::HL, Register::E),
            0x5F => self.copy_register(Register::E, Register::A),

            0x60 => self.copy_register(Register::H, Register::B),
            0x61 => self.copy_register(Register::H, Register::C),
            0x62 => self.copy_register(Register::H, Register::D),
            0x63 => self.copy_register(Register::H, Register::E),
            0x64 => self.copy_register(Register::H, Register::H),
            0x65 => self.copy_register(Register::H, Register::L),
            0x66 => self.read_register(bus, WideRegister::HL, Register::H),
            0x67 => self.copy_register(Register::H, Register::A),
            0x68 => self.copy_register(Register::L, Register::B),
            0x69 => self.copy_register(Register::L, Register::C),
            0x6A => self.copy_register(Register::L, Register::D),
            0x6B => self.copy_register(Register::L, Register::E),
            0x6C => self.copy_register(Register::L, Register::H),
            0x6D => self.copy_register(Register::L, Register::L),
            0x6E => self.read_register(bus, WideRegister::HL, Register::L),
            0x6F => self.copy_register(Register::L, Register::A),

            0x70 => self.write_register(bus, WideRegister::HL, Register::B),
            0x71 => self.write_register(bus, WideRegister::HL, Register::C),
            0x72 => self.write_register(bus, WideRegister::HL, Register::D),
            0x73 => self.write_register(bus, WideRegister::HL, Register::E),
            0x74 => self.write_register(bus, WideRegister::HL, Register::H),
            0x75 => self.write_register(bus, WideRegister::HL, Register::L),
            0x76 => self.halt(),
            0x77 => self.write_register(bus, WideRegister::HL, Register::A),
            0x78 => self.copy_register(Register::A, Register::B),
            0x79 => self.copy_register(Register::A, Register::C),
            0x7A => self.copy_register(Register::A, Register::D),
            0x7B => self.copy_register(Register::A, Register::E),
            0x7C => self.copy_register(Register::A, Register::H),
            0x7D => self.copy_register(Register::A, Register::L),
            0x7E => self.read_register(bus, WideRegister::HL, Register::A),
            0x7F => self.copy_register(Register::A, Register::A),

            0x80 => self.add(Register::B),
            0x81 => self.add(Register::C),
            0x82 => self.add(Register::D),
            0x83 => self.add(Register::E),
            0x84 => self.add(Register::H),
            0x85 => self.add(Register::L),
            0x86 => self.add_mem(bus),
            0x87 => self.add(Register::A),
            0x88 => self.add_carry(Register::B),
            0x89 => self.add_carry(Register::C),
            0x8A => self.add_carry(Register::D),
            0x8B => self.add_carry(Register::E),
            0x8C => self.add_carry(Register::H),
            0x8D => self.add_carry(Register::L),
            0x8E => self.add_carry_mem(bus),
            0x8F => self.add_carry(Register::A),

            0x90 => self.sub(Register::B),
            0x91 => self.sub(Register::C),
            0x92 => self.sub(Register::D),
            0x93 => self.sub(Register::E),
            0x94 => self.sub(Register::H),
            0x95 => self.sub(Register::L),
            0x96 => self.sub_mem(bus),
            0x97 => self.sub(Register::A),
            0x98 => self.sub_carry(Register::B),
            0x99 => self.sub_carry(Register::C),
            0x9A => self.sub_carry(Register::D),
            0x9B => self.sub_carry(Register::E),
            0x9C => self.sub_carry(Register::H),
            0x9D => self.sub_carry(Register::L),
            0x9E => self.sub_carry_mem(bus),
            0x9F => self.sub_carry(Register::A),

            0xA0 => self.and(Register::B),
            0xA1 => self.and(Register::C),
            0xA2 => self.and(Register::D),
            0xA3 => self.and(Register::E),
            0xA4 => self.and(Register::H),
            0xA5 => self.and(Register::L),
            0xA6 => self.and_mem(bus),
            0xA7 => self.and(Register::A),
            0xA8 => self.xor(Register::B),
            0xA9 => self.xor(Register::C),
            0xAA => self.xor(Register::D),
            0xAB => self.xor(Register::E),
            0xAC => self.xor(Register::H),
            0xAD => self.xor(Register::L),
            0xAE => self.xor_mem(bus),
            0xAF => self.xor(Register::A),

            0xB0 => self.or(Register::B),
            0xB1 => self.or(Register::C),
            0xB2 => self.or(Register::D),
            0xB3 => self.or(Register::E),
            0xB4 => self.or(Register::H),
            0xB5 => self.or(Register::L),
            0xB6 => self.or_mem(bus),
            0xB7 => self.or(Register::A),
            0xB8 => self.cp(Register::B),
            0xB9 => self.cp(Register::C),
            0xBA => self.cp(Register::D),
            0xBB => self.cp(Register::E),
            0xBC => self.cp(Register::H),
            0xBD => self.cp(Register::L),
            0xBE => self.cp_mem(bus),
            0xBF => self.cp(Register::A),

            0xC0 => self.ret_condition(bus, Condition::NotZero),
            0xC1 => self.pop_wide(bus, WideRegister::BC),
            0xC2 => self.jmp_condition(bus, Condition::NotZero),
            0xC3 => self.jmp(bus),
            0xC4 => self.call_condition(bus, Condition::NotZero),
            0xC5 => self.push_wide(bus, WideRegister::BC),
            0xC6 => self.add_immediate(bus),
            0xC7 => self.rst(bus, 0x0000),
            0xC8 => self.ret_condition(bus, Condition::Zero),
            0xC9 => self.ret(bus),
            0xCA => self.jmp_condition(bus, Condition::Zero),
            0xCB => self.cb(bus),
            0xCC => self.call_condition(bus, Condition::Zero),
            0xCD => self.call(bus),
            0xCE => self.add_carry_immediate(bus),
            0xCF => self.rst(bus, 0x0008),

            0xD0 => self.ret_condition(bus, Condition::NotCarry),
            0xD1 => self.pop_wide(bus, WideRegister::DE),
            0xD2 => self.jmp_condition(bus, Condition::NotCarry),
            0xD3 => unimplemented!("illegal"),
            0xD4 => self.call_condition(bus, Condition::NotCarry),
            0xD5 => self.push_wide(bus, WideRegister::DE),
            0xD6 => self.sub_immediate(bus),
            0xD7 => self.rst(bus, 0x0010),
            0xD8 => self.ret_condition(bus, Condition::Carry),
            0xD9 => self.reti(bus),
            0xDA => self.jmp_condition(bus, Condition::Carry),
            0xDB => unimplemented!("illegal"),
            0xDC => self.call_condition(bus, Condition::Carry),
            0xDD => unimplemented!("illegal"),
            0xDE => self.sub_carry_immediate(bus),
            0xDF => self.rst(bus, 0x0018),

            0xE0 => self.write_high_immediate(bus, Register::A),
            0xE1 => self.pop_wide(bus, WideRegister::HL),
            0xE2 => self.write_high_register(bus, Register::C, Register::A),
            0xE3 => unimplemented!("illegal"),
            0xE4 => unimplemented!("illegal"),
            0xE5 => self.push_wide(bus, WideRegister::HL),
            0xE6 => self.and_immediate(bus),
            0xE7 => self.rst(bus, 0x0020),
            0xE8 => self.add_sp(bus),
            0xE9 => self.jmp_hl(),
            0xEA => self.write_register_immediate(bus, Register::A),
            0xEB => unimplemented!("illegal"),
            0xEC => unimplemented!("illegal"),
            0xED => unimplemented!("illegal"),
            0xEE => self.xor_immediate(bus),
            0xEF => self.rst(bus, 0x0028),

            0xF0 => self.read_high_immediate(bus, Register::A),
            0xF1 => self.pop_wide(bus, WideRegister::AF),
            0xF2 => self.read_high_register(bus, Register::C, Register::A),
            0xF3 => self.di(),
            0xF4 => unimplemented!("illegal"),
            0xF5 => self.push_wide(bus, WideRegister::AF),
            0xF6 => self.or_immediate(bus),
            0xF7 => self.rst(bus, 0x0030),
            0xF8 => unimplemented!("illegal"),
            0xF9 => self.copy_wide_register(WideRegister::SP, WideRegister::HL),
            0xFA => self.read_register_immediate(bus, Register::A),
            0xFB => self.ei(),
            0xFC => unimplemented!("illegal"),
            0xFD => unimplemented!("illegal"),
            0xFE => self.cp_immediate(bus),
            0xFF => self.rst(bus, 0x0038),
        }
    }
}
