mod board;
mod cpu;
mod modules;

use std::time::{Duration, Instant};

use board::{Esp32Board, BASE_CLOCK_MHZ, BUTTON_GPIO, FIRMWARE_NAME, GPIO_COUNT};
use cpu::{ROM_BASE, SRAM_BYTES};
use modules::{ModuleKind, ModuleState, Rgb};
use eframe::egui::{self, Align2, Color32, FontId, Layout, Margin, Pos2, Rect, RichText, Sense, Stroke, Ui, Vec2};

const NORMAL_CYCLES_PER_SECOND: f64 = 240_000.0;
const MAX_FRAME_CYCLES: u64 = 300_000;
const MAX_TURBO: u32 = 64;

fn canvas() -> Color32 {
    Color32::from_rgb(10, 15, 24)
}

fn panel() -> Color32 {
    Color32::from_rgb(17, 25, 38)
}

fn panel_raised() -> Color32 {
    Color32::from_rgb(22, 32, 48)
}

fn border() -> Color32 {
    Color32::from_rgb(42, 57, 78)
}

fn text_primary() -> Color32 {
    Color32::from_rgb(235, 242, 250)
}

fn text_muted() -> Color32 {
    Color32::from_rgb(139, 157, 180)
}

fn cyan() -> Color32 {
    Color32::from_rgb(63, 210, 219)
}

fn green() -> Color32 {
    Color32::from_rgb(82, 220, 142)
}

fn amber() -> Color32 {
    Color32::from_rgb(255, 177, 72)
}

fn red() -> Color32 {
    Color32::from_rgb(255, 101, 112)
}

fn card() -> egui::Frame {
    egui::Frame::none()
        .fill(panel())
        .stroke(Stroke::new(1.0, border()))
        .rounding(egui::Rounding::same(12.0))
        .inner_margin(Margin::same(14.0))
}

fn small_card() -> egui::Frame {
    egui::Frame::none()
        .fill(panel_raised())
        .stroke(Stroke::new(1.0, border()))
        .rounding(egui::Rounding::same(9.0))
        .inner_margin(Margin::same(10.0))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum View {
    Overview,
    Modules,
    Cpu,
    Memory,
    Serial,
}

impl View {
    fn label(self) -> &'static str {
        match self {
            Self::Overview => "OVERVIEW",
            Self::Modules => "MODULES",
            Self::Cpu => "CPU / DEBUG",
            Self::Memory => "MEMORY",
            Self::Serial => "SERIAL MONITOR",
        }
    }
}

struct Esp32App {
    board: Esp32Board,
    view: View,
    running: bool,
    turbo_enabled: bool,
    turbo_multiplier: u32,
    module_to_add: ModuleKind,
    serial_input: String,
    last_tick: Instant,
    rate_epoch: Instant,
    rate_cycles: u64,
    rate_instructions: u64,
    cycles_per_second: f64,
    instructions_per_second: f64,
    style_applied: bool,
}

impl Esp32App {
    fn new(_creation_context: &eframe::CreationContext<'_>) -> Self {
        let now = Instant::now();
        Self {
            board: Esp32Board::new(),
            view: View::Overview,
            running: true,
            turbo_enabled: false,
            turbo_multiplier: 8,
            module_to_add: ModuleKind::Oled128x64,
            serial_input: String::new(),
            last_tick: now,
            rate_epoch: now,
            rate_cycles: 0,
            rate_instructions: 0,
            cycles_per_second: 0.0,
            instructions_per_second: 0.0,
            style_applied: false,
        }
    }

    fn effective_multiplier(&self) -> u32 {
        if self.turbo_enabled {
            self.turbo_multiplier
        } else {
            1
        }
    }

    fn effective_clock_mhz(&self) -> u32 {
        BASE_CLOCK_MHZ.saturating_mul(self.effective_multiplier())
    }

    fn tick_emulator(&mut self) {
        let now = Instant::now();
        let elapsed = now
            .duration_since(self.last_tick)
            .as_secs_f64()
            .clamp(0.0, 0.100);
        self.last_tick = now;

        if self.running && !self.board.cpu.halted() {
            let requested = (elapsed
                * NORMAL_CYCLES_PER_SECOND
                * f64::from(self.effective_multiplier())) as u64;
            let budget = requested.clamp(1, MAX_FRAME_CYCLES);
            let report = self.board.run_for_cycles(budget);
            self.rate_cycles = self.rate_cycles.saturating_add(report.cycles);
            self.rate_instructions = self.rate_instructions.saturating_add(report.instructions);
            if report.halted {
                self.running = false;
            }
        }

        let rate_elapsed = now.duration_since(self.rate_epoch).as_secs_f64();
        if rate_elapsed >= 0.5 {
            self.cycles_per_second = self.rate_cycles as f64 / rate_elapsed;
            self.instructions_per_second = self.rate_instructions as f64 / rate_elapsed;
            self.rate_cycles = 0;
            self.rate_instructions = 0;
            self.rate_epoch = now;
        }
    }

    fn reset(&mut self) {
        self.board.reset();
        self.running = true;
        self.last_tick = Instant::now();
        self.rate_epoch = self.last_tick;
        self.rate_cycles = 0;
        self.rate_instructions = 0;
        self.cycles_per_second = 0.0;
        self.instructions_per_second = 0.0;
    }

