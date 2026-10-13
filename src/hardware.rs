use std::fs;
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};

pub const DEFAULT_VID: u16 = 0x25A7;
pub const DEFAULT_PID_WIRED: u16 = 0xFAB0;
pub const DEFAULT_PID_WIRELESS: u16 = 0xFA7E;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DpiStage {
    pub enabled: bool,
    pub x: u32,
    pub y: u32,
    pub color_rgb: [u8; 3],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PollingRate {
    Hz125 = 125,
    Hz250 = 250,
    Hz500 = 500,
    Hz1000 = 1000,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LedEffect {
    Steady = 1,
    Respiration = 2,
    Neon = 3,
    Streaming = 4,
    Off = 5,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DpiEffect {
    Steady = 1,
    Respiration = 2,
    Off = 3,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MacroItem {
    pub name: String,
    pub key_events: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MouseProfile {
    pub name: String,
    pub polling_rate: PollingRate,
    pub battery_percent: u8,
    pub current_stage: usize,
    pub dpi_stages: Vec<DpiStage>,
    pub dpi_effect: DpiEffect,
    pub led_effect: LedEffect,
    pub led_brightness: u8, // 1 - 10
    pub led_speed: u8,      // 1 - 10
    pub led_color: [u8; 3],
    pub custom_colors: Vec<[u8; 3]>, // up to 10 custom colors
    pub mouse_sensitivity: u32, // 1 - 20
    pub enhance_pointer_precision: bool,
    pub scroll_speed: u32,      // 1 - 10
    pub scroll_one_page: bool,
    pub double_click_speed: u32, // 1 - 10
    pub button_actions: [String; 8],
    pub selected_button: usize,
}

impl Default for MouseProfile {
    fn default() -> Self {
        Self {
            name: "Profile 1".to_string(),
            polling_rate: PollingRate::Hz1000,
            battery_percent: 100,
            current_stage: 1,
            dpi_stages: vec![
                DpiStage { enabled: true, x: 500, y: 500, color_rgb: [255, 0, 0] },
                DpiStage { enabled: true, x: 1000, y: 1000, color_rgb: [0, 255, 255] },
                DpiStage { enabled: true, x: 2000, y: 2000, color_rgb: [27, 94, 32] },
                DpiStage { enabled: true, x: 3000, y: 3000, color_rgb: [255, 255, 0] },
                DpiStage { enabled: true, x: 10000, y: 10000, color_rgb: [255, 0, 255] },
            ],
            dpi_effect: DpiEffect::Off,
            led_effect: LedEffect::Steady,
            led_brightness: 5,
            led_speed: 5,
            led_color: [255, 0, 0],
            custom_colors: vec![
                [27, 94, 32],   // Dark Forest Green
                [13, 71, 161],  // Dark Royal Blue
                [215, 38, 56],  // Redragon Red
                [255, 140, 0],  // Amber
            ],
            mouse_sensitivity: 10,
            enhance_pointer_precision: true,
            scroll_speed: 3,
            scroll_one_page: false,
            double_click_speed: 5,
            button_actions: [
                "Left button".to_string(),
                "Right button".to_string(),
                "Middle button".to_string(),
                "Forward".to_string(),
                "Back".to_string(),
                "Fire key".to_string(),
                "DPI +".to_string(),
                "DPI -".to_string(),
            ],
            selected_button: 1,
        }
    }
}

pub struct HardwareBridge {
    pub connected: bool,
    pub is_charging: bool,
    pub is_wireless: bool,
    pub battery_percent: u8,
    pub device_name: String,
    pub vid: u16,
    pub pid: u16,
}

// Helper to construct a 4-byte checksum block where b0 + b1 + b2 + checksum == 0x55
fn make_checksum_block(b0: u8, b1: u8, b2: u8) -> [u8; 4] {
    let chk = (0x55u8).wrapping_sub(b0).wrapping_sub(b1).wrapping_sub(b2);
    [b0, b1, b2, chk]
}

// Writes a chunk to mouse EEPROM via Report 0x08 on UsagePage 0xFF02
fn write_mouse_eeprom(dev: &hidapi::HidDevice, addr: u16, data: &[u8]) -> Result<(), hidapi::HidError> {
    for (chunk_idx, chunk) in data.chunks(8).enumerate() {
        let chunk_addr = addr + (chunk_idx * 8) as u16;
        let mut pkt = [0u8; 17];
        pkt[0] = 0x08; // Report ID 0x08
        pkt[1] = 0x07; // Command 0x07: Write EEPROM
        pkt[2] = (chunk_addr >> 8) as u8;
        pkt[3] = (chunk_addr & 0xFF) as u8;
        pkt[4] = chunk.len() as u8;
        pkt[5..5 + chunk.len()].copy_from_slice(chunk);
        dev.send_feature_report(&pkt)?;
    }
    Ok(())
}

impl HardwareBridge {
    pub fn new() -> Self {
        let mut bridge = Self {
            connected: false,
            is_charging: false,
            is_wireless: true,
            battery_percent: 100,
            device_name: "Scanning...".to_string(),
            vid: DEFAULT_VID,
            pid: DEFAULT_PID_WIRELESS,
        };
        bridge.detect();
        bridge
    }

    pub fn detect(&mut self) -> bool {
        let mut found_wired = false;
        let mut found_wireless = false;
        let mut live_battery = 0u8;

        if let Ok(api) = hidapi::HidApi::new() {
            for device in api.device_list() {
                if device.vendor_id() == DEFAULT_VID {
                    if device.product_id() == DEFAULT_PID_WIRED {
                        found_wired = true;
                    } else if device.product_id() == DEFAULT_PID_WIRELESS {
                        found_wireless = true;
                    }

                    // Query live battery & charging status from UsagePage 0xFF04 (Report 0x06)
                    if device.usage_page() == 0xFF04 {
                        if let Ok(dev) = device.open_device(&api) {
                            let mut buf = [0u8; 9];
                            buf[0] = 0x06;
                            if let Ok(n) = dev.get_feature_report(&mut buf) {
                                if n >= 4 {
                                    let bat = buf[3];
                                    if bat > 0 && bat <= 100 {
                                        live_battery = bat;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        if live_battery > 0 {
            self.battery_percent = live_battery;
        }

        if found_wired {
            self.connected = true;
            self.is_wireless = false;
            self.is_charging = true;
            self.vid = DEFAULT_VID;
            self.pid = DEFAULT_PID_WIRED;
            self.device_name = "Redragon M719RGB-PRO (Wired USB)".to_string();
            true
        } else if found_wireless {
            self.connected = true;
            self.is_wireless = true;
            self.is_charging = false;
            self.vid = DEFAULT_VID;
            self.pid = DEFAULT_PID_WIRELESS;
            self.device_name = "Redragon M719RGB-PRO (Wireless 2.4G)".to_string();
            true
        } else {
            self.connected = false;
            self.is_charging = false;
            self.device_name = "No Device Connected".to_string();
            false
        }
    }

    pub fn apply_profile(&self, profile: &MouseProfile, profile_index: usize) -> Result<(), String> {
        let mut delivered = false;

        if let Ok(api) = hidapi::HidApi::new() {
            // Polling rate code: 1000Hz -> 0x01, 500Hz -> 0x02, 250Hz -> 0x04, 125Hz -> 0x08
            let rate_hw_byte = match profile.polling_rate {
                PollingRate::Hz1000 => 0x01,
                PollingRate::Hz500 => 0x02,
                PollingRate::Hz250 => 0x04,
                PollingRate::Hz125 => 0x08,
            };

            for dev_info in api.device_list() {
                if dev_info.vendor_id() == self.vid && (dev_info.product_id() == DEFAULT_PID_WIRED || dev_info.product_id() == DEFAULT_PID_WIRELESS) {
                    // 1. PRIMARY EEPROM SYNC VIA USAGE PAGE 0xFF02 (Report 0x08)
                    if dev_info.usage_page() == 0xFF02 {
                        if let Ok(dev) = dev_info.open_device(&api) {
                            // A. Set Lighting (EEPROM Addr 0x0C, 8 bytes: two checksum blocks)
                            let blk0 = make_checksum_block(
                                profile.led_effect as u8,
                                profile.led_brightness.clamp(1, 10),
                                profile.led_speed.clamp(1, 10),
                            );
                            let blk1 = make_checksum_block(
                                profile.led_color[0],
                                profile.led_color[1],
                                profile.led_color[2],
                            );
                            let mut led_data = [0u8; 8];
                            led_data[0..4].copy_from_slice(&blk0);
                            led_data[4..8].copy_from_slice(&blk1);
                            let _ = write_mouse_eeprom(&dev, 0x000C, &led_data);

                            // B. Set USB Polling Rate (EEPROM Addr 0x00, 4 bytes: one checksum block)
                            let rate_blk = make_checksum_block(rate_hw_byte, 0, 0);
                            let _ = write_mouse_eeprom(&dev, 0x0000, &rate_blk);

                            // C. Set Button Map (EEPROM Addr 0x4C, 8 buttons: 32 bytes)
                            let mut btn_data = Vec::with_capacity(32);
                            for b_action in &profile.button_actions {
                                let (b0, b1, b2) = match b_action.as_str() {
                                    "Left button" => (0x01, 0x11, 0x01),
                                    "Right button" => (0x01, 0x13, 0x02),
                                    "Middle button" => (0x01, 0x12, 0x03),
                                    "Forward" => (0x01, 0x14, 0x05),
                                    "Back" => (0x01, 0x15, 0x04),
                                    "Fire key" => (0x08, 0xA4, 0x09),
                                    "DPI +" => (0x08, 0xA8, 0x0B),
                                    "DPI -" => (0x08, 0xA9, 0x0C),
                                    "RGB ON/OFF" => (0x08, 0xA2, 0x06),
                                    _ => (0x01, 0x11, 0x01),
                                };
                                btn_data.extend_from_slice(&make_checksum_block(b0, b1, b2));
                            }
                            let _ = write_mouse_eeprom(&dev, 0x004C, &btn_data);

                            // D. Notify MCU to reload/refresh live settings (Command 0x02)
                            let mut cmd_pkt = [0u8; 17];
                            cmd_pkt[0] = 0x08;
                            cmd_pkt[1] = 0x02; // Reload/Apply CMD
                            let _ = dev.send_feature_report(&cmd_pkt);

                            delivered = true;
                        }
                    }

                    // 2. MCU RUNTIME CACHE SYNC VIA USAGE PAGE 0xFF04 (Report 0x06)
                    if dev_info.usage_page() == 0xFF04 {
                        if let Ok(dev) = dev_info.open_device(&api) {
                            // Packet A: Set LED Effect, Brightness, Speed & Color (Report 0x06)
                            let mut led_pkt = [0u8; 9];
                            led_pkt[0] = 0x06;
                            led_pkt[1] = 0x02;
                            led_pkt[2] = profile.led_effect as u8;
                            led_pkt[3] = profile.led_brightness;
                            led_pkt[4] = profile.led_speed;
                            led_pkt[5] = profile.led_color[0];
                            led_pkt[6] = profile.led_color[1];
                            led_pkt[7] = profile.led_color[2];
                            let _ = dev.send_feature_report(&led_pkt);

                            // Packet B: Set USB Polling Rate (Report 0x06)
                            let rate_pkt_val = match profile.polling_rate {
                                PollingRate::Hz125 => 1,
                                PollingRate::Hz250 => 2,
                                PollingRate::Hz500 => 3,
                                PollingRate::Hz1000 => 4,
                            };
                            let mut rate_pkt = [0u8; 9];
                            rate_pkt[0] = 0x06;
                            rate_pkt[1] = 0x03;
                            rate_pkt[2] = rate_pkt_val;
                            let _ = dev.send_feature_report(&rate_pkt);

                            // Packet C: Set Active DPI Stage
                            let mut dpi_pkt = [0u8; 9];
                            dpi_pkt[0] = 0x06;
                            dpi_pkt[1] = 0x04;
                            dpi_pkt[2] = (profile.current_stage as u8).max(1);
                            let _ = dev.send_feature_report(&dpi_pkt);

                            delivered = true;
                        }
                    }
                }
            }
        }

        // 3. Sync to OEM database %LOCALAPPDATA%\JM01\pro.dct
        let oem_path = get_oem_profile_path();
        if let Some(path) = oem_path {
            if path.exists() {
                if let Ok(mut bytes) = fs::read(&path) {
                    let prof_size = 2992;
                    let prof_offset = profile_index * prof_size;
                    if prof_offset + prof_size <= bytes.len() {
                        bytes[0] = 0x34;
                        bytes[1] = 0x12;
                        bytes[2] = 0xAA;
                        bytes[3] = 0xFC;
                        bytes[4] = (profile_index as u8) + 1;

                        for (i, stage) in profile.dpi_stages.iter().enumerate().take(5) {
                            let stage_base = prof_offset + 0x0A08 + (i * 16);
                            if stage_base + 12 <= bytes.len() {
                                let val_x = (stage.x as u32).to_le_bytes();
                                let val_y = (stage.y as u32).to_le_bytes();
                                bytes[stage_base..stage_base+4].copy_from_slice(&val_x);
                                bytes[stage_base+4..stage_base+8].copy_from_slice(&val_y);
                                bytes[stage_base+8] = stage.color_rgb[0];
                                bytes[stage_base+9] = stage.color_rgb[1];
                                bytes[stage_base+10] = stage.color_rgb[2];
                            }
                        }

                        let rate_offset = prof_offset + 0x0A94;
                        if rate_offset + 4 <= bytes.len() {
                            let rate_val: u32 = match profile.polling_rate {
                                PollingRate::Hz125 => 1,
                                PollingRate::Hz250 => 2,
                                PollingRate::Hz500 => 3,
                                PollingRate::Hz1000 => 4,
                            };
                            bytes[rate_offset..rate_offset+4].copy_from_slice(&rate_val.to_le_bytes());
                        }

                        let led_offset = prof_offset + 0x0A9C;
                        if led_offset + 8 <= bytes.len() {
                            bytes[led_offset] = profile.led_effect as u8;
                            bytes[led_offset + 1] = profile.led_brightness;
                            bytes[led_offset + 2] = profile.led_speed;
                            bytes[led_offset + 4] = profile.led_color[0];
                            bytes[led_offset + 5] = profile.led_color[1];
                            bytes[led_offset + 6] = profile.led_color[2];
                        }

                        let _ = fs::write(&path, &bytes);
                        delivered = true;
                    }
                }
            }
        }

        if delivered {
            Ok(())
        } else {
            Err("No supported mouse device found to apply settings".to_string())
        }
    }
}

pub fn parse_jmk_bytes(bytes: &[u8]) -> Option<MouseProfile> {
    if bytes.len() < 2992 {
        return None;
    }
    let mut profile = MouseProfile::default();
    let prof_offset = 0;

    for (i, stage) in profile.dpi_stages.iter_mut().enumerate().take(5) {
        let stage_base = prof_offset + 0x0A08 + (i * 16);
        if stage_base + 12 <= bytes.len() {
            let x = u32::from_le_bytes(bytes[stage_base..stage_base+4].try_into().unwrap_or([0,0,0,0]));
            let y = u32::from_le_bytes(bytes[stage_base+4..stage_base+8].try_into().unwrap_or([0,0,0,0]));
            if x >= 100 && x <= 10000 {
                stage.x = x;
                stage.y = y;
            }
            stage.color_rgb = [bytes[stage_base+8], bytes[stage_base+9], bytes[stage_base+10]];
        }
    }

    let led_offset = prof_offset + 0x0A9C;
    if led_offset + 7 <= bytes.len() {
        let effect_byte = bytes[led_offset];
        profile.led_effect = match effect_byte {
            1 => LedEffect::Steady,
            2 => LedEffect::Respiration,
            3 => LedEffect::Neon,
            4 => LedEffect::Streaming,
            _ => LedEffect::Off,
        };
        profile.led_brightness = bytes[led_offset + 1].clamp(1, 10);
        profile.led_speed = bytes[led_offset + 2].clamp(1, 10);
        profile.led_color = [bytes[led_offset + 4], bytes[led_offset + 5], bytes[led_offset + 6]];
    }

    Some(profile)
}

pub fn get_oem_profile_path() -> Option<PathBuf> {
    if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
        let p = Path::new(&local_app_data).join("JM01").join("pro.dct");
        return Some(p);
    }
    None
}
