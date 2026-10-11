use std::fmt;

const GPIO0_MASK: u32 = 1;
const BOOT_BUTTON_MASK: u32 = 1 << 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModuleKind {
    Oled128x64,
    Lcd1602,
    Dht22,
    NeoPixel8,
    Buzzer,
}

impl ModuleKind {
    pub const ALL: [Self; 5] = [
        Self::Oled128x64,
        Self::Lcd1602,
        Self::Dht22,
        Self::NeoPixel8,
        Self::Buzzer,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Oled128x64 => "OLED 128×64",
            Self::Lcd1602 => "LCD 1602",
            Self::Dht22 => "DHT22 sensor",
            Self::NeoPixel8 => "WS2812 ×8",
            Self::Buzzer => "Active buzzer",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Oled128x64 => "I²C monochrome display",
            Self::Lcd1602 => "I²C character display",
            Self::Dht22 => "temperature + humidity",
            Self::NeoPixel8 => "8 addressable RGB LEDs",
            Self::Buzzer => "GPIO tone output",
        }
    }

    pub fn bus_label(self) -> &'static str {
        match self {
            Self::Oled128x64 | Self::Lcd1602 => "I²C",
            Self::Dht22 => "SINGLE-WIRE",
            Self::NeoPixel8 => "GPIO / DATA",
            Self::Buzzer => "GPIO",
        }
    }

    pub fn address(self) -> Option<u8> {
        match self {
            Self::Oled128x64 => Some(0x3C),
            Self::Lcd1602 => Some(0x27),
            Self::Dht22 | Self::NeoPixel8 | Self::Buzzer => None,
        }
    }

}

impl fmt::Display for ModuleKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str((*self).label())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    pub const fn off() -> Self {
        Self::new(12, 20, 29)
    }
}

#[derive(Clone, Debug)]
pub enum ModuleState {
    Display {
        lines: Vec<String>,
        columns: usize,
        rows: usize,
    },
    Sensor {
        temperature_c: f32,
        humidity_pct: f32,
    },
    Pixels {
        pixels: Vec<Rgb>,
    },
    Buzzer {
        active: bool,
        frequency_hz: u32,
    },
}

impl ModuleState {
    fn for_kind(kind: ModuleKind) -> Self {
        match kind {
            ModuleKind::Oled128x64 => Self::Display {
                lines: vec![
                    "ESP32 // ONLINE".to_owned(),
                    "CLK  240 MHz".to_owned(),
                    "GPIO 0x00".to_owned(),
                    "CYC 0".to_owned(),
                ],
                columns: 21,
                rows: 4,
            },
            ModuleKind::Lcd1602 => Self::Display {
                lines: vec!["ESP32 READY".to_owned(), "GPIO:00  CYC:0000".to_owned()],
                columns: 16,
                rows: 2,
            },
            ModuleKind::Dht22 => Self::Sensor {
                temperature_c: 22.0,
                humidity_pct: 45.0,
            },
            ModuleKind::NeoPixel8 => Self::Pixels {
                pixels: vec![Rgb::off(); 8],
            },
            ModuleKind::Buzzer => Self::Buzzer {
                active: false,
                frequency_hz: 440,
            },
        }
    }
}

#[derive(Clone, Debug)]
pub struct PeripheralModule {
    pub id: u32,
    pub kind: ModuleKind,
    pub enabled: bool,
    pub state: ModuleState,
}

impl PeripheralModule {
    pub fn new(id: u32, kind: ModuleKind) -> Self {
        Self {
            id,
            kind,
            enabled: true,
            state: ModuleState::for_kind(kind),
        }
    }

    pub fn connection_label(&self) -> String {
        match self.kind.address() {
            Some(address) => format!("{}  0x{:02X}", self.kind.bus_label(), address),
            None => self.kind.bus_label().to_owned(),
        }
    }

    pub fn reset(&mut self) {
        self.state = ModuleState::for_kind(self.kind);
    }