    fn step_once(&mut self) {
        self.running = false;
        let report = self.board.step_instruction();
        self.rate_cycles = self.rate_cycles.saturating_add(report.cycles);
        self.rate_instructions = self.rate_instructions.saturating_add(report.instructions);
    }

    fn draw_top_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("top_bar")
            .exact_height(72.0)
            .frame(egui::Frame::none().fill(canvas()).inner_margin(Margin::symmetric(20.0, 13.0)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("ESP32")
                            .size(22.0)
                            .strong()
                            .color(cyan()),
                    );
                    ui.label(
                        RichText::new("EMULATOR")
                            .size(22.0)
                            .strong()
                            .color(text_primary()),
                    );
                    ui.add_space(12.0);
                    ui.label(
                        RichText::new("Tensilica Xtensa LX6  /  virtual hardware bench")
                            .size(12.0)
                            .color(text_muted()),
                    );

                    ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                        let state_text = if self.running { "RUNNING" } else { "PAUSED" };
                        let state_color = if self.running { green() } else { amber() };
                        ui.label(
                            RichText::new(format!("● {}", state_text))
                                .size(12.0)
                                .strong()
                                .color(state_color),
                        );
                        ui.separator();
                        ui.label(
                            RichText::new(format!("{} MHz simulated", self.effective_clock_mhz()))
                                .size(12.0)
                                .color(text_primary()),
                        );
                        ui.separator();
                        ui.label(
                            RichText::new("USB-UART  /  CONNECTED")
                                .size(12.0)
                                .strong()
                                .color(green()),
                        );
                    });
                });
            });
    }

    fn draw_sidebar(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("control_panel")
            .resizable(false)
            .default_width(274.0)
            .width_range(250.0..=300.0)
            .frame(egui::Frame::none().fill(canvas()).inner_margin(Margin::same(16.0)))
            .show(ctx, |ui| {
                section_label(ui, "EXECUTION CONTROL");
                card().show(ui, |ui| {
                    let run_label = if self.running { "❚❚  PAUSE" } else { "▶  RUN" };
                    let run_color = if self.running { amber() } else { green() };
                    if ui
                        .add_sized(
                            [ui.available_width(), 38.0],
                            egui::Button::new(
                                RichText::new(run_label).strong().color(Color32::from_rgb(8, 15, 22)),
                            )
                            .fill(run_color),
                        )
                        .clicked()
                    {
                        self.running = !self.running;
                        self.last_tick = Instant::now();
                    }

                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui
                            .add_sized(
                                [ui.available_width() * 0.48, 31.0],
                                egui::Button::new(RichText::new("STEP").color(text_primary())),
                            )
                            .clicked()
                        {
                            self.step_once();
                        }
                        if ui
                            .add_sized(
                                [ui.available_width(), 31.0],
                                egui::Button::new(RichText::new("RESET").color(text_primary())),
                            )
                            .clicked()
                        {
                            self.reset();
                        }
                    });

                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("TURBO POWER").size(10.0).strong().color(text_muted()));
                        ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(
                                RichText::new(format!("×{}", self.turbo_multiplier))
                                    .strong()
                                    .color(if self.turbo_enabled { amber() } else { text_primary() }),
                            );
                        });
                    });
                    ui.add(
                        egui::Slider::new(&mut self.turbo_multiplier, 1..=MAX_TURBO)
                            .text("power vs ESP32"),
                    );
                    let turbo_label = if self.turbo_enabled {
                        format!("⚡  TURBO ACTIVE  ·  {}× faster", self.turbo_multiplier)
                    } else {
                        "⚡  ENABLE TURBO".to_owned()
                    };
                    let turbo_fill = if self.turbo_enabled {
                        amber()
                    } else {
                        Color32::from_rgb(71, 57, 36)
                    };
                    if ui
                        .add_sized(
                            [ui.available_width(), 34.0],
                            egui::Button::new(
                                RichText::new(turbo_label).strong().color(if self.turbo_enabled {
                                    Color32::from_rgb(37, 25, 8)
                                } else {
                                    amber()
                                }),
                            )
                            .fill(turbo_fill),
                        )
                        .clicked()
                    {
                        self.turbo_enabled = !self.turbo_enabled;
                        self.last_tick = Instant::now();
                    }
                    ui.add_space(5.0);
                    ui.label(
                        RichText::new(format!(
                            "1× = {} MHz · turbo = {} MHz",
                            BASE_CLOCK_MHZ,
                            BASE_CLOCK_MHZ.saturating_mul(self.turbo_multiplier)
                        ))
                        .size(10.0)
                        .color(text_muted()),
                    );
                });

                ui.add_space(14.0);
                section_label(ui, "VIRTUAL DEVICE");
                card().show(ui, |ui| {
                    key_value(ui, "MODEL", "ESP32-WROOM-32");
                    key_value(ui, "CORE", "Dual LX6 (simulated)");
                    key_value(ui, "FIRMWARE", FIRMWARE_NAME);
                    key_value(ui, "SRAM", "64 KiB mapped");
                    ui.add_space(7.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("●").color(green()));
                        ui.label(RichText::new("virtual USB-UART attached").size(11.0).color(text_primary()));
                    });
                });

                ui.add_space(14.0);
                section_label(ui, "PERIPHERAL MODULES");
                card().show(ui, |ui| {
                    egui::ComboBox::from_id_source("module_picker_sidebar")
                        .width(ui.available_width())
                        .selected_text(self.module_to_add.label())
                        .show_ui(ui, |ui| {
                            for kind in ModuleKind::ALL {
                                ui.selectable_value(&mut self.module_to_add, kind, kind.label());
                            }
                        });
                    ui.add_space(6.0);
                    if ui
                        .add_sized(
                            [ui.available_width(), 31.0],
                            egui::Button::new(RichText::new("＋  ATTACH MODULE").strong().color(text_primary()))
                                .fill(Color32::from_rgb(28, 66, 76)),
                        )
                        .clicked()
                    {
                        self.board.attach_module(self.module_to_add);
                        self.view = View::Modules;
                    }
                    ui.add_space(6.0);
                    if self.board.modules.is_empty() {
                        ui.label(RichText::new("No modules attached").size(10.0).color(text_muted()));
                    } else {
                        for module in self.board.modules.iter().take(3) {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("●").size(10.0).color(green()));
                                ui.label(RichText::new(module.kind.label()).size(10.0).color(text_primary()));
                            });
                        }
                        if self.board.modules.len() > 3 {
                            ui.label(
                                RichText::new(format!("+ {} more in MODULES", self.board.modules.len() - 3))
                                    .size(10.0)
                                    .color(text_muted()),
                            );
                        }
                    }
                });

                ui.add_space(14.0);
                section_label(ui, "GPIO INPUT");
                card().show(ui, |ui| {
                    let pressed = self.board.button_pressed();
                    let label = if pressed { "BOOT BUTTON  ·  HIGH" } else { "BOOT BUTTON  ·  LOW" };
                    let fill = if pressed {
                        Color32::from_rgb(48, 91, 76)
                    } else {
                        panel_raised()
                    };
                    if ui
                        .add_sized(
                            [ui.available_width(), 34.0],
                            egui::Button::new(RichText::new(label).color(text_primary())).fill(fill),
                        )
                        .clicked()
                    {
                        self.board.set_button_pressed(!pressed);
                    }
                    ui.label(
                        RichText::new(format!("GPIO{}  ·  click to change input bus", BUTTON_GPIO))
                            .size(10.0)
                            .color(text_muted()),
                    );
                });
            });
    }

    fn draw_status_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("status_bar")
            .exact_height(30.0)
            .frame(egui::Frame::none().fill(panel()).inner_margin(Margin::symmetric(18.0, 6.0)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("● ONLINE").size(10.0).strong().color(green()));
                    ui.separator();
                    ui.label(
                        RichText::new(format!(
                            "PC 0x{:08X}",
                            ROM_BASE + (self.board.cpu.pc() as u32 * 4)
                        ))
                        .monospace()
                        .size(10.0)
                        .color(text_muted()),
                    );
                    ui.separator();
                    ui.label(
                        RichText::new(format!(
                            "{} cycles/s  ·  {} instr/s",
                            compact_number(self.cycles_per_second),
                            compact_number(self.instructions_per_second)
                        ))
                        .monospace()
                        .size(10.0)
                        .color(text_muted()),
                    );
                    ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new("DEMO FIRMWARE  /  EDUCATIONAL SUBSET")
                                .size(10.0)
                                .color(text_muted()),
                        );
                    });
                });
            });
    }

    fn draw_main(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(canvas()).inner_margin(Margin::symmetric(18.0, 16.0)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    for view in [View::Overview, View::Modules, View::Cpu, View::Memory, View::Serial] {
                        let selected = self.view == view;
                        let button = egui::Button::new(
                            RichText::new(view.label())
                                .size(11.0)
                                .strong()
                                .color(if selected { cyan() } else { text_muted() }),
                        )
                        .fill(if selected { panel_raised() } else { canvas() })
                        .stroke(Stroke::new(1.0, if selected { cyan() } else { canvas() }));
                        if ui.add(button).clicked() {
                            self.view = view;
                        }
                    }
                });
                ui.add_space(12.0);

                match self.view {
                    View::Overview => self.draw_overview(ui),
                    View::Modules => self.draw_modules(ui),
                    View::Cpu => self.draw_cpu(ui),
                    View::Memory => self.draw_memory(ui),
                    View::Serial => self.draw_serial(ui),
                }
            });
    }

    fn draw_overview(&mut self, ui: &mut Ui) {
        card().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new("VIRTUAL ESP32 BOARD").size(11.0).strong().color(text_muted()));
                    ui.label(
                        RichText::new("A live, connected-looking hardware surface")
                            .size(12.0)
                            .color(text_primary()),
                    );
                });
                ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new("● POWERED  ·  3.3 V").size(10.0).strong().color(green()));
                });
            });
            ui.add_space(7.0);
            board_preview(ui, &self.board);
        });

        ui.add_space(12.0);
        ui.columns(4, |columns| {
            metric_card(
                &mut columns[0],
                "SIMULATED CLOCK",
                format!("{} MHz", self.effective_clock_mhz()),
                &format!("{}× relative speed", self.effective_multiplier()),
                cyan(),
            );
            metric_card(
                &mut columns[1],
                "CPU CYCLES",
                compact_number(self.board.cpu.total_cycles() as f64),
                "since last reset",
                amber(),
            );
            metric_card(
                &mut columns[2],
                "INSTRUCTIONS",
                compact_number(self.board.cpu.total_instructions() as f64),
                "Xtensa teaching core",
                green(),
            );
            metric_card(
                &mut columns[3],
                "MODULES",
                self.board.modules.len().to_string(),
                "virtual peripherals attached",
                red(),
            );
        });

        ui.add_space(12.0);
        ui.columns(2, |columns| {
            card().show(&mut columns[0], |ui| {
                section_label(ui, "CPU PIPELINE");
                ui.add_space(3.0);
                ui.horizontal(|ui| {
                    data_chip(ui, "PC", &format!("0x{:08X}", self.board.cpu.pc_address()), cyan());
                    data_chip(ui, "PS", &format!("0x{:08X}", self.board.cpu.ps()), text_primary());
                });
                ui.add_space(10.0);
                ui.label(
                    RichText::new(self.board.cpu.current_instruction().to_string())
                        .monospace()
                        .size(15.0)
                        .color(text_primary()),
                );
                if let Some(last) = self.board.last_step() {
                    ui.label(
                        RichText::new(format!(
                            "last: 0x{:04X}  ·  {} cycles  ·  GPIO_OUT 0x{:02X}",
                            last.pc_before * 4,
                            last.cycles,
                            self.board.cpu.gpio_out() & 0xff
                        ))
                        .monospace()
                        .size(10.0)
                        .color(text_muted()),
                    );
                }
                ui.add_space(10.0);
                ui.horizontal_wrapped(|ui| {
                    for index in 1..=4 {
                        data_chip(
                            ui,
                            &format!("a{}", index),
                            &format!("0x{:08X}", self.board.cpu.registers()[index]),
                            text_muted(),
                        );
                    }
                });
            });

            card().show(&mut columns[1], |ui| {
                section_label(ui, "SERIAL MONITOR  /  UART0");
                ui.add_space(3.0);
                terminal(ui, &self.board, 152.0);
                ui.add_space(5.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("115200 8N1").size(10.0).color(text_muted()));
                    ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("OPEN MONITOR").clicked() {
                            self.view = View::Serial;
                        }
                    });
                });
            });
        });
    }

    fn draw_modules(&mut self, ui: &mut Ui) {
        card().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    section_label(ui, "PERIPHERAL MODULE BAY");
                    ui.label(
                        RichText::new("Attach virtual hardware to the ESP32 buses")
                            .size(15.0)
                            .strong()
                            .color(text_primary()),
                    );
                    ui.label(
                        RichText::new("Displays, sensors and actuators receive live GPIO and CPU-cycle data.")
                            .size(11.0)
                            .color(text_muted()),
                    );
                });
                ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new(format!("{} ATTACHED", self.board.modules.len()))
                            .size(11.0)
                            .strong()
                            .color(cyan()),
                    );
                });
            });
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_source("module_picker_main")
                    .width(230.0)
                    .selected_text(self.module_to_add.label())
                    .show_ui(ui, |ui| {
                        for kind in ModuleKind::ALL {
                            ui.selectable_value(&mut self.module_to_add, kind, kind.label());
                        }
                    });
                if ui
                    .add(egui::Button::new(RichText::new("＋  ATTACH MODULE").strong().color(text_primary())).fill(cyan()))
                    .clicked()
                {
                    self.board.attach_module(self.module_to_add);
                }
                ui.label(
                    RichText::new("Each attach creates a separate virtual module instance.")
                        .size(10.0)
                        .color(text_muted()),
                );
            });
        });

        ui.add_space(12.0);
        if self.board.modules.is_empty() {
            card().show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(24.0);
                    ui.label(RichText::new("＋").size(32.0).color(text_muted()));
                    ui.label(
                        RichText::new("No modules connected")
                            .size(16.0)
                            .strong()
                            .color(text_primary()),
                    );
                    ui.label(
                        RichText::new("Choose a module above to extend the virtual ESP32.")
                            .size(11.0)
                            .color(text_muted()),
                    );
                    ui.add_space(24.0);
                });
            });
            return;
        }

        let module_ids = self.board.modules.iter().map(|module| module.id).collect::<Vec<_>>();
        for id in module_ids {
            self.draw_module_card(ui, id);
            ui.add_space(10.0);
        }
    }

    fn draw_module_card(&mut self, ui: &mut Ui, id: u32) {
        let Some(module) = self.board.module(id).cloned() else {
            return;
        };
        let mut detach = false;

        card().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(module.kind.label()).size(15.0).strong().color(text_primary()));
                ui.label(RichText::new(module.kind.description()).size(11.0).color(text_muted()));
                ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add(egui::Button::new(RichText::new("DETACH").size(10.0).color(red())))
                        .clicked()
                    {
                        detach = true;
                    }
                    let enabled_label = if module.enabled { "● ENABLED" } else { "○ DISABLED" };
                    if ui
                        .add(egui::Button::new(
                            RichText::new(enabled_label)
                                .size(10.0)
                                .strong()
                                .color(if module.enabled { green() } else { text_muted() }),
                        ))
                        .clicked()
                    {
                        self.board.set_module_enabled(id, !module.enabled);
                    }
                });
            });
            ui.add_space(5.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("MODULE #{:02}", module.id)).size(10.0).color(text_muted()));
                ui.separator();
                ui.label(RichText::new(module.connection_label()).monospace().size(10.0).color(cyan()));
                ui.separator();
                ui.label(RichText::new("LIVE BUS STATE").size(10.0).color(text_muted()));
            });
            ui.add_space(10.0);

            match &module.state {
                ModuleState::Display {
                    lines,
                    columns,
                    rows,
                } => module_display_preview(ui, lines, *columns, *rows, module.kind),
                ModuleState::Sensor {
                    temperature_c,
                    humidity_pct,
                } => module_sensor_preview(ui, *temperature_c, *humidity_pct),
                ModuleState::Pixels { pixels } => module_pixels_preview(ui, pixels),
                ModuleState::Buzzer {
                    active,
                    frequency_hz,
                } => module_buzzer_preview(ui, *active, *frequency_hz),
            }
        });

        if detach {
            self.board.detach_module(id);
        }
    }

    fn draw_cpu(&mut self, ui: &mut Ui) {
        ui.columns(2, |columns| {
            card().show(&mut columns[0], |ui| {
                section_label(ui, "REGISTER FILE  /  16 × 32 BIT");
                ui.add_space(7.0);
                egui::Grid::new("register_grid")
                    .num_columns(2)
                    .spacing([18.0, 7.0])
                    .striped(true)
                    .show(ui, |ui| {
                        for row in 0..8 {
                            let left = row;
                            let right = row + 8;
                            register_row(ui, left, self.board.cpu.registers()[left]);
                            register_row(ui, right, self.board.cpu.registers()[right]);
                            ui.end_row();
                        }
                    });
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    data_chip(ui, "PC", &format!("0x{:08X}", self.board.cpu.pc_address()), cyan());
                    data_chip(ui, "PS", &format!("0x{:08X}", self.board.cpu.ps()), text_primary());
                });
            });

            card().show(&mut columns[1], |ui| {
                section_label(ui, "PIPELINE / DISASSEMBLY");
                ui.add_space(7.0);
                let pc = self.board.cpu.pc();
                let rom = self.board.cpu.rom();
                let start = pc.saturating_sub(4);
                let end = (pc + 5).min(rom.len());
                egui::ScrollArea::vertical().max_height(278.0).show(ui, |ui| {
                    for index in start..end {
                        let active = index == pc;
                        let row_fill = if active {
                            Color32::from_rgb(28, 67, 76)
                        } else {
                            Color32::TRANSPARENT
                        };
                        egui::Frame::none()
                            .fill(row_fill)
                            .rounding(egui::Rounding::same(5.0))
                            .inner_margin(Margin::symmetric(7.0, 5.0))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(if active { "▶" } else { " " })
                                            .color(if active { cyan() } else { text_muted() }),
                                    );
                                    ui.label(
                                        RichText::new(format!("0x{:04X}", index * 4))
                                            .monospace()
                                            .size(11.0)
                                            .color(text_muted()),
                                    );
                                    ui.label(
                                        RichText::new(rom[index].to_string())
                                            .monospace()
                                            .size(11.0)
                                            .color(if active { text_primary() } else { text_muted() }),
                                    );
                                });
                            });
                    }
                });
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("CYCLES").size(10.0).color(text_muted()));
                    ui.label(
                        RichText::new(format!("{} total", self.board.cpu.total_cycles()))
                            .monospace()
                            .color(amber()),
                    );
                    ui.separator();
                    ui.label(RichText::new("HALTED").size(10.0).color(text_muted()));
                    ui.label(
                        RichText::new(if self.board.cpu.halted() { "YES" } else { "NO" })
                            .strong()
                            .color(if self.board.cpu.halted() { red() } else { green() }),
                    );
                });
            });
        });

        ui.add_space(12.0);
        card().show(ui, |ui| {
            section_label(ui, "HOW TURBO WORKS");
            ui.label(
                RichText::new(format!(
                    "The emulator budgets {} virtual cycles per second at 1×. With Turbo at {}×, each render tick receives {}× that budget, so the same LX6 firmware advances {} times faster.",
                    compact_number(NORMAL_CYCLES_PER_SECOND),
                    self.turbo_multiplier,
                    self.effective_multiplier(),
                    self.effective_multiplier()
                ))
                .size(12.0)
                .color(text_primary()),
            );
        });
    }

    fn draw_memory(&mut self, ui: &mut Ui) {
        ui.columns(2, |columns| {
            card().show(&mut columns[0], |ui| {
                section_label(ui, "SRAM MAP");
                ui.add_space(7.0);
                key_value(ui, "BASE", "0x3FFB_0000");
                key_value(ui, "SIZE", &format!("{} KiB", SRAM_BYTES / 1024));
                key_value(ui, "FIRMWARE WRITE", "counter → 0x0000");
                ui.add_space(12.0);
                ui.label(RichText::new("LIVE WATCH").size(10.0).strong().color(text_muted()));
                ui.add_space(5.0);
                data_chip(
                    ui,
                    "[0x0000]",
                    &format!("0x{:02X}", self.board.cpu.memory()[0]),
                    amber(),
                );
                ui.add_space(12.0);
                ui.label(
                    RichText::new("The demo firmware stores its loop counter in SRAM so the memory bus is visible while the CPU runs.")
                        .size(11.0)
                        .color(text_muted()),
                );
            });

            card().show(&mut columns[1], |ui| {
                section_label(ui, "HEXDUMP  /  FIRST 256 BYTES");
                ui.add_space(7.0);
                egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                    let memory = self.board.cpu.memory();
                    for offset in (0..256).step_by(16) {
                        let hex = (0..16)
                            .map(|index| format!("{:02X}", memory[offset + index]))
                            .collect::<Vec<_>>()
                            .join(" ");
                        let ascii = (0..16)
                            .map(|index| {
                                let byte = memory[offset + index];
                                if byte.is_ascii_graphic() {
                                    byte as char
                                } else {
                                    '.'
                                }
                            })
                            .collect::<String>();
                        ui.label(
                            RichText::new(format!("{:04X}   {:<47}  │{}│", offset, hex, ascii))
                                .monospace()
                                .size(11.0)
                                .color(if offset == 0 { amber() } else { text_muted() }),
                        );
                    }
                });
            });
        });
    }

    fn draw_serial(&mut self, ui: &mut Ui) {
        card().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    section_label(ui, "UART0 SERIAL MONITOR");
                    ui.label(RichText::new("Virtual USB-UART bridge · 115200 baud · 8N1").size(11.0).color(text_muted()));
                });
                ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("CLEAR").clicked() {
                        self.board.clear_serial();
                    }
                    ui.label(RichText::new("RX/TX ACTIVE").size(10.0).strong().color(green()));
                });
            });
            ui.add_space(12.0);
            terminal(ui, &self.board, 360.0);
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                let input = ui.add_sized(
                    [ui.available_width() - 74.0, 30.0],
                    egui::TextEdit::singleline(&mut self.serial_input)
                        .hint_text("send a line to the virtual UART"),
                );
                let send = ui.add_sized([64.0, 30.0], egui::Button::new("SEND"));
                if send.clicked() || (input.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                    self.board.send_serial(&self.serial_input);
                    self.serial_input.clear();
                }
            });
        });
    }
}

