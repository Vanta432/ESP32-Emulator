use std::fmt;

/// The address at which the demo firmware is displayed in the UI.
///
/// The emulator uses an instruction index internally, but showing a familiar
/// ESP32 IROM address makes the debugger easier to read.
pub const ROM_BASE: u32 = 0x4008_0000;
pub const REG_COUNT: usize = 16;
pub const SRAM_BYTES: usize = 64 * 1024;

/// A deliberately small, observable subset of the Xtensa LX6 instruction set.
///
/// This is a teaching core rather than a binary-compatible LX6 decoder. Every
/// instruction still has a cycle cost and interacts with the same kind of
/// registers, GPIO and SRAM that are useful when demonstrating an ESP32.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Instruction {
    Nop,
    LoadImm {
        rd: usize,
        imm: u32,
    },
    Addi {
        rd: usize,
        rs: usize,
        imm: i32,
    },
    Add {
        rd: usize,
        rs1: usize,
        rs2: usize,
    },
    Xor {
        rd: usize,
        rs1: usize,
        rs2: usize,
    },
    ReadGpio {
        rd: usize,
    },
    WriteGpio {
        rs: usize,
    },
    ToggleGpio {
        mask: u32,
    },
    Load8 {
        rd: usize,
        address: usize,
    },
    Store8 {
        rs: usize,
        address: usize,
    },
    BranchLt {
        rs1: usize,
        rs2: usize,
        target: usize,
    },
    BranchNz {
        rs: usize,
        target: usize,
    },
    Jump {
        target: usize,
    },
    Uart {
        byte: u8,
    },
    Halt,
}

impl Instruction {
    pub fn cycle_cost(self) -> u32 {
        match self {
            Self::Nop | Self::LoadImm { .. } | Self::Addi { .. } | Self::Add { .. } => 1,
            Self::Xor { .. } | Self::ReadGpio { .. } | Self::WriteGpio { .. } => 1,
            Self::ToggleGpio { .. } | Self::Load8 { .. } | Self::Store8 { .. } => 2,
            Self::BranchLt { .. } | Self::BranchNz { .. } | Self::Jump { .. } => 2,
            Self::Uart { .. } => 2,
            Self::Halt => 1,
        }
    }

    fn register(index: usize) -> String {
        format!("a{}", index.min(REG_COUNT - 1))
    }
}

impl fmt::Display for Instruction {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Nop => write!(formatter, "nop"),
            Self::LoadImm { rd, imm } => {
                write!(formatter, "movi {}, 0x{:08X}", Self::register(*rd), *imm)
            }
            Self::Addi { rd, rs, imm } => write!(
                formatter,
                "addi {}, {}, {}",
                Self::register(*rd),
                Self::register(*rs),
                *imm
            ),
            Self::Add { rd, rs1, rs2 } => write!(
                formatter,
                "add {}, {}, {}",
                Self::register(*rd),
                Self::register(*rs1),
                Self::register(*rs2)
            ),
            Self::Xor { rd, rs1, rs2 } => write!(
                formatter,
                "xor {}, {}, {}",
                Self::register(*rd),
                Self::register(*rs1),
                Self::register(*rs2)
            ),
            Self::ReadGpio { rd } => write!(formatter, "l32i {}, GPIO_IN", Self::register(*rd)),
            Self::WriteGpio { rs } => write!(formatter, "s32i {}, GPIO_OUT", Self::register(*rs)),
            Self::ToggleGpio { mask } => write!(formatter, "gpio_toggle 0x{:02X}", *mask),
            Self::Load8 { rd, address } => write!(
                formatter,
                "l8ui {}, [0x{:04X}]",
                Self::register(*rd),
                *address
            ),
            Self::Store8 { rs, address } => write!(
                formatter,
                "s8i {}, [0x{:04X}]",
                Self::register(*rs),
                *address
            ),
            Self::BranchLt { rs1, rs2, target } => write!(
                formatter,
                "blt {}, {}, 0x{:04X}",
                Self::register(*rs1),
                Self::register(*rs2),
                *target * 4
            ),
            Self::BranchNz { rs, target } => write!(
                formatter,
                "bnez {}, 0x{:04X}",
                Self::register(*rs),
                *target * 4
            ),
            Self::Jump { target } => write!(formatter, "j 0x{:04X}", *target * 4),
            Self::Uart { byte } => {
                let character = if byte.is_ascii_graphic() || *byte == b' ' {
                    *byte as char
                } else {
                    '.'
                };
                write!(formatter, "uart_tx 0x{:02X} '{}'", *byte, character)
            }
            Self::Halt => write!(formatter, "halt"),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct StepResult {
    pub pc_before: usize,
    pub pc_after: usize,
    pub instruction: Instruction,
    pub cycles: u32,
    pub uart_byte: Option<u8>,
    pub gpio_out: Option<u32>,
    pub halted: bool,
}

pub struct XtensaCpu {
    registers: [u32; REG_COUNT],
    pc: usize,
    ps: u32,
    gpio_out: u32,
    gpio_in: u32,
    total_cycles: u64,
    total_instructions: u64,
    halted: bool,
    rom: Vec<Instruction>,
    memory: Vec<u8>,
    last_step: Option<StepResult>,
}

impl XtensaCpu {
    pub fn new() -> Self {
        Self {
            registers: [0; REG_COUNT],
            pc: 0,
            ps: 0x0006_0000,
            gpio_out: 0,
            gpio_in: 0,
            total_cycles: 0,
            total_instructions: 0,
            halted: false,
            rom: demo_firmware(),
            memory: vec![0; SRAM_BYTES],
            last_step: None,
        }
    }

