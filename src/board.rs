use std::collections::VecDeque;

use crate::cpu::{StepResult, XtensaCpu};
use crate::modules::{ModuleKind, PeripheralModule};

pub const GPIO_COUNT: usize = 8;
pub const BUTTON_GPIO: usize = 4;
pub const BASE_CLOCK_MHZ: u32 = 240;
pub const FIRMWARE_NAME: &str = "blink-demo / virtual USB-UART";

pub const GPIO_NAMES: [&str; GPIO_COUNT] = [
    "LED_BUILTIN",
    "UART0_TX",
    "STATUS_LED",
    "GPIO3",
    "BOOT_BUTTON",
    "GPIO5",
    "GPIO6",
    "GPIO7",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PinMode {
    Input,
    Output,
    Alternate,
}

pub const GPIO_MODES: [PinMode; GPIO_COUNT] = [
    PinMode::Output,
    PinMode::Alternate,
    PinMode::Output,
    PinMode::Input,
    PinMode::Input,
    PinMode::Input,
    PinMode::Input,
    PinMode::Input,
];

#[derive(Clone, Copy, Debug, Default)]
pub struct RunReport {
    pub cycles: u64,
    pub instructions: u64,
    pub uart_bytes: u64,
    pub halted: bool,
}

pub struct Esp32Board {
    pub cpu: XtensaCpu,
    input_mask: u32,
    serial_lines: VecDeque<String>,
    serial_partial: String,
    pub modules: Vec<PeripheralModule>,
    next_module_id: u32,
    pub boot_count: u32,
}

impl Esp32Board {
    pub fn new() -> Self {
        let mut board = Self {
            cpu: XtensaCpu::new(),
            input_mask: 0,
            serial_lines: VecDeque::new(),
            serial_partial: String::new(),
            modules: Vec::new(),
            next_module_id: 1,
            boot_count: 0,
        };
        board.boot_count = 1;
        board.write_boot_log();
        board
    }

    pub fn reset(&mut self) {
        self.cpu.reset();
        self.input_mask = 0;
        for module in &mut self.modules {
            module.reset();
        }
        self.serial_lines.clear();
        self.serial_partial.clear();
        self.boot_count = self.boot_count.saturating_add(1);
        self.write_boot_log();
    }

    pub fn run_for_cycles(&mut self, budget: u64) -> RunReport {
        let mut report = RunReport::default();

        while report.cycles < budget {
            let step = self.cpu.step(self.input_mask);
            if step.cycles == 0 {
                report.halted = true;
                break;
            }

            report.cycles = report.cycles.saturating_add(step.cycles as u64);
            report.instructions = report.instructions.saturating_add(1);
            if let Some(byte) = step.uart_byte {
                report.uart_bytes = report.uart_bytes.saturating_add(1);
                self.accept_uart_byte(byte);
            }
            if step.halted {
                report.halted = true;
                break;
            }
        }

        self.update_modules();
        report
    }

    pub fn step_instruction(&mut self) -> RunReport {
        let step = self.cpu.step(self.input_mask);
        if step.cycles == 0 {
            return RunReport {
                halted: true,
                ..RunReport::default()
            };
        }

        if let Some(byte) = step.uart_byte {
            self.accept_uart_byte(byte);
        }
        self.update_modules();

        RunReport {
            cycles: step.cycles as u64,
            instructions: 1,
            uart_bytes: u64::from(step.uart_byte.is_some()),
            halted: step.halted,
        }
    }

    pub fn attach_module(&mut self, kind: ModuleKind) -> u32 {
        let id = self.next_module_id;
        self.next_module_id = self.next_module_id.saturating_add(1);
        let module = PeripheralModule::new(id, kind);
        self.add_serial_line(format!(
            "[MODULE] attached: {} ({})",
            kind.label(),
            module.connection_label()
        ));
        self.modules.push(module);
        self.update_modules();
        id
    }

    pub fn detach_module(&mut self, id: u32) -> bool {
        let Some(index) = self.modules.iter().position(|module| module.id == id) else {
            return false;
        };
        let module = self.modules.remove(index);
        self.add_serial_line(format!("[MODULE] detached: {}", module.kind.label()));
        true
    }

    pub fn set_module_enabled(&mut self, id: u32, enabled: bool) -> bool {
        let label = {
            let Some(module) = self.modules.iter_mut().find(|module| module.id == id) else {
                return false;
            };
            module.enabled = enabled;
            module.kind.label().to_owned()
        };
        self.add_serial_line(format!(
            "[MODULE] {} {}",
            if enabled { "enabled" } else { "disabled" },
            label
        ));
        true
    }

    pub fn module(&self, id: u32) -> Option<&PeripheralModule> {
        self.modules.iter().find(|module| module.id == id)
    }

    fn update_modules(&mut self) {
        let total_cycles = self.cpu.total_cycles();
        let gpio_out = self.cpu.gpio_out();
        let gpio_in = self.input_mask;
        for module in &mut self.modules {
            module.tick(total_cycles, gpio_out, gpio_in);
        }
    }

    pub fn set_button_pressed(&mut self, pressed: bool) {
        let bit = 1_u32 << BUTTON_GPIO;
        let was_pressed = self.input_mask & bit != 0;
        if was_pressed == pressed {
            return;
        }

        if pressed {
            self.input_mask |= bit;
            self.add_serial_line("[GPIO] BOOT_BUTTON -> HIGH (pressed)");
        } else {
            self.input_mask &= !bit;
            self.add_serial_line("[GPIO] BOOT_BUTTON -> LOW (released)");
        }
        self.update_modules();
    }

    pub fn button_pressed(&self) -> bool {
        self.input_mask & (1_u32 << BUTTON_GPIO) != 0
    }

    pub fn gpio_input(&self) -> u32 {
        self.input_mask
    }

    pub fn gpio_level(&self, pin: usize) -> bool {
        if pin >= GPIO_COUNT {
            return false;
        }

        match GPIO_MODES[pin] {
            PinMode::Output | PinMode::Alternate => self.cpu.gpio_out() & (1_u32 << pin) != 0,
            PinMode::Input => self.input_mask & (1_u32 << pin) != 0,
        }
    }

    pub fn pin_name(pin: usize) -> &'static str {
        GPIO_NAMES.get(pin).copied().unwrap_or("GPIO")
    }

    pub fn pin_mode(pin: usize) -> PinMode {
        GPIO_MODES.get(pin).copied().unwrap_or(PinMode::Input)
    }

    pub fn serial_lines(&self) -> &VecDeque<String> {
        &self.serial_lines
    }

    pub fn serial_partial(&self) -> &str {
        &self.serial_partial
    }

    pub fn serial_text(&self) -> String {
        let mut text = self.serial_lines.iter().cloned().collect::<Vec<_>>().join("\n");
        if !self.serial_partial.is_empty() {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(&self.serial_partial);
        }
        text
    }

    pub fn clear_serial(&mut self) {
        self.serial_lines.clear();
        self.serial_partial.clear();
    }

    pub fn send_serial(&mut self, message: &str) {
        let message = message.trim();
        if !message.is_empty() {
            self.add_serial_line(format!("[HOST -> UART0] {}", message));
        }
    }

    pub fn last_step(&self) -> Option<StepResult> {
        self.cpu.last_step()
    }

    fn write_boot_log(&mut self) {
        self.add_serial_line("[BOOT] virtual USB-UART connected");
        self.add_serial_line("[BOOT] chip: ESP32 / dual-core Tensilica Xtensa LX6");
        self.add_serial_line("[BOOT] clock: 240 MHz base, 64 KiB SRAM mapped");
        self.add_serial_line(format!("[BOOT] firmware: {}", FIRMWARE_NAME));
        if !self.modules.is_empty() {
            self.add_serial_line(format!("[BOOT] peripheral modules: {} attached", self.modules.len()));
        }
    }

    fn accept_uart_byte(&mut self, byte: u8) {
        match byte {
            b'\n' => {
                let line = std::mem::take(&mut self.serial_partial);
                self.add_serial_line(line);
            }
            0x20..=0x7e | b'\t' => self.serial_partial.push(byte as char),
            _ => self.serial_partial.push('·'),
        }
    }

    fn add_serial_line(&mut self, line: impl Into<String>) {
        const MAX_LINES: usize = 160;
        self.serial_lines.push_back(line.into());
        while self.serial_lines.len() > MAX_LINES {
            self.serial_lines.pop_front();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Esp32Board, BUTTON_GPIO};
    use crate::modules::ModuleKind;

    #[test]
    fn board_exposes_a_virtual_boot_button() {
        let mut board = Esp32Board::new();
        assert!(!board.button_pressed());
        board.set_button_pressed(true);
        assert!(board.button_pressed());
        assert_ne!(board.gpio_input() & (1 << BUTTON_GPIO), 0);
    }

    #[test]
    fn uart_output_reaches_the_serial_monitor() {
        let mut board = Esp32Board::new();
        board.send_serial("ping");
        assert!(board.serial_text().contains("HOST -> UART0"));
    }

    #[test]
    fn modules_can_be_attached_and_detached() {
        let mut board = Esp32Board::new();
        let id = board.attach_module(ModuleKind::Oled128x64);
        assert_eq!(board.modules.len(), 1);
        assert!(board.module(id).is_some());
        assert!(board.serial_text().contains("attached: OLED"));
        assert!(board.detach_module(id));
        assert!(board.modules.is_empty());
    }
}