impl eframe::App for Esp32App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !self.style_applied {
            configure_style(ctx);
            self.style_applied = true;
        }

        self.tick_emulator();
        self.draw_top_bar(ctx);
        self.draw_sidebar(ctx);
        self.draw_status_bar(ctx);
        self.draw_main(ctx);

        ctx.request_repaint_after(Duration::from_millis(16));
    }
}

fn configure_style(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = Vec2::new(8.0, 8.0);
    style.spacing.button_padding = Vec2::new(10.0, 6.0);
    style.visuals = egui::Visuals::dark();
    style.visuals.window_fill = canvas();
    style.visuals.panel_fill = canvas();
    style.visuals.faint_bg_color = panel_raised();
    style.visuals.extreme_bg_color = Color32::from_rgb(7, 11, 18);
    style.visuals.widgets.noninteractive.bg_fill = panel();
    style.visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, text_muted());
    style.visuals.widgets.inactive.bg_fill = panel_raised();
    style.visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, text_primary());
    style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(34, 53, 70);
    style.visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, cyan());
    style.visuals.widgets.active.bg_fill = Color32::from_rgb(32, 83, 91);
    style.visuals.widgets.active.fg_stroke = Stroke::new(1.0, text_primary());
    ctx.set_style(style);
}

fn section_label(ui: &mut Ui, label: &str) {
    ui.label(RichText::new(label).size(10.0).strong().color(text_muted()));
}

