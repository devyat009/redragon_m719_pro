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
    // Diagnostic details:
    pub found_dongle: bool,
    pub found_wired: bool,
    pub handshake_ok: bool,
    pub last_status_detail: String,
    hid_api: Option<hidapi::HidApi>,
}

// Calculate the 17-byte HID Feature Report packet checksum used by the MCU firmware.
// The MCU requires: (sum(pkt[0..15]) + pkt[16]) & 0xFF == 0x55.
pub fn calc_packet_checksum(pkt: &[u8; 17]) -> u8 {
    let mut sum: u8 = 0;
    for i in 0..16 {
        sum = sum.wrapping_add(pkt[i]);
    }
    (0x55u8).wrapping_sub(sum)
}

// Sends a single command (nCmdID, bData) over Report 0x08 on UsagePage 0xFF02 with valid packet checksum.
fn send_mouse_cmd(dev: &hidapi::HidDevice, cmd_id: u8, data: u8) -> Result<(), hidapi::HidError> {
    let mut pkt = [0u8; 17];
    pkt[0] = 0x08; // Report ID 0x08
    pkt[1] = cmd_id;
    pkt[2] = data;
    pkt[16] = calc_packet_checksum(&pkt);
    dev.send_feature_report(&pkt)
}

// Writes a chunk to mouse EEPROM via Report 0x08 on UsagePage 0xFF02.
// Chunks up to 10 bytes (0x0A) per packet as required by the OEM MCU protocol.
fn write_mouse_eeprom(dev: &hidapi::HidDevice, addr: u16, data: &[u8]) -> Result<(), hidapi::HidError> {
    for (chunk_idx, chunk) in data.chunks(10).enumerate() {
        let chunk_addr = addr + (chunk_idx * 10) as u16;
        let mut pkt = [0u8; 17];
        pkt[0] = 0x08; // Report ID 0x08
        pkt[1] = 0x07; // Command 0x07: Write EEPROM
        pkt[2] = (chunk_addr >> 8) as u8;
        pkt[3] = (chunk_addr & 0xFF) as u8;
        pkt[4] = chunk.len() as u8;
        pkt[5..5 + chunk.len()].copy_from_slice(chunk);
        // Byte 16 is the packet checksum over pkt[0..15]
        pkt[16] = calc_packet_checksum(&pkt);
        dev.send_feature_report(&pkt)?;
        std::thread::sleep(std::time::Duration::from_millis(5));
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
            found_dongle: false,
            found_wired: false,
            handshake_ok: false,
            last_status_detail: String::new(),
            hid_api: hidapi::HidApi::new().ok(),
        };
        bridge.detect();
        bridge
    }

    pub fn detect(&mut self) -> bool {
        let mut found_wired = false;
        let mut found_dongle = false;
        let mut mouse_responsive = false;
        let mut live_battery = 0u8;

        if self.hid_api.is_none() {
            self.hid_api = hidapi::HidApi::new().ok();
        }

        if let Some(api) = &mut self.hid_api {
            if api.refresh_devices().is_err() {
                self.hid_api = hidapi::HidApi::new().ok();
            }
        }

        if let Some(api) = &self.hid_api {
            let mut dev_cmd = None;
            let mut dev_notify = None;

            for device in api.device_list() {
                if device.vendor_id() == DEFAULT_VID {
                    if device.product_id() == DEFAULT_PID_WIRED {
                        found_wired = true;
                    } else if device.product_id() == DEFAULT_PID_WIRELESS {
                        found_dongle = true;
                    }

                    if device.usage_page() == 0xFF02 && dev_cmd.is_none() {
                        dev_cmd = device.open_device(api).ok();
                    }
                    if device.usage_page() == 0xFF01 && dev_notify.is_none() {
                        dev_notify = device.open_device(api).ok();
                    }

                    // Query live battery from UsagePage 0xFF04 (Report 0x06)
                    if device.usage_page() == 0xFF04 {
                        if let Ok(dev) = device.open_device(api) {
                            let mut buf = [0u8; 9];
                            buf[0] = 0x06;
                            if let Ok(n) = dev.get_feature_report(&mut buf) {
                                if n >= 4 {
                                    let bat = buf[3];
                                    if bat > 0 && bat <= 100 {
                                        live_battery = bat;
                                        mouse_responsive = true;
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Real Hardware Handshake: Verify if the mouse is physically switched ON and responsive
            if found_dongle {
                if let (Some(cmd), Some(notify)) = (dev_cmd, dev_notify) {
                    let mut pkt = [0u8; 17];
                    pkt[0] = 0x08;
                    pkt[1] = 0x03; // GetConnectStatus
                    pkt[16] = calc_packet_checksum(&pkt); // Exact checksum: 0x4A

                    for _ in 0..3 {
                        if cmd.send_feature_report(&pkt).is_ok() {
                            let mut buf = [0u8; 64];
                            if let Ok(n) = notify.read_timeout(&mut buf, 100) {
                                if n >= 3 && buf[0] == 0x09 && buf[1] == 0x03 {
                                    if (n >= 7 && (buf[5] != 0 || buf[6] != 0)) || live_battery > 0 {
                                        mouse_responsive = true;
                                        break;
                                    }
                                }
                            }
                        }
                        std::thread::sleep(std::time::Duration::from_millis(15));
                    }
                }
            }
        }

        self.found_dongle = found_dongle;
        self.found_wired = found_wired;
        self.handshake_ok = mouse_responsive;

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
            self.last_status_detail = "Wired USB cable connected and active.".to_string();
            true
        } else if found_dongle && mouse_responsive {
            self.connected = true;
            self.is_wireless = true;
            self.is_charging = false;
            self.vid = DEFAULT_VID;
            self.pid = DEFAULT_PID_WIRELESS;
            self.device_name = "Redragon M719RGB-PRO (Wireless 2.4G)".to_string();
            self.last_status_detail = "Wireless 2.4G dongle connected and mouse is powered ON and responding.".to_string();
            true
        } else if found_dongle {
            // Dongle plugged into PC USB, but mouse is turned OFF or asleep
            self.connected = false;
            self.is_wireless = true;
            self.is_charging = false;
            self.vid = DEFAULT_VID;
            self.pid = DEFAULT_PID_WIRELESS;
            self.device_name = "Redragon M719RGB-PRO (Mouse Off / Standby)".to_string();
            self.last_status_detail = "2.4G USB Dongle (PID 0xFA7E) detected, but mouse power switch is OFF or mouse is in standby sleep mode.".to_string();
            false
        } else {
            self.connected = false;
            self.is_wireless = false;
            self.is_charging = false;
            self.device_name = "No Device Connected".to_string();
            self.last_status_detail = "No Redragon USB device found (neither 2.4G receiver 0xFA7E nor wired USB cable 0xFAB0 detected on USB bus).".to_string();
            false
        }
    }

    pub fn apply_profile(&self, profile: &MouseProfile, profile_index: usize) -> Result<(), String> {
        Self::apply_profile_direct(self.vid, profile, profile_index, |_, _| {})
    }

    pub fn apply_profile_with_progress<F: Fn(f32, &str)>(&self, profile: &MouseProfile, profile_index: usize, on_progress: F) -> Result<(), String> {
        Self::apply_profile_direct(self.vid, profile, profile_index, on_progress)
    }

    pub fn apply_profile_direct<F: Fn(f32, &str)>(vid: u16, profile: &MouseProfile, profile_index: usize, on_progress: F) -> Result<(), String> {
        const SPEED_LUT: [u8; 10] = [0xFF, 0xE6, 0xD2, 0xBE, 0xAA, 0x96, 0x82, 0x6E, 0x46, 0x28];
        const BRIGHTNESS_LUT: [u8; 10] = [0x19, 0x32, 0x4B, 0x64, 0x7D, 0x96, 0xAF, 0xC8, 0xE1, 0xFF];

        let mut delivered = false;

        on_progress(0.05, "Connecting to Mouse MCU...");

        if let Ok(api) = hidapi::HidApi::new() {
            // Polling rate code: 1000Hz -> 0x01, 500Hz -> 0x02, 250Hz -> 0x04, 125Hz -> 0x08
            let rate_hw_byte = match profile.polling_rate {
                PollingRate::Hz1000 => 0x01,
                PollingRate::Hz500 => 0x02,
                PollingRate::Hz250 => 0x04,
                PollingRate::Hz125 => 0x08,
            };

            for dev_info in api.device_list() {
                if dev_info.vendor_id() == vid && (dev_info.product_id() == DEFAULT_PID_WIRED || dev_info.product_id() == DEFAULT_PID_WIRELESS) {
                    // PRIMARY EEPROM SYNC VIA USAGE PAGE 0xFF02 (Report 0x08)
                    if dev_info.usage_page() == 0xFF02 {
                        if let Ok(dev) = dev_info.open_device(&api) {
                            // 1. Set USB Polling Rate (EEPROM Addr 0x0000, 2 bytes)
                            on_progress(0.15, "Writing USB Polling Rate...");
                            let rate_chk = (0x55u8).wrapping_sub(rate_hw_byte);
                            let rate_data = [rate_hw_byte, rate_chk];
                            let _ = write_mouse_eeprom(&dev, 0x0000, &rate_data);
                            std::thread::sleep(std::time::Duration::from_millis(30));

                            // 2. Set Main RGB Lighting (EEPROM Addr 0x00A0, 7 bytes)
                            on_progress(0.35, "Syncing RGB Lighting & Effects...");
                            let spd_idx = (profile.led_speed.clamp(1, 10) - 1) as usize;
                            let bri_idx = (profile.led_brightness.clamp(1, 10) - 1) as usize;
                            let hw_spd = SPEED_LUT[spd_idx];
                            let (hw_effect, hw_bri) = match profile.led_effect {
                                LedEffect::Steady => (0x02, BRIGHTNESS_LUT[bri_idx]),
                                LedEffect::Respiration => (0x01, BRIGHTNESS_LUT[bri_idx]),
                                LedEffect::Neon => (0x03, BRIGHTNESS_LUT[bri_idx]),
                                LedEffect::Streaming => (0x04, BRIGHTNESS_LUT[bri_idx]),
                                LedEffect::Off => (0x04, 0x00),
                            };

                            let mut main_rgb = [0u8; 7];
                            main_rgb[0] = profile.led_color[0]; // Red
                            main_rgb[1] = profile.led_color[1]; // Green
                            main_rgb[2] = profile.led_color[2]; // Blue
                            main_rgb[3] = hw_spd;               // Speed LUT
                            main_rgb[4] = hw_effect;            // Effect hardware code
                            main_rgb[5] = hw_bri;               // Brightness LUT
                            let mut rgb_sum = 0u8;
                            for b in &main_rgb[0..6] {
                                rgb_sum = rgb_sum.wrapping_add(*b);
                            }
                            main_rgb[6] = (0x55u8).wrapping_sub(rgb_sum);
                            let _ = write_mouse_eeprom(&dev, 0x00A0, &main_rgb);
                            std::thread::sleep(std::time::Duration::from_millis(30));

                            // 3. Set DPI Stages & Active Stage (EEPROM Addr 0x0054, 8 bytes)
                            on_progress(0.55, "Calibrating Sensor DPI Stages...");
                            let mut dpi_stages_data = [0u8; 8];
                            for (idx, stage) in profile.dpi_stages.iter().enumerate().take(5) {
                                dpi_stages_data[idx] = (stage.x / 100).clamp(1, 100) as u8;
                            }
                            dpi_stages_data[5] = (profile.current_stage as u8).saturating_sub(1);
                            dpi_stages_data[6] = 0x00;
                            let mut dpi_sum = 0u8;
                            for b in &dpi_stages_data[0..7] {
                                dpi_sum = dpi_sum.wrapping_add(*b);
                            }
                            dpi_stages_data[7] = (0x55u8).wrapping_sub(dpi_sum);
                            let _ = write_mouse_eeprom(&dev, 0x0054, &dpi_stages_data);
                            std::thread::sleep(std::time::Duration::from_millis(30));

                            // 4. Set Logo RGB (EEPROM Addr 0x0058, 2 bytes)
                            on_progress(0.68, "Updating Logo LED...");
                            let logo_effect = match profile.dpi_effect {
                                DpiEffect::Steady => 0x01,
                                DpiEffect::Respiration => 0x02,
                                DpiEffect::Off => 0x00,
                            };
                            let logo_chk = (0x55u8).wrapping_sub(logo_effect);
                            let logo_data = [logo_effect, logo_chk];
                            let _ = write_mouse_eeprom(&dev, 0x0058, &logo_data);
                            std::thread::sleep(std::time::Duration::from_millis(30));

                            // 5. Set DPI Stage Colors (EEPROM Addr 0x004C, 6 bytes)
                            on_progress(0.78, "Saving DPI Stage Indicators...");
                            let mut dpi_colors = [0u8; 6];
                            for (idx, stage) in profile.dpi_stages.iter().enumerate().take(5) {
                                dpi_colors[idx] = stage.color_rgb[0]; // Stage color identifier
                            }
                            let mut color_sum = 0u8;
                            for b in &dpi_colors[0..5] {
                                color_sum = color_sum.wrapping_add(*b);
                            }
                            dpi_colors[5] = (0x55u8).wrapping_sub(color_sum);
                            let _ = write_mouse_eeprom(&dev, 0x004C, &dpi_colors);
                            std::thread::sleep(std::time::Duration::from_millis(30));

                            // 6. Set Button Matrix (EEPROM Addr 0x0060..0x007F, 8 buttons * 4 bytes)
                            on_progress(0.88, "Writing Hardware Button Matrix...");
                            for (idx, b_action) in profile.button_actions.iter().enumerate().take(8) {
                                let (b0, b1, b2): (u8, u8, u8) = match b_action.as_str() {
                                    "Left button" => (0x01, 0x01, 0x00),
                                    "Right button" => (0x01, 0x02, 0x00),
                                    "Middle button" => (0x01, 0x04, 0x00),
                                    "Forward" => (0x01, 0x10, 0x00),
                                    "Back" => (0x01, 0x08, 0x00),
                                    "Fire key" => (0x07, 0x00, 0x00),
                                    "DPI +" => (0x04, 0x32, 0x02),
                                    "DPI -" => (0x04, 0x32, 0x03),
                                    "RGB ON/OFF" | "RGB Switch" => (0x04, 0x32, 0x05),
                                    "Disable" => (0x05, 0x00, 0x00),
                                    _ => (0x01, 0x01, 0x00),
                                };
                                let btn_chk = (0x55u8).wrapping_sub(b0.wrapping_add(b1).wrapping_add(b2));
                                let btn_chunk = [b0, b1, b2, btn_chk];
                                let btn_addr = 0x0060 + (idx as u16 * 4);
                                let _ = write_mouse_eeprom(&dev, btn_addr, &btn_chunk);
                            }
                            std::thread::sleep(std::time::Duration::from_millis(30));

                            // 7. COMMIT & FLASH SAVE: SendCMD(0x09, 0x00) followed by SendCMD(0x02, 0x00)
                            on_progress(0.96, "Committing to Mouse Flash Memory...");
                            let _ = send_mouse_cmd(&dev, 0x09, 0x00);
                            std::thread::sleep(std::time::Duration::from_millis(40));
                            let _ = send_mouse_cmd(&dev, 0x02, 0x00);

                            on_progress(1.0, "Configuration Applied Successfully!");
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