    pub fn reset(&mut self) {
        self.registers = [0; REG_COUNT];
        self.pc = 0;
        self.ps = 0x0006_0000;
        self.gpio_out = 0;
        self.gpio_in = 0;
        self.total_cycles = 0;
        self.total_instructions = 0;
        self.halted = false;
        self.memory.fill(0);
        self.last_step = None;
    }

    /// Execute one instruction while exposing the GPIO input bus to the core.
    pub fn step(&mut self, gpio_in: u32) -> StepResult {
        if self.halted || self.rom.is_empty() {
            return StepResult {
                pc_before: self.pc,
                pc_after: self.pc,
                instruction: Instruction::Halt,
                cycles: 0,
                uart_byte: None,
                gpio_out: None,
                halted: true,
            };
        }

        self.gpio_in = gpio_in;
        let pc_before = self.pc;
        let instruction = self.rom[self.pc];
        let mut next_pc = self.pc.saturating_add(1);
        let mut uart_byte = None;
        let mut gpio_changed = None;

        match instruction {
            Instruction::Nop => {}
            Instruction::LoadImm { rd, imm } => self.write_register(rd, imm),
            Instruction::Addi { rd, rs, imm } => {
                let value = self.register(rs).wrapping_add(imm as u32);
                self.write_register(rd, value);
            }
            Instruction::Add { rd, rs1, rs2 } => {
                let value = self.register(rs1).wrapping_add(self.register(rs2));
                self.write_register(rd, value);
            }
            Instruction::Xor { rd, rs1, rs2 } => {
                let value = self.register(rs1) ^ self.register(rs2);
                self.write_register(rd, value);
            }
            Instruction::ReadGpio { rd } => self.write_register(rd, self.gpio_in),
            Instruction::WriteGpio { rs } => {
                self.gpio_out = self.register(rs);
                gpio_changed = Some(self.gpio_out);
            }
            Instruction::ToggleGpio { mask } => {
                self.gpio_out ^= mask;
                gpio_changed = Some(self.gpio_out);
            }
            Instruction::Load8 { rd, address } => {
                let value = self.memory.get(address).copied().unwrap_or_default() as u32;
                self.write_register(rd, value);
            }
            Instruction::Store8 { rs, address } => {
                if let Some(byte) = self.memory.get_mut(address) {
                    *byte = self.register(rs) as u8;
                }
            }
            Instruction::BranchLt { rs1, rs2, target } => {
                if self.register(rs1) < self.register(rs2) {
                    next_pc = target;
                }
            }
            Instruction::BranchNz { rs, target } => {
                if self.register(rs) != 0 {
                    next_pc = target;
                }
            }
            Instruction::Jump { target } => next_pc = target,
            Instruction::Uart { byte } => uart_byte = Some(byte),
            Instruction::Halt => self.halted = true,
        }

        if next_pc >= self.rom.len() && !self.halted {
            self.halted = true;
        }

        self.pc = if self.rom.is_empty() {
            0
        } else {
            next_pc.min(self.rom.len() - 1)
        };

        let cycles = instruction.cycle_cost();
        self.total_cycles = self.total_cycles.saturating_add(cycles as u64);
        self.total_instructions = self.total_instructions.saturating_add(1);

        let result = StepResult {
            pc_before,
            pc_after: self.pc,
            instruction,
            cycles,
            uart_byte,
            gpio_out: gpio_changed,
            halted: self.halted,
        };
        self.last_step = Some(result);
        result
    }