fn key_value(ui: &mut Ui, key: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(key).size(10.0).strong().color(text_muted()));
        ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(RichText::new(value).size(11.0).color(text_primary()));
        });
    });
}

fn metric_card(ui: &mut Ui, label: &str, value: String, note: &str, accent: Color32) {
    small_card().show(ui, |ui| {
        ui.set_min_height(78.0);
        ui.label(RichText::new(label).size(9.0).strong().color(text_muted()));
        ui.add_space(4.0);
        ui.label(RichText::new(value).size(19.0).strong().color(accent));
        ui.label(RichText::new(note).size(10.0).color(text_muted()));
    });
}

fn data_chip(ui: &mut Ui, key: &str, value: &str, accent: Color32) {
    small_card().show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(key).size(9.0).strong().color(text_muted()));
            ui.label(RichText::new(value).monospace().size(11.0).color(accent));
        });
    });
}

fn register_row(ui: &mut Ui, index: usize, value: u32) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(format!("a{}", index))
                .monospace()
                .strong()
                .color(cyan()),
        );
        ui.label(
            RichText::new(format!("0x{:08X}", value))
                .monospace()
                .size(11.0)
                .color(text_primary()),
        );
    });
}

fn module_display_preview(
    ui: &mut Ui,
    lines: &[String],
    columns: usize,
    rows: usize,
    kind: ModuleKind,
) {
    let height = if rows >= 4 { 142.0 } else { 105.0 };
    let (rect, _response) = ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 8.0, Color32::from_rgb(6, 16, 20));
    painter.rect_stroke(rect, 8.0, Stroke::new(1.0, Color32::from_rgb(38, 103, 104)));

    let glow = if kind == ModuleKind::Oled128x64 {
        Color32::from_rgb(92, 226, 211)
    } else {
        Color32::from_rgb(255, 189, 82)
    };
    let font_size = if rows >= 4 { 12.0 } else { 14.0 };
    let line_height = (rect.height() - 18.0) / rows.max(1) as f32;
    for (index, line) in lines.iter().take(rows).enumerate() {
        painter.text(
            Pos2::new(rect.left() + 16.0, rect.top() + 10.0 + index as f32 * line_height),
            egui::Align2::LEFT_TOP,
            if columns == 0 { "" } else { line.as_str() },
            FontId::monospace(font_size),
            glow,
        );
    }
    painter.text(
        Pos2::new(rect.right() - 12.0, rect.bottom() - 8.0),
        Align2::RIGHT_BOTTOM,
        if rows >= 4 { "I²C 0x3C" } else { "I²C 0x27" },
        FontId::monospace(9.0),
        text_muted(),
    );
}

