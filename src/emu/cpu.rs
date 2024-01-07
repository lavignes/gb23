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
pub enum WideRegister {
    PC,
    SP,
    AF,
    BC,
    DE,
    HL,
}

#[derive(Copy, Clone)]
pub enum Register {
    A,
    F,
    B,
    C,
    D,
    E,
    H,
    L,
}

#[derive(Copy, Clone)]
pub enum Flag {
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
    pub fn flag(&self, flag: Flag) -> bool {
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
    fn fetch<B: Bus>(&mut self, bus: &mut B) -> u8 {
        let value = bus.read(self.pc);
        self.pc = self.pc.wrapping_add(1);
        value
    }

    #[inline(always)]
    fn fetch_wide<B: Bus>(&mut self, bus: &mut B) -> u16 {
        (self.fetch(bus) as u16) | ((self.fetch(bus) as u16) << 8)
    }

    #[inline(always)]
    pub fn register(&self, reg: Register) -> u8 {
        match reg {
            Register::A => self.af[1],
            Register::F => self.af[0],
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
            Register::F => self.af[0] = value,
            Register::B => self.bc[1] = value,
            Register::C => self.bc[0] = value,
            Register::D => self.de[1] = value,
            Register::E => self.de[0] = value,
            Register::H => self.hl[1] = value,
            Register::L => self.hl[0] = value,
        }
    }

    #[inline(always)]
    fn copy(&mut self, dest: Register, src: Register) -> usize {
        let value = self.register(src);
        self.set_register(dest, value);
        4
    }

    #[inline(always)]
    pub fn wide_register(&self, reg: WideRegister) -> u16 {
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
            WideRegister::AF => self.af = (value & 0xFFF0).to_le_bytes(),
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
    fn load_wide_immediate<B: Bus>(&mut self, bus: &mut B, reg: WideRegister) -> usize {
        let value = self.fetch_wide(bus);
        self.set_wide_register(reg, value);
        12
    }

    #[inline(always)]
    fn load_immediate<B: Bus>(&mut self, bus: &mut B, reg: Register) -> usize {
        let value = self.fetch(bus);
        self.set_register(reg, value);
        8
    }

    #[inline(always)]
    fn store_register_indirect<B: Bus>(
        &self,
        bus: &mut B,
        addr: WideRegister,
        reg: Register,
    ) -> usize {
        bus.write(self.wide_register(addr), self.register(reg));
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
        let value = self.register(reg);
        let result = value.wrapping_add(1);
        self.set_register(reg, result);
        self.set_flag(Flag::Zero, result == 0x00);
        self.set_flag(Flag::HalfCarry, ((result ^ value) & 0x10) != 0);
        self.set_flag(Flag::Negative, false);
        4
    }

    #[inline(always)]
    fn dec(&mut self, reg: Register) -> usize {
        let value = self.register(reg);
        let result = value.wrapping_sub(1);
        self.set_register(reg, result);
        self.set_flag(Flag::Zero, result == 0x00);
        self.set_flag(Flag::HalfCarry, ((result ^ value) & 0x10) != 0);
        self.set_flag(Flag::Negative, true);
        4
    }

    #[inline(always)]
    fn rlc_value(&mut self, value: u8) -> u8 {
        let carry = (value & 0x80) >> 7;
        let result = (value << 1) | carry;
        self.set_flag(Flag::Zero, result == 0x00);
        self.set_flag(Flag::HalfCarry, false);
        self.set_flag(Flag::Negative, false);
        self.set_flag(Flag::Carry, carry != 0);
        result
    }

    #[inline(always)]
    fn rlc(&mut self, reg: Register) -> usize {
        let value = self.register(reg);
        let result = self.rlc_value(value);
        self.set_register(reg, result);
        8
    }

    #[inline(always)]
    fn rlc_hl_indirect<B: Bus>(&mut self, bus: &mut B) -> usize {
        let addr = self.wide_register(WideRegister::HL);
        let value = bus.read(addr);
        let result = self.rlc_value(value);
        bus.write(addr, result);
        16
    }

    #[inline(always)]
    fn rl_value(&mut self, value: u8) -> u8 {
        let result = (value << 1) | if self.flag(Flag::Carry) { 0x01 } else { 0x00 };
        self.set_flag(Flag::Zero, result == 0x00);
        self.set_flag(Flag::HalfCarry, false);
        self.set_flag(Flag::Negative, false);
        self.set_flag(Flag::Carry, (value & 0x80) != 0);
        result
    }

    #[inline(always)]
    fn rl(&mut self, reg: Register) -> usize {
        let value = self.register(reg);
        let result = self.rl_value(value);
        self.set_register(reg, result);
        8
    }

    #[inline(always)]
    fn rl_hl_indirect<B: Bus>(&mut self, bus: &mut B) -> usize {
        let addr = self.wide_register(WideRegister::HL);
        let value = bus.read(addr);
        let result = self.rl_value(value);
        bus.write(addr, result);
        16
    }

    #[inline(always)]
    fn rrc_value(&mut self, value: u8) -> u8 {
        let carry = (value & 0x01) << 7;
        let result = (value >> 1) | carry;
        self.set_flag(Flag::Zero, result == 0x00);
        self.set_flag(Flag::HalfCarry, false);
        self.set_flag(Flag::Negative, false);
        self.set_flag(Flag::Carry, carry != 0);
        result
    }

    #[inline(always)]
    fn rrc(&mut self, reg: Register) -> usize {
        let value = self.register(reg);
        let result = self.rrc_value(value);
        self.set_register(reg, result);
        8
    }

    #[inline(always)]
    fn rrc_hl_indirect<B: Bus>(&mut self, bus: &mut B) -> usize {
        let addr = self.wide_register(WideRegister::HL);
        let value = bus.read(addr);
        let result = self.rrc_value(value);
        bus.write(addr, result);
        16
    }

    #[inline(always)]
    fn rr_value(&mut self, value: u8) -> u8 {
        let result = (value >> 1) | if self.flag(Flag::Carry) { 0x80 } else { 0x00 };
        self.set_flag(Flag::Zero, result == 0x00);
        self.set_flag(Flag::HalfCarry, false);
        self.set_flag(Flag::Negative, false);
        self.set_flag(Flag::Carry, (value & 0x01) != 0);
        result
    }

    #[inline(always)]
    fn rr(&mut self, reg: Register) -> usize {
        let value = self.register(reg);
        let result = self.rr_value(value);
        self.set_register(reg, result);
        8
    }

    #[inline(always)]
    fn rr_hl_indirect<B: Bus>(&mut self, bus: &mut B) -> usize {
        let addr = self.wide_register(WideRegister::HL);
        let value = bus.read(addr);
        let result = self.rr_value(value);
        bus.write(addr, result);
        16
    }

    #[inline(always)]
    fn rlca(&mut self) -> usize {
        let value = self.register(Register::A);
        let result = self.rlc_value(value);
        self.set_register(Register::A, result);
        self.set_flag(Flag::Zero, false);
        4
    }

    #[inline(always)]
    fn rla(&mut self) -> usize {
        let value = self.register(Register::A);
        let result = self.rl_value(value);
        self.set_register(Register::A, result);
        self.set_flag(Flag::Zero, false);
        4
    }

    #[inline(always)]
    fn rrca(&mut self) -> usize {
        let value = self.register(Register::A);
        let result = self.rrc_value(value);
        self.set_register(Register::A, result);
        self.set_flag(Flag::Zero, false);
        4
    }

    #[inline(always)]
    fn rra(&mut self) -> usize {
        let value = self.register(Register::A);
        let result = self.rr_value(value);
        self.set_register(Register::A, result);
        self.set_flag(Flag::Zero, false);
        4
    }

    #[inline(always)]
    fn sla_value(&mut self, value: u8) -> u8 {
        let result = value << 1;
        self.set_flag(Flag::Zero, result == 0x00);
        self.set_flag(Flag::HalfCarry, false);
        self.set_flag(Flag::Negative, false);
        self.set_flag(Flag::Carry, (value & 0x80) != 0);
        result
    }

    #[inline(always)]
    fn sla(&mut self, reg: Register) -> usize {
        let value = self.register(reg);
        let result = self.sla_value(value);
        self.set_register(reg, result);
        8
    }

    #[inline(always)]
    fn sla_hl_indirect<B: Bus>(&mut self, bus: &mut B) -> usize {
        let addr = self.wide_register(WideRegister::HL);
        let value = bus.read(addr);
        let result = self.sla_value(value);
        bus.write(addr, result);
        16
    }

    #[inline(always)]
    fn sra_value(&mut self, value: u8) -> u8 {
        let result = (value as i8) >> 1;
        self.set_flag(Flag::Zero, result == 0x00);
        self.set_flag(Flag::HalfCarry, false);
        self.set_flag(Flag::Negative, false);
        self.set_flag(Flag::Carry, (value & 0x01) != 0);
        result as u8
    }

    #[inline(always)]
    fn sra(&mut self, reg: Register) -> usize {
        let value = self.register(reg);
        let result = self.sra_value(value);
        self.set_register(reg, result);
        8
    }

    #[inline(always)]
    fn sra_hl_indirect<B: Bus>(&mut self, bus: &mut B) -> usize {
        let addr = self.wide_register(WideRegister::HL);
        let value = bus.read(addr);
        let result = self.sra_value(value);
        bus.write(addr, result);
        16
    }

    #[inline(always)]
    fn srl_value(&mut self, value: u8) -> u8 {
        let result = value >> 1;
        self.set_flag(Flag::Zero, result == 0x00);
        self.set_flag(Flag::HalfCarry, false);
        self.set_flag(Flag::Negative, false);
        self.set_flag(Flag::Carry, (value & 0x01) != 0);
        result
    }

    #[inline(always)]
    fn srl(&mut self, reg: Register) -> usize {
        let value = self.register(reg);
        let result = self.srl_value(value);
        self.set_register(reg, result);
        8
    }

    #[inline(always)]
    fn srl_hl_indirect<B: Bus>(&mut self, bus: &mut B) -> usize {
        let addr = self.wide_register(WideRegister::HL);
        let value = bus.read(addr);
        let result = self.srl_value(value);
        bus.write(addr, result);
        16
    }

    #[inline(always)]
    fn write_wide<B: Bus>(&self, bus: &mut B, addr: u16, value: u16) {
        bus.write(addr, value as u8);
        bus.write(addr.wrapping_add(1), (value >> 8) as u8);
    }

    #[inline(always)]
    fn write_stack_immediate<B: Bus>(&mut self, bus: &mut B) -> usize {
        let addr = self.fetch_wide(bus);
        self.write_wide(bus, addr, self.sp);
        20
    }

    #[inline(always)]
    fn add_wide(&mut self, reg: WideRegister) -> usize {
        let hl = self.wide_register(WideRegister::HL);
        let rhs = self.wide_register(reg);
        let (result, carry) = hl.overflowing_add(rhs);
        self.set_wide_register(WideRegister::HL, result);
        self.set_flag(Flag::HalfCarry, ((hl ^ result ^ rhs) & 0x1000) != 0);
        self.set_flag(Flag::Negative, false);
        self.set_flag(Flag::Carry, carry);
        8
    }

    #[inline(always)]
    fn load_register_indirect<B: Bus>(
        &mut self,
        bus: &mut B,
        addr: WideRegister,
        reg: Register,
    ) -> usize {
        let value = bus.read(self.wide_register(addr));
        self.set_register(reg, value);
        8
    }

    #[inline(always)]
    fn stop<B: Bus>(&mut self, bus: &mut B) -> usize {
        self.stopped = true;
        self.fetch(bus);
        4
    }

    #[inline(always)]
    fn jr<B: Bus>(&mut self, bus: &mut B) -> usize {
        let offset = self.fetch(bus) as i8 as i16;
        self.pc = self.pc.wrapping_add_signed(offset);
        12
    }

    #[inline(always)]
    fn jr_condition<B: Bus>(&mut self, bus: &mut B, condition: Condition) -> usize {
        let met = match condition {
            Condition::Zero => self.flag(Flag::Zero),
            Condition::NotZero => !self.flag(Flag::Zero),
            Condition::Carry => self.flag(Flag::Carry),
            Condition::NotCarry => !self.flag(Flag::Carry),
        };
        if met {
            self.jr(bus)
        } else {
            self.fetch(bus);
            8
        }
    }

    #[inline(always)]
    fn pop_value<B: Bus>(&mut self, bus: &mut B) -> u16 {
        let lo = bus.read(self.sp);
        self.sp = self.sp.wrapping_add(1);
        let hi = bus.read(self.sp);
        self.sp = self.sp.wrapping_add(1);
        u16::from_le_bytes([lo, hi])
    }

    #[inline(always)]
    fn push_value<B: Bus>(&mut self, bus: &mut B, value: u16) {
        self.sp = self.sp.wrapping_sub(1);
        bus.write(self.sp, (value >> 8) as u8);
        self.sp = self.sp.wrapping_sub(1);
        bus.write(self.sp, value as u8);
    }

    #[inline(always)]
    fn ret<B: Bus>(&mut self, bus: &mut B) -> usize {
        self.pc = self.pop_value(bus);
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
            4 + self.ret(bus)
        } else {
            8
        }
    }

    #[inline(always)]
    fn daa(&mut self) -> usize {
        let value = self.register(Register::A);
        let mut result = value;
        if self.flag(Flag::Negative) {
            if self.flag(Flag::HalfCarry) {
                result = result.wrapping_sub(0x06);
            }
            if self.flag(Flag::Carry) {
                result = result.wrapping_sub(0x60);
            }
        } else {
            if ((value & 0x0F) > 0x09) || self.flag(Flag::HalfCarry) {
                result = result.wrapping_add(0x06);
            }
            if (value > 0x99) || self.flag(Flag::Carry) {
                result = result.wrapping_add(0x60);
                self.set_flag(Flag::Carry, true);
            }
        }
        self.set_register(Register::A, result);
        self.set_flag(Flag::Zero, result == 0x00);
        // TODO should I be reseting H?
        self.set_flag(Flag::HalfCarry, false);
        // TODO Do I always do this?
        // self.set_flag(Flag::Carry, self.flag(Flag::Carry) || (value > 0x99));
        4
    }

    #[inline(always)]
    fn scf(&mut self) -> usize {
        self.set_flag(Flag::Negative, false);
        self.set_flag(Flag::HalfCarry, false);
        self.set_flag(Flag::Carry, true);
        4
    }

    #[inline(always)]
    fn ccf(&mut self) -> usize {
        let carry = self.flag(Flag::Carry);
        self.set_flag(Flag::Negative, false);
        self.set_flag(Flag::HalfCarry, false);
        self.set_flag(Flag::Carry, !carry);
        4
    }

    #[inline(always)]
    fn store_a_hli_indirect<B: Bus>(&mut self, bus: &mut B) -> usize {
        let addr = self.wide_register(WideRegister::HL);
        bus.write(addr, self.register(Register::A));
        self.set_wide_register(WideRegister::HL, addr.wrapping_add(1));
        8
    }

    #[inline(always)]
    fn store_a_hld_indirect<B: Bus>(&mut self, bus: &mut B) -> usize {
        let addr = self.wide_register(WideRegister::HL);
        bus.write(addr, self.register(Register::A));
        self.set_wide_register(WideRegister::HL, addr.wrapping_sub(1));
        8
    }

    #[inline(always)]
    fn load_a_hli_indirect<B: Bus>(&mut self, bus: &mut B) -> usize {
        let addr = self.wide_register(WideRegister::HL);
        let value = bus.read(addr);
        self.set_register(Register::A, value);
        self.set_wide_register(WideRegister::HL, addr.wrapping_add(1));
        8
    }

    #[inline(always)]
    fn load_a_hld_indirect<B: Bus>(&mut self, bus: &mut B) -> usize {
        let addr = self.wide_register(WideRegister::HL);
        let value = bus.read(addr);
        self.set_register(Register::A, value);
        self.set_wide_register(WideRegister::HL, addr.wrapping_sub(1));
        8
    }

    #[inline(always)]
    fn cpl(&mut self) -> usize {
        let a = self.register(Register::A);
        self.set_register(Register::A, !a);
        self.set_flag(Flag::Negative, true);
        self.set_flag(Flag::HalfCarry, true);
        4
    }

    #[inline(always)]
    fn inc_hl_indirect<B: Bus>(&mut self, bus: &mut B) -> usize {
        let addr = self.wide_register(WideRegister::HL);
        let value = bus.read(addr);
        let result = value.wrapping_add(1);
        bus.write(addr, result);
        self.set_flag(Flag::Zero, result == 0x00);
        self.set_flag(Flag::Negative, false);
        self.set_flag(Flag::HalfCarry, ((result ^ value) & 0x10) != 0);
        12
    }

    #[inline(always)]
    fn dec_hl_indirect<B: Bus>(&mut self, bus: &mut B) -> usize {
        let addr = self.wide_register(WideRegister::HL);
        let value = bus.read(addr);
        let result = value.wrapping_sub(1);
        bus.write(addr, result);
        self.set_flag(Flag::Zero, result == 0x00);
        self.set_flag(Flag::Negative, true);
        self.set_flag(Flag::HalfCarry, ((result ^ value) & 0x10) != 0);
        12
    }

    #[inline(always)]
    fn store_immediate_hl_indirect<B: Bus>(&mut self, bus: &mut B) -> usize {
        let value = self.fetch(bus);
        bus.write(self.wide_register(WideRegister::HL), value);
        12
    }

    #[inline(always)]
    fn store_indirect<B: Bus>(&mut self, bus: &mut B) -> usize {
        let addr = self.fetch_wide(bus);
        let value = self.register(Register::A);
        bus.write(addr, value);
        16
    }

    #[inline(always)]
    fn load_indirect<B: Bus>(&mut self, bus: &mut B) -> usize {
        let addr = self.fetch_wide(bus);
        let value = bus.read(addr);
        self.set_register(Register::A, value);
        16
    }

    #[inline(always)]
    fn halt(&mut self) -> usize {
        self.halted = true;
        4
    }

    #[inline(always)]
    fn add_value(&mut self, value: u8, carry: bool) {
        let a = self.register(Register::A);
        let (result, carry) = a.carrying_add(value, carry);
        self.set_register(Register::A, result as u8);
        self.set_flag(Flag::Zero, result == 0x00);
        self.set_flag(Flag::Negative, false);
        self.set_flag(Flag::HalfCarry, ((a ^ value ^ result) & 0x10) != 0);
        self.set_flag(Flag::Carry, carry);
    }

    #[inline(always)]
    fn add(&mut self, reg: Register) -> usize {
        let value = self.register(reg);
        self.add_value(value, false);
        4
    }

    #[inline(always)]
    fn add_hl_indirect<B: Bus>(&mut self, bus: &mut B) -> usize {
        let addr = self.wide_register(WideRegister::HL);
        let value = bus.read(addr);
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
    fn add_carry_hl_indirect<B: Bus>(&mut self, bus: &mut B) -> usize {
        let addr = self.wide_register(WideRegister::HL);
        let value = bus.read(addr);
        let carry = self.flag(Flag::Carry);
        self.add_value(value, carry);
        8
    }

    #[inline(always)]
    fn sub_value(&mut self, value: u8, carry: bool) {
        let a = self.register(Register::A);
        let (result, carry) = a.borrowing_sub(value, carry);
        self.set_register(Register::A, result as u8);
        self.set_flag(Flag::Zero, result == 0x00);
        self.set_flag(Flag::Negative, true);
        self.set_flag(Flag::HalfCarry, ((a ^ value ^ result) & 0x10) != 0);
        self.set_flag(Flag::Carry, carry);
    }

    #[inline(always)]
    fn sub(&mut self, reg: Register) -> usize {
        let value = self.register(reg);
        self.sub_value(value, false);
        4
    }

    #[inline(always)]
    fn sub_hl_indirect<B: Bus>(&mut self, bus: &mut B) -> usize {
        let addr = self.wide_register(WideRegister::HL);
        let value = bus.read(addr);
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
    fn sub_carry_hl_indirect<B: Bus>(&mut self, bus: &mut B) -> usize {
        let addr = self.wide_register(WideRegister::HL);
        let value = bus.read(addr);
        let carry = self.flag(Flag::Carry);
        self.sub_value(value, carry);
        8
    }

    #[inline(always)]
    fn and_value(&mut self, value: u8) {
        let a = self.register(Register::A);
        let result = a & value;
        self.set_register(Register::A, result);
        self.set_flag(Flag::Zero, result == 0x00);
        self.set_flag(Flag::Negative, false);
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
    fn and_hl_indirect<B: Bus>(&mut self, bus: &mut B) -> usize {
        let addr = self.wide_register(WideRegister::HL);
        let value = bus.read(addr);
        self.and_value(value);
        8
    }

    #[inline(always)]
    fn and_immediate<B: Bus>(&mut self, bus: &mut B) -> usize {
        let value = self.fetch(bus);
        self.and_value(value);
        8
    }

    #[inline(always)]
    fn xor_value(&mut self, value: u8) {
        let a = self.register(Register::A);
        let result = a ^ value;
        self.set_register(Register::A, result);
        self.set_flag(Flag::Zero, result == 0x00);
        self.set_flag(Flag::Negative, false);
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
    fn xor_hl_indirect<B: Bus>(&mut self, bus: &mut B) -> usize {
        let addr = self.wide_register(WideRegister::HL);
        let value = bus.read(addr);
        self.xor_value(value);
        8
    }

    #[inline(always)]
    fn xor_immediate<B: Bus>(&mut self, bus: &mut B) -> usize {
        let value = self.fetch(bus);
        self.xor_value(value);
        8
    }

    #[inline(always)]
    fn or_value(&mut self, value: u8) {
        let a = self.register(Register::A);
        let result = a | value;
        self.set_register(Register::A, result);
        self.set_flag(Flag::Zero, result == 0x00);
        self.set_flag(Flag::Negative, false);
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
    fn or_hl_indirect<B: Bus>(&mut self, bus: &mut B) -> usize {
        let addr = self.wide_register(WideRegister::HL);
        let value = bus.read(addr);
        self.or_value(value);
        8
    }

    #[inline(always)]
    fn or_immediate<B: Bus>(&mut self, bus: &mut B) -> usize {
        let value = self.fetch(bus);
        self.or_value(value);
        8
    }

    #[inline(always)]
    fn compare_value(&mut self, value: u8) {
        let a = self.register(Register::A);
        let (result, carry) = a.overflowing_sub(value);
        self.set_flag(Flag::Zero, result == 0x00);
        self.set_flag(Flag::Negative, true);
        self.set_flag(Flag::HalfCarry, ((a ^ value ^ result) & 0x10) != 0);
        self.set_flag(Flag::Carry, carry);
    }

    #[inline(always)]
    fn compare(&mut self, reg: Register) -> usize {
        let value = self.register(reg);
        self.compare_value(value);
        4
    }

    #[inline(always)]
    fn compare_hl_indirect<B: Bus>(&mut self, bus: &mut B) -> usize {
        let addr = self.wide_register(WideRegister::HL);
        let value = bus.read(addr);
        self.compare_value(value);
        8
    }

    #[inline(always)]
    fn compare_immediate<B: Bus>(&mut self, bus: &mut B) -> usize {
        let value = self.fetch(bus);
        self.compare_value(value);
        8
    }

    #[inline(always)]
    fn pop<B: Bus>(&mut self, bus: &mut B, reg: WideRegister) -> usize {
        let value = self.pop_value(bus);
        self.set_wide_register(reg, value);
        12
    }

    #[inline(always)]
    fn push<B: Bus>(&mut self, bus: &mut B, reg: WideRegister) -> usize {
        let value = self.wide_register(reg);
        self.push_value(bus, value);
        16
    }

    #[inline(always)]
    fn jmp<B: Bus>(&mut self, bus: &mut B) -> usize {
        self.pc = self.fetch_wide(bus);
        16
    }

    #[inline(always)]
    fn jmp_condition<B: Bus>(&mut self, bus: &mut B, condition: Condition) -> usize {
        let met = match condition {
            Condition::Zero => self.flag(Flag::Zero),
            Condition::NotZero => !self.flag(Flag::Zero),
            Condition::Carry => self.flag(Flag::Carry),
            Condition::NotCarry => !self.flag(Flag::Carry),
        };
        if met {
            self.jmp(bus)
        } else {
            self.fetch_wide(bus);
            12
        }
    }

    #[inline(always)]
    fn call<B: Bus>(&mut self, bus: &mut B) -> usize {
        let addr = self.fetch_wide(bus);
        self.push(bus, WideRegister::PC);
        self.pc = addr;
        24
    }

    #[inline(always)]
    fn call_condition<B: Bus>(&mut self, bus: &mut B, condition: Condition) -> usize {
        let met = match condition {
            Condition::Zero => self.flag(Flag::Zero),
            Condition::NotZero => !self.flag(Flag::Zero),
            Condition::Carry => self.flag(Flag::Carry),
            Condition::NotCarry => !self.flag(Flag::Carry),
        };
        if met {
            self.call(bus)
        } else {
            self.fetch_wide(bus);
            12
        }
    }

    #[inline(always)]
    fn add_immediate<B: Bus>(&mut self, bus: &mut B) -> usize {
        let value = self.fetch(bus);
        self.add_value(value, false);
        8
    }

    #[inline(always)]
    fn add_carry_immediate<B: Bus>(&mut self, bus: &mut B) -> usize {
        let value = self.fetch(bus);
        let carry = self.flag(Flag::Carry);
        self.add_value(value, carry);
        8
    }

    #[inline(always)]
    fn sub_immediate<B: Bus>(&mut self, bus: &mut B) -> usize {
        let value = self.fetch(bus);
        self.sub_value(value, false);
        8
    }

    #[inline(always)]
    fn sub_carry_immediate<B: Bus>(&mut self, bus: &mut B) -> usize {
        let value = self.fetch(bus);
        let carry = self.flag(Flag::Carry);
        self.sub_value(value, carry);
        8
    }

    #[inline(always)]
    fn rst<B: Bus>(&mut self, bus: &mut B, addr: u16) -> usize {
        self.push(bus, WideRegister::PC);
        self.pc = addr;
        16
    }

    #[inline(always)]
    fn reti<B: Bus>(&mut self, bus: &mut B) -> usize {
        self.ime = true;
        self.ret(bus)
    }

    #[inline(always)]
    fn write_high_offset<B: Bus>(&mut self, bus: &mut B, offset: u8, value: u8) {
        let addr = 0xFF00 | (offset as u16);
        bus.write(addr, value);
    }

    #[inline(always)]
    fn store_high_indirect<B: Bus>(&mut self, bus: &mut B) -> usize {
        let offset = self.fetch(bus);
        let value = self.register(Register::A);
        self.write_high_offset(bus, offset, value);
        12
    }

    #[inline(always)]
    fn store_high_c_indirect<B: Bus>(&mut self, bus: &mut B) -> usize {
        let offset = self.register(Register::C);
        let value = self.register(Register::A);
        self.write_high_offset(bus, offset, value);
        8
    }

    #[inline(always)]
    fn read_high_indirect<B: Bus>(&mut self, bus: &mut B, offset: u8) -> u8 {
        let addr = 0xFF00 + (offset as u16);
        bus.read(addr)
    }

    #[inline(always)]
    fn load_high_indirect<B: Bus>(&mut self, bus: &mut B) -> usize {
        let offset = self.fetch(bus);
        let value = self.read_high_indirect(bus, offset);
        self.set_register(Register::A, value);
        12
    }

    #[inline(always)]
    fn load_high_c_indirect<B: Bus>(&mut self, bus: &mut B) -> usize {
        let offset = self.register(Register::C);
        let value = self.read_high_indirect(bus, offset);
        self.set_register(Register::A, value);
        8
    }

    #[inline(always)]
    fn add_sp<B: Bus>(&mut self, bus: &mut B) -> usize {
        let sp = self.sp;
        let offset = self.fetch(bus);
        let rhs = offset as i8 as i16;
        let result = sp.wrapping_add_signed(rhs);
        self.sp = result;
        // flags are actually based on unsigned add of lo byte of sp
        let lo = sp as u8;
        let (result, carry) = lo.overflowing_add(offset);
        self.set_flag(Flag::Zero, false);
        self.set_flag(Flag::Negative, false);
        self.set_flag(Flag::HalfCarry, ((lo ^ result ^ offset) & 0x10) != 0);
        self.set_flag(Flag::Carry, carry);
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
    fn load_sp_indirect<B: Bus>(&mut self, bus: &mut B) -> usize {
        let sp = self.sp;
        let offset = self.fetch(bus);
        let rhs = offset as i8 as i16;
        let result = sp.wrapping_add_signed(rhs);
        self.set_wide_register(WideRegister::HL, result);
        // flags are actually based on unsigned add of lo byte of sp
        let lo = sp as u8;
        let (result, carry) = lo.overflowing_add(offset);
        self.set_flag(Flag::Zero, false);
        self.set_flag(Flag::Negative, false);
        self.set_flag(Flag::HalfCarry, ((lo ^ result ^ offset) & 0x10) != 0);
        self.set_flag(Flag::Carry, carry);
        12
    }

    #[inline(always)]
    fn ei(&mut self) -> usize {
        self.ime = true;
        4
    }

    #[inline(always)]
    fn copy_wide(&mut self, dest: WideRegister, src: WideRegister) -> usize {
        let value = self.wide_register(src);
        self.set_wide_register(dest, value);
        8
    }

    #[inline(always)]
    fn swap_value(&mut self, value: u8) -> u8 {
        let result = ((value << 4) & 0xF0) | ((value >> 4) & 0x0F);
        self.set_flag(Flag::Carry, false);
        self.set_flag(Flag::HalfCarry, false);
        self.set_flag(Flag::Negative, false);
        self.set_flag(Flag::Zero, result == 0x00);
        result
    }

    #[inline(always)]
    fn swap(&mut self, reg: Register) -> usize {
        let value = self.register(reg);
        let result = self.swap_value(value);
        self.set_register(reg, result);
        8
    }

    #[inline(always)]
    fn swap_hl_indirect<B: Bus>(&mut self, bus: &mut B) -> usize {
        let addr = self.wide_register(WideRegister::HL);
        let value = bus.read(addr);
        let result = self.swap_value(value);
        bus.write(addr, result);
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
    fn bit_hl_indirect<B: Bus>(&mut self, bus: &mut B, bit: u8) -> usize {
        let addr = self.wide_register(WideRegister::HL);
        let value = bus.read(addr);
        self.bit_value(bit, value);
        16
    }

    #[inline(always)]
    fn reset_bit_value(&mut self, bit: u8, value: u8) -> u8 {
        value & !(1 << bit)
    }

    #[inline(always)]
    fn reset_bit(&mut self, bit: u8, reg: Register) -> usize {
        let value = self.register(reg);
        let result = self.reset_bit_value(bit, value);
        self.set_register(reg, result);
        8
    }

    #[inline(always)]
    fn reset_bit_hl_indirect<B: Bus>(&mut self, bus: &mut B, bit: u8) -> usize {
        let addr = self.wide_register(WideRegister::HL);
        let value = bus.read(addr);
        let result = self.reset_bit_value(bit, value);
        bus.write(addr, result);
        16
    }

    #[inline(always)]
    fn set_bit_value(&mut self, bit: u8, value: u8) -> u8 {
        value | (1 << bit)
    }

    #[inline(always)]
    fn set_bit(&mut self, bit: u8, reg: Register) -> usize {
        let value = self.register(reg);
        let result = self.set_bit_value(bit, value);
        self.set_register(reg, result);
        8
    }

    #[inline(always)]
    fn set_bit_hl_indirect<B: Bus>(&mut self, bus: &mut B, bit: u8) -> usize {
        let addr = self.wide_register(WideRegister::HL);
        let value = bus.read(addr);
        let result = self.set_bit_value(bit, value);
        bus.write(addr, result);
        16
    }

    fn cb<B: Bus>(&mut self, bus: &mut B) -> usize {
        let opcode = self.fetch(bus);
        match opcode {
            0x00 => self.rlc(Register::B),
            0x01 => self.rlc(Register::C),
            0x02 => self.rlc(Register::D),
            0x03 => self.rlc(Register::E),
            0x04 => self.rlc(Register::H),
            0x05 => self.rlc(Register::L),
            0x06 => self.rlc_hl_indirect(bus),
            0x07 => self.rlc(Register::A),
            0x08 => self.rrc(Register::B),
            0x09 => self.rrc(Register::C),
            0x0A => self.rrc(Register::D),
            0x0B => self.rrc(Register::E),
            0x0C => self.rrc(Register::H),
            0x0D => self.rrc(Register::L),
            0x0E => self.rrc_hl_indirect(bus),
            0x0F => self.rrc(Register::A),

            0x10 => self.rl(Register::B),
            0x11 => self.rl(Register::C),
            0x12 => self.rl(Register::D),
            0x13 => self.rl(Register::E),
            0x14 => self.rl(Register::H),
            0x15 => self.rl(Register::L),
            0x16 => self.rl_hl_indirect(bus),
            0x17 => self.rl(Register::A),
            0x18 => self.rr(Register::B),
            0x19 => self.rr(Register::C),
            0x1A => self.rr(Register::D),
            0x1B => self.rr(Register::E),
            0x1C => self.rr(Register::H),
            0x1D => self.rr(Register::L),
            0x1E => self.rr_hl_indirect(bus),
            0x1F => self.rr(Register::A),

            0x20 => self.sla(Register::B),
            0x21 => self.sla(Register::C),
            0x22 => self.sla(Register::D),
            0x23 => self.sla(Register::E),
            0x24 => self.sla(Register::H),
            0x25 => self.sla(Register::L),
            0x26 => self.sla_hl_indirect(bus),
            0x27 => self.sla(Register::A),
            0x28 => self.sra(Register::B),
            0x29 => self.sra(Register::C),
            0x2A => self.sra(Register::D),
            0x2B => self.sra(Register::E),
            0x2C => self.sra(Register::H),
            0x2D => self.sra(Register::L),
            0x2E => self.sra_hl_indirect(bus),
            0x2F => self.sra(Register::A),

            0x30 => self.swap(Register::B),
            0x31 => self.swap(Register::C),
            0x32 => self.swap(Register::D),
            0x33 => self.swap(Register::E),
            0x34 => self.swap(Register::H),
            0x35 => self.swap(Register::L),
            0x36 => self.swap_hl_indirect(bus),
            0x37 => self.swap(Register::A),
            0x38 => self.srl(Register::B),
            0x39 => self.srl(Register::C),
            0x3A => self.srl(Register::D),
            0x3B => self.srl(Register::E),
            0x3C => self.srl(Register::H),
            0x3D => self.srl(Register::L),
            0x3E => self.srl_hl_indirect(bus),
            0x3F => self.srl(Register::A),

            0x40 => self.bit(0x00, Register::B),
            0x41 => self.bit(0x00, Register::C),
            0x42 => self.bit(0x00, Register::D),
            0x43 => self.bit(0x00, Register::E),
            0x44 => self.bit(0x00, Register::H),
            0x45 => self.bit(0x00, Register::L),
            0x46 => self.bit_hl_indirect(bus, 0x00),
            0x47 => self.bit(0x00, Register::A),
            0x48 => self.bit(0x01, Register::B),
            0x49 => self.bit(0x01, Register::C),
            0x4A => self.bit(0x01, Register::D),
            0x4B => self.bit(0x01, Register::E),
            0x4C => self.bit(0x01, Register::H),
            0x4D => self.bit(0x01, Register::L),
            0x4E => self.bit_hl_indirect(bus, 0x01),
            0x4F => self.bit(0x01, Register::A),

            0x50 => self.bit(0x02, Register::B),
            0x51 => self.bit(0x02, Register::C),
            0x52 => self.bit(0x02, Register::D),
            0x53 => self.bit(0x02, Register::E),
            0x54 => self.bit(0x02, Register::H),
            0x55 => self.bit(0x02, Register::L),
            0x56 => self.bit_hl_indirect(bus, 0x02),
            0x57 => self.bit(0x02, Register::A),
            0x58 => self.bit(0x03, Register::B),
            0x59 => self.bit(0x03, Register::C),
            0x5A => self.bit(0x03, Register::D),
            0x5B => self.bit(0x03, Register::E),
            0x5C => self.bit(0x03, Register::H),
            0x5D => self.bit(0x03, Register::L),
            0x5E => self.bit_hl_indirect(bus, 0x03),
            0x5F => self.bit(0x03, Register::A),

            0x60 => self.bit(0x04, Register::B),
            0x61 => self.bit(0x04, Register::C),
            0x62 => self.bit(0x04, Register::D),
            0x63 => self.bit(0x04, Register::E),
            0x64 => self.bit(0x04, Register::H),
            0x65 => self.bit(0x04, Register::L),
            0x66 => self.bit_hl_indirect(bus, 0x04),
            0x67 => self.bit(0x04, Register::A),
            0x68 => self.bit(0x05, Register::B),
            0x69 => self.bit(0x05, Register::C),
            0x6A => self.bit(0x05, Register::D),
            0x6B => self.bit(0x05, Register::E),
            0x6C => self.bit(0x05, Register::H),
            0x6D => self.bit(0x05, Register::L),
            0x6E => self.bit_hl_indirect(bus, 0x05),
            0x6F => self.bit(0x05, Register::A),

            0x70 => self.bit(0x06, Register::B),
            0x71 => self.bit(0x06, Register::C),
            0x72 => self.bit(0x06, Register::D),
            0x73 => self.bit(0x06, Register::E),
            0x74 => self.bit(0x06, Register::H),
            0x75 => self.bit(0x06, Register::L),
            0x76 => self.bit_hl_indirect(bus, 0x06),
            0x77 => self.bit(0x06, Register::A),
            0x78 => self.bit(0x07, Register::B),
            0x79 => self.bit(0x07, Register::C),
            0x7A => self.bit(0x07, Register::D),
            0x7B => self.bit(0x07, Register::E),
            0x7C => self.bit(0x07, Register::H),
            0x7D => self.bit(0x07, Register::L),
            0x7E => self.bit_hl_indirect(bus, 0x07),
            0x7F => self.bit(0x07, Register::A),

            0x80 => self.reset_bit(0x00, Register::B),
            0x81 => self.reset_bit(0x00, Register::C),
            0x82 => self.reset_bit(0x00, Register::D),
            0x83 => self.reset_bit(0x00, Register::E),
            0x84 => self.reset_bit(0x00, Register::H),
            0x85 => self.reset_bit(0x00, Register::L),
            0x86 => self.reset_bit_hl_indirect(bus, 0x00),
            0x87 => self.reset_bit(0x00, Register::A),
            0x88 => self.reset_bit(0x01, Register::B),
            0x89 => self.reset_bit(0x01, Register::C),
            0x8A => self.reset_bit(0x01, Register::D),
            0x8B => self.reset_bit(0x01, Register::E),
            0x8C => self.reset_bit(0x01, Register::H),
            0x8D => self.reset_bit(0x01, Register::L),
            0x8E => self.reset_bit_hl_indirect(bus, 0x01),
            0x8F => self.reset_bit(0x01, Register::A),

            0x90 => self.reset_bit(0x02, Register::B),
            0x91 => self.reset_bit(0x02, Register::C),
            0x92 => self.reset_bit(0x02, Register::D),
            0x93 => self.reset_bit(0x02, Register::E),
            0x94 => self.reset_bit(0x02, Register::H),
            0x95 => self.reset_bit(0x02, Register::L),
            0x96 => self.reset_bit_hl_indirect(bus, 0x02),
            0x97 => self.reset_bit(0x02, Register::A),
            0x98 => self.reset_bit(0x03, Register::B),
            0x99 => self.reset_bit(0x03, Register::C),
            0x9A => self.reset_bit(0x03, Register::D),
            0x9B => self.reset_bit(0x03, Register::E),
            0x9C => self.reset_bit(0x03, Register::H),
            0x9D => self.reset_bit(0x03, Register::L),
            0x9E => self.reset_bit_hl_indirect(bus, 0x03),
            0x9F => self.reset_bit(0x03, Register::A),

            0xA0 => self.reset_bit(0x04, Register::B),
            0xA1 => self.reset_bit(0x04, Register::C),
            0xA2 => self.reset_bit(0x04, Register::D),
            0xA3 => self.reset_bit(0x04, Register::E),
            0xA4 => self.reset_bit(0x04, Register::H),
            0xA5 => self.reset_bit(0x04, Register::L),
            0xA6 => self.reset_bit_hl_indirect(bus, 0x04),
            0xA7 => self.reset_bit(0x04, Register::A),
            0xA8 => self.reset_bit(0x05, Register::B),
            0xA9 => self.reset_bit(0x05, Register::C),
            0xAA => self.reset_bit(0x05, Register::D),
            0xAB => self.reset_bit(0x05, Register::E),
            0xAC => self.reset_bit(0x05, Register::H),
            0xAD => self.reset_bit(0x05, Register::L),
            0xAE => self.reset_bit_hl_indirect(bus, 0x05),
            0xAF => self.reset_bit(0x05, Register::A),

            0xB0 => self.reset_bit(0x06, Register::B),
            0xB1 => self.reset_bit(0x06, Register::C),
            0xB2 => self.reset_bit(0x06, Register::D),
            0xB3 => self.reset_bit(0x06, Register::E),
            0xB4 => self.reset_bit(0x06, Register::H),
            0xB5 => self.reset_bit(0x06, Register::L),
            0xB6 => self.reset_bit_hl_indirect(bus, 0x06),
            0xB7 => self.reset_bit(0x06, Register::A),
            0xB8 => self.reset_bit(0x07, Register::B),
            0xB9 => self.reset_bit(0x07, Register::C),
            0xBA => self.reset_bit(0x07, Register::D),
            0xBB => self.reset_bit(0x07, Register::E),
            0xBC => self.reset_bit(0x07, Register::H),
            0xBD => self.reset_bit(0x07, Register::L),
            0xBE => self.reset_bit_hl_indirect(bus, 0x07),
            0xBF => self.reset_bit(0x07, Register::A),

            0xC0 => self.set_bit(0x00, Register::B),
            0xC1 => self.set_bit(0x00, Register::C),
            0xC2 => self.set_bit(0x00, Register::D),
            0xC3 => self.set_bit(0x00, Register::E),
            0xC4 => self.set_bit(0x00, Register::H),
            0xC5 => self.set_bit(0x00, Register::L),
            0xC6 => self.set_bit_hl_indirect(bus, 0x00),
            0xC7 => self.set_bit(0x00, Register::A),
            0xC8 => self.set_bit(0x01, Register::B),
            0xC9 => self.set_bit(0x01, Register::C),
            0xCA => self.set_bit(0x01, Register::D),
            0xCB => self.set_bit(0x01, Register::E),
            0xCC => self.set_bit(0x01, Register::H),
            0xCD => self.set_bit(0x01, Register::L),
            0xCE => self.set_bit_hl_indirect(bus, 0x01),
            0xCF => self.set_bit(0x01, Register::A),

            0xD0 => self.set_bit(0x02, Register::B),
            0xD1 => self.set_bit(0x02, Register::C),
            0xD2 => self.set_bit(0x02, Register::D),
            0xD3 => self.set_bit(0x02, Register::E),
            0xD4 => self.set_bit(0x02, Register::H),
            0xD5 => self.set_bit(0x02, Register::L),
            0xD6 => self.set_bit_hl_indirect(bus, 0x02),
            0xD7 => self.set_bit(0x02, Register::A),
            0xD8 => self.set_bit(0x03, Register::B),
            0xD9 => self.set_bit(0x03, Register::C),
            0xDA => self.set_bit(0x03, Register::D),
            0xDB => self.set_bit(0x03, Register::E),
            0xDC => self.set_bit(0x03, Register::H),
            0xDD => self.set_bit(0x03, Register::L),
            0xDE => self.set_bit_hl_indirect(bus, 0x03),
            0xDF => self.set_bit(0x03, Register::A),

            0xE0 => self.set_bit(0x04, Register::B),
            0xE1 => self.set_bit(0x04, Register::C),
            0xE2 => self.set_bit(0x04, Register::D),
            0xE3 => self.set_bit(0x04, Register::E),
            0xE4 => self.set_bit(0x04, Register::H),
            0xE5 => self.set_bit(0x04, Register::L),
            0xE6 => self.set_bit_hl_indirect(bus, 0x04),
            0xE7 => self.set_bit(0x04, Register::A),
            0xE8 => self.set_bit(0x05, Register::B),
            0xE9 => self.set_bit(0x05, Register::C),
            0xEA => self.set_bit(0x05, Register::D),
            0xEB => self.set_bit(0x05, Register::E),
            0xEC => self.set_bit(0x05, Register::H),
            0xED => self.set_bit(0x05, Register::L),
            0xEE => self.set_bit_hl_indirect(bus, 0x05),
            0xEF => self.set_bit(0x05, Register::A),

            0xF0 => self.set_bit(0x06, Register::B),
            0xF1 => self.set_bit(0x06, Register::C),
            0xF2 => self.set_bit(0x06, Register::D),
            0xF3 => self.set_bit(0x06, Register::E),
            0xF4 => self.set_bit(0x06, Register::H),
            0xF5 => self.set_bit(0x06, Register::L),
            0xF6 => self.set_bit_hl_indirect(bus, 0x06),
            0xF7 => self.set_bit(0x06, Register::A),
            0xF8 => self.set_bit(0x07, Register::B),
            0xF9 => self.set_bit(0x07, Register::C),
            0xFA => self.set_bit(0x07, Register::D),
            0xFB => self.set_bit(0x07, Register::E),
            0xFC => self.set_bit(0x07, Register::H),
            0xFD => self.set_bit(0x07, Register::L),
            0xFE => self.set_bit_hl_indirect(bus, 0x07),
            0xFF => self.set_bit(0x07, Register::A),
        }
    }
}

impl<B: Bus> BusDevice<B> for Cpu {
    fn reset(&mut self, _bus: &mut B) {
        self.pc = 0x0000;
        self.irq = false;
        self.ime = false;
        self.stopped = false;
        self.halted = false;
    }

    fn tick(&mut self, bus: &mut B) -> usize {
        if self.halted {
            // TODO: exit halt state
            return 4;
        }
        // handle interrupts
        if self.irq && self.ime {
            let iflags = bus.read(Port::IF);
            let imasked = bus.read(Port::IE) & iflags;
            if imasked != 0 {
                if (imasked & 0x01) != 0x00 {
                    self.rst(bus, 0x0040);
                    bus.write(Port::IF, iflags ^ 0x01);
                } else if (imasked & 0x02) != 0x00 {
                    self.rst(bus, 0x0048);
                    bus.write(Port::IF, iflags ^ 0x02);
                } else if (imasked & 0x04) != 0x00 {
                    self.rst(bus, 0x0050);
                    bus.write(Port::IF, iflags ^ 0x04);
                } else if (imasked & 0x08) != 0x00 {
                    self.rst(bus, 0x0058);
                    bus.write(Port::IF, iflags ^ 0x08);
                } else if (imasked & 0x10) != 0x00 {
                    self.rst(bus, 0x0060);
                    bus.write(Port::IF, iflags ^ 0x10);
                }
                self.ime = false;
                return 20;
            }
        }
        let opcode = self.fetch(bus);
        match opcode {
            0x00 => self.nop(),
            0x01 => self.load_wide_immediate(bus, WideRegister::BC),
            0x02 => self.store_register_indirect(bus, WideRegister::BC, Register::A),
            0x03 => self.inc_wide(WideRegister::BC),
            0x04 => self.inc(Register::B),
            0x05 => self.dec(Register::B),
            0x06 => self.load_immediate(bus, Register::B),
            0x07 => self.rlca(),
            0x08 => self.write_stack_immediate(bus),
            0x09 => self.add_wide(WideRegister::BC),
            0x0A => self.load_register_indirect(bus, WideRegister::BC, Register::A),
            0x0B => self.dec_wide(WideRegister::BC),
            0x0C => self.inc(Register::C),
            0x0D => self.dec(Register::C),
            0x0E => self.load_immediate(bus, Register::C),
            0x0F => self.rrca(),

            0x10 => self.stop(bus),
            0x11 => self.load_wide_immediate(bus, WideRegister::DE),
            0x12 => self.store_register_indirect(bus, WideRegister::DE, Register::A),
            0x13 => self.inc_wide(WideRegister::DE),
            0x14 => self.inc(Register::D),
            0x15 => self.dec(Register::D),
            0x16 => self.load_immediate(bus, Register::D),
            0x17 => self.rla(),
            0x18 => self.jr(bus),
            0x19 => self.add_wide(WideRegister::DE),
            0x1A => self.load_register_indirect(bus, WideRegister::DE, Register::A),
            0x1B => self.dec_wide(WideRegister::DE),
            0x1C => self.inc(Register::E),
            0x1D => self.dec(Register::E),
            0x1E => self.load_immediate(bus, Register::E),
            0x1F => self.rra(),

            0x20 => self.jr_condition(bus, Condition::NotZero),
            0x21 => self.load_wide_immediate(bus, WideRegister::HL),
            0x22 => self.store_a_hli_indirect(bus),
            0x23 => self.inc_wide(WideRegister::HL),
            0x24 => self.inc(Register::H),
            0x25 => self.dec(Register::H),
            0x26 => self.load_immediate(bus, Register::H),
            0x27 => self.daa(),
            0x28 => self.jr_condition(bus, Condition::Zero),
            0x29 => self.add_wide(WideRegister::HL),
            0x2A => self.load_a_hli_indirect(bus),
            0x2B => self.dec_wide(WideRegister::HL),
            0x2C => self.inc(Register::L),
            0x2D => self.dec(Register::L),
            0x2E => self.load_immediate(bus, Register::L),
            0x2F => self.cpl(),

            0x30 => self.jr_condition(bus, Condition::NotCarry),
            0x31 => self.load_wide_immediate(bus, WideRegister::SP),
            0x32 => self.store_a_hld_indirect(bus),
            0x33 => self.inc_wide(WideRegister::SP),
            0x34 => self.inc_hl_indirect(bus),
            0x35 => self.dec_hl_indirect(bus),
            0x36 => self.store_immediate_hl_indirect(bus),
            0x37 => self.scf(),
            0x38 => self.jr_condition(bus, Condition::Carry),
            0x39 => self.add_wide(WideRegister::SP),
            0x3A => self.load_a_hld_indirect(bus),
            0x3B => self.dec_wide(WideRegister::SP),
            0x3C => self.inc(Register::A),
            0x3D => self.dec(Register::A),
            0x3E => self.load_immediate(bus, Register::A),
            0x3F => self.ccf(),

            0x40 => self.copy(Register::B, Register::B),
            0x41 => self.copy(Register::B, Register::C),
            0x42 => self.copy(Register::B, Register::D),
            0x43 => self.copy(Register::B, Register::E),
            0x44 => self.copy(Register::B, Register::H),
            0x45 => self.copy(Register::B, Register::L),
            0x46 => self.load_register_indirect(bus, WideRegister::HL, Register::B),
            0x47 => self.copy(Register::B, Register::A),
            0x48 => self.copy(Register::C, Register::B),
            0x49 => self.copy(Register::C, Register::C),
            0x4A => self.copy(Register::C, Register::D),
            0x4B => self.copy(Register::C, Register::E),
            0x4C => self.copy(Register::C, Register::H),
            0x4D => self.copy(Register::C, Register::L),
            0x4E => self.load_register_indirect(bus, WideRegister::HL, Register::C),
            0x4F => self.copy(Register::C, Register::A),

            0x50 => self.copy(Register::D, Register::B),
            0x51 => self.copy(Register::D, Register::C),
            0x52 => self.copy(Register::D, Register::D),
            0x53 => self.copy(Register::D, Register::E),
            0x54 => self.copy(Register::D, Register::H),
            0x55 => self.copy(Register::D, Register::L),
            0x56 => self.load_register_indirect(bus, WideRegister::HL, Register::D),
            0x57 => self.copy(Register::D, Register::A),
            0x58 => self.copy(Register::E, Register::B),
            0x59 => self.copy(Register::E, Register::C),
            0x5A => self.copy(Register::E, Register::D),
            0x5B => self.copy(Register::E, Register::E),
            0x5C => self.copy(Register::E, Register::H),
            0x5D => self.copy(Register::E, Register::L),
            0x5E => self.load_register_indirect(bus, WideRegister::HL, Register::E),
            0x5F => self.copy(Register::E, Register::A),

            0x60 => self.copy(Register::H, Register::B),
            0x61 => self.copy(Register::H, Register::C),
            0x62 => self.copy(Register::H, Register::D),
            0x63 => self.copy(Register::H, Register::E),
            0x64 => self.copy(Register::H, Register::H),
            0x65 => self.copy(Register::H, Register::L),
            0x66 => self.load_register_indirect(bus, WideRegister::HL, Register::H),
            0x67 => self.copy(Register::H, Register::A),
            0x68 => self.copy(Register::L, Register::B),
            0x69 => self.copy(Register::L, Register::C),
            0x6A => self.copy(Register::L, Register::D),
            0x6B => self.copy(Register::L, Register::E),
            0x6C => self.copy(Register::L, Register::H),
            0x6D => self.copy(Register::L, Register::L),
            0x6E => self.load_register_indirect(bus, WideRegister::HL, Register::L),
            0x6F => self.copy(Register::L, Register::A),

            0x70 => self.store_register_indirect(bus, WideRegister::HL, Register::B),
            0x71 => self.store_register_indirect(bus, WideRegister::HL, Register::C),
            0x72 => self.store_register_indirect(bus, WideRegister::HL, Register::D),
            0x73 => self.store_register_indirect(bus, WideRegister::HL, Register::E),
            0x74 => self.store_register_indirect(bus, WideRegister::HL, Register::H),
            0x75 => self.store_register_indirect(bus, WideRegister::HL, Register::L),
            0x76 => self.halt(),
            0x77 => self.store_register_indirect(bus, WideRegister::HL, Register::A),
            0x78 => self.copy(Register::A, Register::B),
            0x79 => self.copy(Register::A, Register::C),
            0x7A => self.copy(Register::A, Register::D),
            0x7B => self.copy(Register::A, Register::E),
            0x7C => self.copy(Register::A, Register::H),
            0x7D => self.copy(Register::A, Register::L),
            0x7E => self.load_register_indirect(bus, WideRegister::HL, Register::A),
            0x7F => self.copy(Register::A, Register::A),

            0x80 => self.add(Register::B),
            0x81 => self.add(Register::C),
            0x82 => self.add(Register::D),
            0x83 => self.add(Register::E),
            0x84 => self.add(Register::H),
            0x85 => self.add(Register::L),
            0x86 => self.add_hl_indirect(bus),
            0x87 => self.add(Register::A),
            0x88 => self.add_carry(Register::B),
            0x89 => self.add_carry(Register::C),
            0x8A => self.add_carry(Register::D),
            0x8B => self.add_carry(Register::E),
            0x8C => self.add_carry(Register::H),
            0x8D => self.add_carry(Register::L),
            0x8E => self.add_carry_hl_indirect(bus),
            0x8F => self.add_carry(Register::A),

            0x90 => self.sub(Register::B),
            0x91 => self.sub(Register::C),
            0x92 => self.sub(Register::D),
            0x93 => self.sub(Register::E),
            0x94 => self.sub(Register::H),
            0x95 => self.sub(Register::L),
            0x96 => self.sub_hl_indirect(bus),
            0x97 => self.sub(Register::A),
            0x98 => self.sub_carry(Register::B),
            0x99 => self.sub_carry(Register::C),
            0x9A => self.sub_carry(Register::D),
            0x9B => self.sub_carry(Register::E),
            0x9C => self.sub_carry(Register::H),
            0x9D => self.sub_carry(Register::L),
            0x9E => self.sub_carry_hl_indirect(bus),
            0x9F => self.sub_carry(Register::A),

            0xA0 => self.and(Register::B),
            0xA1 => self.and(Register::C),
            0xA2 => self.and(Register::D),
            0xA3 => self.and(Register::E),
            0xA4 => self.and(Register::H),
            0xA5 => self.and(Register::L),
            0xA6 => self.and_hl_indirect(bus),
            0xA7 => self.and(Register::A),
            0xA8 => self.xor(Register::B),
            0xA9 => self.xor(Register::C),
            0xAA => self.xor(Register::D),
            0xAB => self.xor(Register::E),
            0xAC => self.xor(Register::H),
            0xAD => self.xor(Register::L),
            0xAE => self.xor_hl_indirect(bus),
            0xAF => self.xor(Register::A),

            0xB0 => self.or(Register::B),
            0xB1 => self.or(Register::C),
            0xB2 => self.or(Register::D),
            0xB3 => self.or(Register::E),
            0xB4 => self.or(Register::H),
            0xB5 => self.or(Register::L),
            0xB6 => self.or_hl_indirect(bus),
            0xB7 => self.or(Register::A),
            0xB8 => self.compare(Register::B),
            0xB9 => self.compare(Register::C),
            0xBA => self.compare(Register::D),
            0xBB => self.compare(Register::E),
            0xBC => self.compare(Register::H),
            0xBD => self.compare(Register::L),
            0xBE => self.compare_hl_indirect(bus),
            0xBF => self.compare(Register::A),

            0xC0 => self.ret_condition(bus, Condition::NotZero),
            0xC1 => self.pop(bus, WideRegister::BC),
            0xC2 => self.jmp_condition(bus, Condition::NotZero),
            0xC3 => self.jmp(bus),
            0xC4 => self.call_condition(bus, Condition::NotZero),
            0xC5 => self.push(bus, WideRegister::BC),
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
            0xD1 => self.pop(bus, WideRegister::DE),
            0xD2 => self.jmp_condition(bus, Condition::NotCarry),
            0xD3 => 4,
            0xD4 => self.call_condition(bus, Condition::NotCarry),
            0xD5 => self.push(bus, WideRegister::DE),
            0xD6 => self.sub_immediate(bus),
            0xD7 => self.rst(bus, 0x0010),
            0xD8 => self.ret_condition(bus, Condition::Carry),
            0xD9 => self.reti(bus),
            0xDA => self.jmp_condition(bus, Condition::Carry),
            0xDB => 4,
            0xDC => self.call_condition(bus, Condition::Carry),
            0xDD => 4,
            0xDE => self.sub_carry_immediate(bus),
            0xDF => self.rst(bus, 0x0018),

            0xE0 => self.store_high_indirect(bus),
            0xE1 => self.pop(bus, WideRegister::HL),
            0xE2 => self.store_high_c_indirect(bus),
            0xE3 => 4,
            0xE4 => 4,
            0xE5 => self.push(bus, WideRegister::HL),
            0xE6 => self.and_immediate(bus),
            0xE7 => self.rst(bus, 0x0020),
            0xE8 => self.add_sp(bus),
            0xE9 => self.jmp_hl(),
            0xEA => self.store_indirect(bus),
            0xEB => 4,
            0xEC => 4,
            0xED => 4,
            0xEE => self.xor_immediate(bus),
            0xEF => self.rst(bus, 0x0028),

            0xF0 => self.load_high_indirect(bus),
            0xF1 => self.pop(bus, WideRegister::AF),
            0xF2 => self.load_high_c_indirect(bus),
            0xF3 => self.di(),
            0xF4 => 4,
            0xF5 => self.push(bus, WideRegister::AF),
            0xF6 => self.or_immediate(bus),
            0xF7 => self.rst(bus, 0x0030),
            0xF8 => self.load_sp_indirect(bus),
            0xF9 => self.copy_wide(WideRegister::SP, WideRegister::HL),
            0xFA => self.load_indirect(bus),
            0xFB => self.ei(),
            0xFC => 4,
            0xFD => 4,
            0xFE => self.compare_immediate(bus),
            0xFF => self.rst(bus, 0x0038),
        }
    }
}