    pub fn registers(&self) -> &[u32; REG_COUNT] {
        &self.registers
    }

    pub fn memory(&self) -> &[u8] {
        &self.memory
    }

    pub fn rom(&self) -> &[Instruction] {
        &self.rom
    }

    pub fn pc(&self) -> usize {
        self.pc
    }

    pub fn pc_address(&self) -> u32 {
        ROM_BASE.saturating_add((self.pc as u32).saturating_mul(4))
    }

    pub fn ps(&self) -> u32 {
        self.ps
    }

    pub fn gpio_out(&self) -> u32 {
        self.gpio_out
    }

    pub fn gpio_in(&self) -> u32 {
        self.gpio_in
    }

    pub fn total_cycles(&self) -> u64 {
        self.total_cycles
    }

    pub fn total_instructions(&self) -> u64 {
        self.total_instructions
    }

    pub fn halted(&self) -> bool {
        self.halted
    }

    pub fn last_step(&self) -> Option<StepResult> {
        self.last_step
    }

    pub fn current_instruction(&self) -> Instruction {
        self.rom.get(self.pc).copied().unwrap_or(Instruction::Halt)
    }

    fn register(&self, index: usize) -> u32 {
        self.registers[index.min(REG_COUNT - 1)]
    }

    fn write_register(&mut self, index: usize, value: u32) {
        self.registers[index.min(REG_COUNT - 1)] = value;
    }
}

fn demo_firmware() -> Vec<Instruction> {
    vec![
        // A tiny blink firmware: poll the input, count, write SRAM, and toggle
        // GPIO0 every 800 iterations. The UART instructions make the result
        // visible in the serial monitor without requiring physical hardware.
        Instruction::LoadImm { rd: 1, imm: 0 },
        Instruction::LoadImm { rd: 2, imm: 800 },
        Instruction::LoadImm { rd: 3, imm: 1 },
        Instruction::ReadGpio { rd: 4 },
        Instruction::Addi {
            rd: 1,
            rs: 1,
            imm: 1,
        },
        Instruction::Store8 {
            rs: 1,
            address: 0,
        },
        Instruction::BranchLt {
            rs1: 1,
            rs2: 2,
            target: 3,
        },
        Instruction::ToggleGpio { mask: 0x01 },
        Instruction::Uart { byte: b'L' },
        Instruction::Uart { byte: b'E' },
        Instruction::Uart { byte: b'D' },
        Instruction::Uart { byte: b' ' },
        Instruction::Uart { byte: b't' },
        Instruction::Uart { byte: b'o' },
        Instruction::Uart { byte: b'g' },
        Instruction::Uart { byte: b'g' },
        Instruction::Uart { byte: b'l' },
        Instruction::Uart { byte: b'e' },
        Instruction::Uart { byte: b'd' },
        Instruction::Uart { byte: b'\n' },
        Instruction::LoadImm { rd: 1, imm: 0 },
        Instruction::Jump { target: 3 },
    ]
}

#[cfg(test)]
mod tests {
    use super::{Instruction, XtensaCpu};

    #[test]
    fn demo_reads_gpio_and_writes_sram() {
        let mut cpu = XtensaCpu::new();
        for _ in 0..6 {
            cpu.step(1 << 4);
        }

        assert_eq!(cpu.registers()[4], 1 << 4);
        assert_eq!(cpu.memory()[0], 1);
        assert_eq!(cpu.total_instructions(), 6);
    }

    #[test]
    fn branch_and_jump_keep_the_demo_running() {
        let mut cpu = XtensaCpu::new();
        for _ in 0..10_000 {
            cpu.step(0);
        }

        assert!(!cpu.halted());
        assert!(cpu.total_cycles() > 10_000);
        assert_ne!(cpu.gpio_out() & 1, 0);
    }

    #[test]
    fn instruction_display_is_debugger_friendly() {
        let text = Instruction::LoadImm { rd: 1, imm: 42 }.to_string();
        assert!(text.contains("movi a1"));
        assert!(text.contains("0000002A"));
    }
}