fn module_sensor_preview(ui: &mut Ui, temperature_c: f32, humidity_pct: f32) {
    ui.columns(2, |columns| {
        metric_card(
            &mut columns[0],
            "TEMPERATURE",
            format!("{:.1} °C", temperature_c),
            "DHT22 live sample",
            amber(),
        );
        metric_card(
            &mut columns[1],
            "HUMIDITY",
            format!("{:.1} %", humidity_pct),
            "DHT22 live sample",
            cyan(),
        );
    });
}

fn module_pixels_preview(ui: &mut Ui, pixels: &[Rgb]) {
    let (rect, _response) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 94.0), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 8.0, Color32::from_rgb(9, 15, 23));
    painter.rect_stroke(rect, 8.0, Stroke::new(1.0, border()));
    let spacing = rect.width() / (pixels.len().max(1) + 1) as f32;
    for (index, pixel) in pixels.iter().enumerate() {
        let position = Pos2::new(rect.left() + spacing * (index + 1) as f32, rect.center().y);
        let color = Color32::from_rgb(pixel.r, pixel.g, pixel.b);
        painter.circle_filled(position, 14.0, color);
        painter.circle_stroke(position, 16.0, Stroke::new(1.0, Color32::from_rgb(79, 101, 122)));
    }
    painter.text(
        Pos2::new(rect.left() + 14.0, rect.top() + 10.0),
        Align2::LEFT_TOP,
        "WS2812 DATA",
        FontId::monospace(10.0),
        text_muted(),
    );
    painter.text(
        Pos2::new(rect.right() - 14.0, rect.top() + 10.0),
        Align2::RIGHT_TOP,
        "GPIO0",
        FontId::monospace(10.0),
        text_muted(),
    );
}