    /// Advance a peripheral from the state of the virtual ESP32 buses.
    ///
    /// The module API is intentionally small and deterministic. It gives the
    /// UI a live signal without pretending that an arbitrary real-world driver
    /// or a complete I²C implementation is present.
    pub fn tick(&mut self, total_cycles: u64, gpio_out: u32, gpio_in: u32) {
        if !self.enabled {
            return;
        }

        match &mut self.state {
            ModuleState::Display {
                lines,
                columns,
                rows,
            } => {
                let gpio = gpio_out & 0xFF;
                let cycle_k = total_cycles / 1_000;
                if self.kind == ModuleKind::Oled128x64 {
                    *lines = vec![
                        fit_line("ESP32 // ONLINE", *columns),
                        fit_line("CLK  240 MHz", *columns),
                        fit_line(&format!("GPIO 0x{:02X}", gpio), *columns),
                        fit_line(&format!("CYC {:>8}", cycle_k), *columns),
                    ];
                } else {
                    *lines = vec![
                        fit_line("ESP32 READY", *columns),
                        fit_line(&format!("GPIO:{:02X} CYC:{:04}", gpio, cycle_k % 10_000), *columns),
                    ];
                }
                lines.truncate(*rows);
            }
            ModuleState::Sensor {
                temperature_c,
                humidity_pct,
            } => {
                // A repeatable small waveform makes the sensor feel alive while
                // keeping tests and recordings deterministic.
                let phase = ((total_cycles / 3_000) % 40) as f32;
                *temperature_c = 22.0 + (phase - 20.0) * 0.06;
                let button_offset = if gpio_in & BOOT_BUTTON_MASK != 0 { 4.0 } else { 0.0 };
                *humidity_pct = 46.0 + (20.0 - phase) * 0.18 + button_offset;
            }
            ModuleState::Pixels { pixels } => {
                let pixel_count = pixels.len().max(1) as u64;
                let phase = ((total_cycles / 1_200) % pixel_count) as usize;
                let led_on = gpio_out & GPIO0_MASK != 0;
                for (index, pixel) in pixels.iter_mut().enumerate() {
                    let active = led_on && index == phase;
                    *pixel = if active {
                        Rgb::new(43, 220, 155)
                    } else if led_on {
                        Rgb::new(17, 74, 71)
                    } else {
                        Rgb::off()
                    };
                }
            }
            ModuleState::Buzzer {
                active,
                frequency_hz,
            } => {
                *active = gpio_out & GPIO0_MASK != 0;
                *frequency_hz = if *active {
                    440 + ((total_cycles / 100) % 5) as u32 * 20
                } else {
                    440
                };
            }
        }
    }
}

fn fit_line(text: &str, columns: usize) -> String {
    let mut line = text.chars().take(columns).collect::<String>();
    while line.chars().count() < columns {
        line.push(' ');
    }
    line
}

#[cfg(test)]
mod tests {
    use super::{ModuleKind, ModuleState, PeripheralModule};

    #[test]
    fn displays_and_sensors_follow_the_virtual_bus() {
        let mut oled = PeripheralModule::new(1, ModuleKind::Oled128x64);
        oled.tick(12_000, 1, 0);
        match oled.state {
            ModuleState::Display { ref lines, .. } => {
                assert!(lines[2].contains("GPIO 0x01"));
                assert!(lines[3].contains("CYC"));
            }
            _ => panic!("OLED must expose a display state"),
        }

        let mut sensor = PeripheralModule::new(2, ModuleKind::Dht22);
        sensor.tick(9_000, 0, 1 << 4);
        match sensor.state {
            ModuleState::Sensor { humidity_pct, .. } => assert!(humidity_pct > 45.0),
            _ => panic!("DHT22 must expose a sensor state"),
        }
    }

    #[test]
    fn disabled_modules_hold_their_last_value() {
        let mut buzzer = PeripheralModule::new(1, ModuleKind::Buzzer);
        buzzer.enabled = false;
        buzzer.tick(10_000, 1, 0);
        match buzzer.state {
            ModuleState::Buzzer { active, .. } => assert!(!active),
            _ => panic!("buzzer must expose a buzzer state"),
        }
    }
}