fn module_buzzer_preview(ui: &mut Ui, active: bool, frequency_hz: u32) {
    small_card().show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(if active { "●" } else { "○" })
                    .size(26.0)
                    .color(if active { amber() } else { text_muted() }),
            );
            ui.vertical(|ui| {
                ui.label(
                    RichText::new(if active { "BUZZER ACTIVE" } else { "BUZZER SILENT" })
                        .strong()
                        .color(if active { amber() } else { text_primary() }),
                );
                ui.label(
                    RichText::new(format!("{} Hz · driven by GPIO0", frequency_hz))
                        .size(11.0)
                        .color(text_muted()),
                );
            });
        });
    });
}

fn terminal(ui: &mut Ui, board: &Esp32Board, height: f32) {
    let terminal_frame = egui::Frame::none()
        .fill(Color32::from_rgb(7, 12, 18))
        .stroke(Stroke::new(1.0, Color32::from_rgb(29, 50, 62)))
        .rounding(egui::Rounding::same(8.0))
        .inner_margin(Margin::same(10.0));
    terminal_frame.show(ui, |ui| {
        egui::ScrollArea::vertical()
            .max_height(height)
            .stick_to_bottom(true)
            .show(ui, |ui| {
                for line in board.serial_lines() {
                    ui.label(
                        RichText::new(line)
                            .monospace()
                            .size(11.0)
                            .color(if line.starts_with("[BOOT]") { cyan() } else { text_primary() }),
                    );
                }
                if !board.serial_partial().is_empty() {
                    ui.label(
                        RichText::new(board.serial_partial())
                            .monospace()
                            .size(11.0)
                            .color(green()),
                    );
                }
                if board.serial_lines().is_empty() && board.serial_partial().is_empty() {
                    ui.label(RichText::new("— no serial data —").monospace().color(text_muted()));
                }
            });
    });
}

fn board_preview(ui: &mut Ui, board: &Esp32Board) {
    let desired = Vec2::new(ui.available_width(), 245.0);
    let (rect, _response) = ui.allocate_exact_size(desired, Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 10.0, Color32::from_rgb(11, 19, 28));

    let board_width = (rect.width() * 0.55).clamp(340.0, 540.0);
    let board_height = 174.0;
    let board_rect = Rect::from_center_size(
        Pos2::new(rect.center().x, rect.center().y + 4.0),
        Vec2::new(board_width.min(rect.width() - 120.0), board_height),
    );
    painter.rect_filled(board_rect, 13.0, Color32::from_rgb(23, 92, 83));
    painter.rect_stroke(board_rect, 13.0, Stroke::new(1.0, Color32::from_rgb(68, 185, 155)));

    // PCB traces and the central ESP32 module.
    let module = Rect::from_center_size(
        Pos2::new(board_rect.center().x, board_rect.center().y + 3.0),
        Vec2::new(board_rect.width() * 0.48, 92.0),
    );
    painter.rect_filled(module, 6.0, Color32::from_rgb(24, 31, 43));
    painter.rect_stroke(module, 6.0, Stroke::new(1.0, Color32::from_rgb(83, 103, 122)));
    painter.text(
        module.center() + Vec2::new(0.0, -12.0),
        Align2::CENTER_CENTER,
        "ESP32",
        FontId::proportional(17.0),
        text_primary(),
    );
    painter.text(
        module.center() + Vec2::new(0.0, 12.0),
        Align2::CENTER_CENTER,
        "LX6  ·  240 MHz",
        FontId::monospace(10.0),
        cyan(),
    );

    for index in 0..GPIO_COUNT {
        let y = board_rect.top() + 20.0 + index as f32 * 17.0;
        let left_pin = Rect::from_min_size(Pos2::new(board_rect.left() - 8.0, y), Vec2::new(18.0, 5.0));
        let right_pin = Rect::from_min_size(Pos2::new(board_rect.right() - 10.0, y), Vec2::new(18.0, 5.0));
        let left_level = board.gpio_level(index);
        painter.rect_filled(left_pin, 2.0, if left_level { amber() } else { Color32::from_rgb(124, 143, 150) });
        painter.rect_filled(right_pin, 2.0, if left_level { amber() } else { Color32::from_rgb(124, 143, 150) });
        painter.line_segment(
            [Pos2::new(board_rect.left() + 12.0, y + 2.5), Pos2::new(module.left(), y + 2.5)],
            Stroke::new(1.0, Color32::from_rgb(71, 139, 126)),
        );
        painter.line_segment(
            [Pos2::new(module.right(), y + 2.5), Pos2::new(board_rect.right() - 12.0, y + 2.5)],
            Stroke::new(1.0, Color32::from_rgb(71, 139, 126)),
        );
    }

    let led_x = board_rect.left() + 34.0;
    let led_y = board_rect.bottom() - 19.0;
    for (offset, pin) in [0_usize, 2_usize].iter().enumerate() {
        let is_on = board.gpio_level(*pin);
        painter.circle_filled(
            Pos2::new(led_x + offset as f32 * 22.0, led_y),
            6.0,
            if is_on { green() } else { Color32::from_rgb(39, 74, 65) },
        );
    }
    painter.text(
        Pos2::new(led_x + 52.0, led_y),
        Align2::LEFT_CENTER,
        "GPIO LEDs",
        FontId::proportional(10.0),
        text_muted(),
    );

    let button_center = Pos2::new(board_rect.right() - 43.0, board_rect.bottom() - 22.0);
    painter.circle_filled(
        button_center,
        10.0,
        if board.button_pressed() { red() } else { Color32::from_rgb(81, 93, 108) },
    );
    painter.circle_stroke(button_center, 12.0, Stroke::new(1.0, text_primary()));
    painter.text(
        button_center + Vec2::new(-31.0, 0.0),
        Align2::RIGHT_CENTER,
        "BOOT",
        FontId::proportional(10.0),
        text_muted(),
    );

    painter.text(
        Pos2::new(rect.left() + 18.0, rect.top() + 18.0),
        Align2::LEFT_CENTER,
        "VIRTUAL USB",
        FontId::monospace(10.0),
        green(),
    );
    painter.text(
        Pos2::new(rect.right() - 18.0, rect.top() + 18.0),
        Align2::RIGHT_CENTER,
        "CONNECTED",
        FontId::monospace(10.0),
        green(),
    );
    painter.text(
        Pos2::new(rect.left() + 18.0, rect.bottom() - 16.0),
        Align2::LEFT_CENTER,
        "GPIO0 — GPIO7",
        FontId::monospace(10.0),
        text_muted(),
    );
}

fn compact_number(value: f64) -> String {
    if value >= 1_000_000_000.0 {
        format!("{:.1}G", value / 1_000_000_000.0)
    } else if value >= 1_000_000.0 {
        format!("{:.1}M", value / 1_000_000.0)
    } else if value >= 1_000.0 {
        format!("{:.1}k", value / 1_000.0)
    } else {
        format!("{:.0}", value)
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1420.0, 900.0])
            .with_min_inner_size([1050.0, 700.0]),
        ..Default::default()
    };

    eframe::run_native(
        "ESP32 Emulator · Xtensa LX6",
        options,
        Box::new(|creation_context| Ok(Box::new(Esp32App::new(creation_context)))),
    )
}
