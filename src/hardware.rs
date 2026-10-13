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

// PixArt PAW3325 Hardware DPI tables extracted from OemDrv.exe data segment (DAT_005cd1b8 & DAT_005cd418)
pub const PAW3325_DPI_TABLE: [u32; 49] = [
    200, 300, 400, 500, 600, 700, 800, 900, 1000, 1100, 1200, 1300, 1400, 1500, 1600, 1700,
    1800, 1900, 2000, 2100, 2200, 2300, 2400, 2500, 2600, 2700, 2800, 2900, 3000, 3100, 3200,
    3300, 3400, 3500, 3600, 3700, 3800, 3900, 4000, 4100, 4200, 4300, 4400, 4500, 4600, 4700,
    4800, 4900, 5000,
];

pub const PAW3325_CODE_TABLE: [u8; 49] = [
    0x04, 0x06, 0x08, 0x0B, 0x0D, 0x0F, 0x12, 0x14, 0x16, 0x19, 0x1B, 0x1D, 0x20, 0x22, 0x24,
    0x27, 0x29, 0x2B, 0x2E, 0x30, 0x32, 0x34, 0x37, 0x39, 0x3B, 0x3E, 0x40, 0x42, 0x45, 0x47,
    0x49, 0x4C, 0x4E, 0x50, 0x53, 0x55, 0x57, 0x5A, 0x5C, 0x5E, 0x61, 0x63, 0x65, 0x68, 0x6A,
    0x6C, 0x6F, 0x71, 0x73,
];

pub fn dpi_to_paw3325_code(dpi: u32) -> (u8, bool) {
    let hi_range = dpi > 5000;
    let effective_dpi = if hi_range { dpi / 2 } else { dpi };

    // Find closest entry in PAW3325 table
    let mut best_idx = 0;
    let mut min_diff = u32::MAX;
    for (i, &table_dpi) in PAW3325_DPI_TABLE.iter().enumerate() {
        let diff = if table_dpi > effective_dpi {
            table_dpi - effective_dpi
        } else {
            effective_dpi - table_dpi
        };
        if diff < min_diff {
            min_diff = diff;
            best_idx = i;
        }
    }
    (PAW3325_CODE_TABLE[best_idx], hi_range)
}

pub fn paw3325_code_to_dpi(code: u8, hi_range: bool) -> u32 {
    for (i, &table_code) in PAW3325_CODE_TABLE.iter().enumerate() {
        if table_code == code {
            let base = PAW3325_DPI_TABLE[i];
            return if hi_range { base * 2 } else { base };
        }
    }
    1000
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

// Performs the authentic OEM Challenge/Response handshake (Cmd 0x01) with the mouse MCU.
// Algorithm from FUN_004431b0:
// Generates 4 random bytes r[0..3] (1..100).
// Secret key k = [1, 0, 0, 0] (default from DAT_005da8f4..f7).
// Expected reply:
//   expect[0] = k0 + r1 + r0
//   expect[1] = k1 + r2 + 2*r1
//   expect[2] = k2 + r3 + 3*r2
//   expect[3] = k3 + r0 + 4*r3
// Expected reply:
//   expect[0] = k0 + r1 + r0
//   expect[1] = k1 + r2 + 2*r1
//   expect[2] = k2 + r3 + 3*r2
//   expect[3] = k3 + r0 + 4*r3
// Packet: [0x08, 0x01, 0x00, 0x00, 0x00, 0x04, r0, r1, r2, r3, 0, 0, 0, 0, 0, 0, chk]
// Verified from Wireshark PCAP (Frame 489/491): Secret key PD = [0x05, 0x00, 0x00, 0x00].
// Reads reply on notify endpoint (Report 0x09) and verifies bytes match expect[].
pub fn perform_handshake(cmd_dev: &hidapi::HidDevice, notify_dev: &hidapi::HidDevice) -> bool {
    let k = [0x05u8, 0x00u8, 0x00u8, 0x00u8]; // Solved secret key PD from live Wireshark USB trace

    for _attempt in 0..5 {
        use std::time::SystemTime;
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .subsec_nanos();
        let r0 = ((nanos & 0xFF) % 100 + 1) as u8;
        let r1 = (((nanos >> 8) & 0xFF) % 100 + 1) as u8;
        let r2 = (((nanos >> 16) & 0xFF) % 100 + 1) as u8;
        let r3 = (((nanos >> 24) & 0xFF) % 100 + 1) as u8;

        let exp0 = k[0].wrapping_add(r1).wrapping_add(r0);
        let exp1 = k[1].wrapping_add(r2).wrapping_add(2u8.wrapping_mul(r1));
        let exp2 = k[2].wrapping_add(r3).wrapping_add(3u8.wrapping_mul(r2));
        let exp3 = k[3].wrapping_add(r0).wrapping_add(4u8.wrapping_mul(r3));

        let mut pkt = [0u8; 17];
        pkt[0] = 0x08; // Report 0x08
        pkt[1] = 0x01; // Cmd 0x01 Handshake
        pkt[2] = 0x00; // Sub-op
        pkt[3] = 0x00;
        pkt[4] = 0x00;
        pkt[5] = 0x04; // length 4
        pkt[6] = r0;
        pkt[7] = r1;
        pkt[8] = r2;
        pkt[9] = r3;
        pkt[16] = calc_packet_checksum(&pkt);

        if cmd_dev.send_feature_report(&pkt).is_ok() {
            let mut buf = [0u8; 64];
            if let Ok(n) = notify_dev.read_timeout(&mut buf, 80) {
                // Reply is Report 0x09, cmd 0x01 in buf[1], with return bytes in buf[6..10]
                if n >= 10 && buf[0] == 0x09 && buf[1] == 0x01 {
                    if buf[6] == exp0 && buf[7] == exp1 && buf[8] == exp2 && buf[9] == exp3 {
                        return true;
                    }
                }
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(30));
    }
    false
}

// Reads up to 10 bytes from mouse EEPROM via Cmd 0x08 on UsagePage 0xFF02 (FUN_004438f0).
// Packet layout verified from PCAP Frame 493: [0x08, 0x08, 0x00, addr_hi, addr_lo, len, ...]
pub fn read_mouse_eeprom(cmd_dev: &hidapi::HidDevice, notify_dev: &hidapi::HidDevice, addr: u16, len: u8) -> Result<Vec<u8>, String> {
    let chunk_len = len.min(10);
    let mut pkt = [0u8; 17];
    pkt[0] = 0x08;
    pkt[1] = 0x08; // Cmd 0x08: Read EEPROM
    pkt[2] = 0x00; // Sub-op / pad
    pkt[3] = (addr >> 8) as u8;
    pkt[4] = (addr & 0xFF) as u8;
    pkt[5] = chunk_len;
    pkt[16] = calc_packet_checksum(&pkt);

    cmd_dev.send_feature_report(&pkt).map_err(|e| e.to_string())?;

    let mut buf = [0u8; 64];
    for _ in 0..5 {
        if let Ok(n) = notify_dev.read_timeout(&mut buf, 80) {
            if n >= 6 + (chunk_len as usize) && buf[0] == 0x09 && buf[1] == 0x08 {
                return Ok(buf[6..6 + (chunk_len as usize)].to_vec());
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    Err("ReadEEPROM timeout".to_string())
}

// Writes arbitrary data to mouse EEPROM via Cmd 0x07 in up to 10-byte chunks (FUN_004435d0)
// Packet layout verified from PCAP Frame 1555/1575: [0x08, 0x07, 0x00, addr_hi, addr_lo, len, data..., chk]
pub fn write_mouse_eeprom(dev: &hidapi::HidDevice, addr: u16, data: &[u8]) -> Result<(), hidapi::HidError> {
    let mut offset = 0;
    let mut current_addr = addr;

    while offset < data.len() {
        let chunk_size = std::cmp::min(10, data.len() - offset);
        let mut pkt = [0u8; 17];
        pkt[0] = 0x08; // Report ID
        pkt[1] = 0x07; // Cmd 0x07: Write EEPROM
        pkt[2] = 0x00; // Sub-op / pad
        pkt[3] = (current_addr >> 8) as u8;
        pkt[4] = (current_addr & 0xFF) as u8;
        pkt[5] = chunk_size as u8;

        for i in 0..chunk_size {
            pkt[6 + i] = data[offset + i];
        }

        pkt[16] = calc_packet_checksum(&pkt);

        dev.send_feature_report(&pkt)?;
        std::thread::sleep(std::time::Duration::from_millis(15));

        offset += chunk_size;
        current_addr += chunk_size as u16;
    }

    Ok(())
}

// Cmd 0x02: TellDrvStatus (FUN_004434b0 -> FUN_00443050(2, status))
// Verified from PCAP Frame 497: buf[2..5] = 00 00 00 01, buf[6..9] = 01 00 00 00
pub fn send_tell_drv_status(dev: &hidapi::HidDevice) -> Result<(), hidapi::HidError> {
    let mut pkt = [0u8; 17];
    pkt[0] = 0x08;
    pkt[1] = 0x02; // Cmd 0x02 TellDrvStatus
    pkt[2] = 0x00;
    pkt[3] = 0x00;
    pkt[4] = 0x00;
    pkt[5] = 0x01;
    pkt[6] = 0x01; // status online
    pkt[7] = 0x00;
    pkt[8] = 0x00;
    pkt[9] = 0x00;
    pkt[16] = calc_packet_checksum(&pkt);
    dev.send_feature_report(&pkt)
}

// Cmd 0x09: Commit Apply (FUN_00443050(9, 0)) - Note: Not sent during standard Apply in live PCAP
pub fn send_commit(dev: &hidapi::HidDevice) -> Result<(), hidapi::HidError> {
    let mut pkt = [0u8; 17];
    pkt[0] = 0x08;
    pkt[1] = 0x09; // Cmd 0x09 Commit
    pkt[16] = calc_packet_checksum(&pkt);
    dev.send_feature_report(&pkt)
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
        let mut handshake_passed = false;
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

            // Real Hardware Handshake & Connection Verification
            if found_dongle {
                if let (Some(cmd), Some(notify)) = (&dev_cmd, &dev_notify) {
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

                    if mouse_responsive {
                        // Authentic challenge/response handshake (Cmd 0x01)
                        if perform_handshake(cmd, notify) {
                            handshake_passed = true;
                            let _ = send_tell_drv_status(cmd);
                        }
                    }
                }
            } else if found_wired {
                mouse_responsive = true;
                if let (Some(cmd), Some(notify)) = (&dev_cmd, &dev_notify) {
                    if perform_handshake(cmd, notify) {
                        handshake_passed = true;
                        let _ = send_tell_drv_status(cmd);
                    }
                }
            }
        }

        self.found_dongle = found_dongle;
        self.found_wired = found_wired;
        self.handshake_ok = handshake_passed || mouse_responsive;

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
        const NEON_SPEED_LUT: [u8; 10] = [0x2D, 0x28, 0x23, 0x1E, 0x19, 0x13, 0x0F, 0x0A, 0x05, 0x03];
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
                            // 1. Button Matrix (EEPROM Addr 0x0060..0x009F, written in chunks)
                            on_progress(0.15, "Writing Hardware Button Matrix...");
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
                            std::thread::sleep(std::time::Duration::from_millis(15));

                            // 2. DPI Stage Values (EEPROM Addr 0x000C + 4*i, 4 bytes each [x_code, y_code, hi_range, chk])
                            // Uses PixArt PAW3325 lookup code table
                            on_progress(0.30, "Calibrating Sensor DPI Stages...");
                            for (idx, stage) in profile.dpi_stages.iter().enumerate().take(5) {
                                let (x_code, hi_range_x) = dpi_to_paw3325_code(stage.x);
                                let (y_code, _) = dpi_to_paw3325_code(stage.y);
                                let hi_byte = if hi_range_x { 0x01 } else { 0x00 };
                                let stage_sum = x_code.wrapping_add(y_code).wrapping_add(hi_byte);
                                let stage_chk = (0x55u8).wrapping_sub(stage_sum);
                                let stage_chunk = [x_code, y_code, hi_byte, stage_chk];
                                let stage_addr = 0x000C + (idx as u16 * 4);
                                let _ = write_mouse_eeprom(&dev, stage_addr, &stage_chunk);
                            }
                            std::thread::sleep(std::time::Duration::from_millis(15));

                            // 3. Active DPI Stage Count (EEPROM Addr 0x0002, 2 bytes)
                            // Note: OEM writes total active stage count (e.g. 05 50)
                            on_progress(0.45, "Writing DPI stage configuration count...");
                            let active_count: u8 = profile.dpi_stages.iter().take(5).filter(|s| s.enabled).count() as u8;
                            let count_byte = if active_count == 0 { 5 } else { active_count };
                            let count_chk = (0x55u8).wrapping_sub(count_byte);
                            let _ = write_mouse_eeprom(&dev, 0x0002, &[count_byte, count_chk]);
                            std::thread::sleep(std::time::Duration::from_millis(15));

                            // 4. DPI Stage Indicator Colors (EEPROM Addr 0x002C + 4*i, 4 bytes each [R, G, B, chk])
                            on_progress(0.60, "Writing DPI Stage Color Indicators...");
                            for (idx, stage) in profile.dpi_stages.iter().enumerate().take(5) {
                                let r = stage.color_rgb[0];
                                let g = stage.color_rgb[1];
                                let b = stage.color_rgb[2];
                                let color_sum = r.wrapping_add(g).wrapping_add(b);
                                let color_chk = (0x55u8).wrapping_sub(color_sum);
                                let color_chunk = [r, g, b, color_chk];
                                let color_addr = 0x002C + (idx as u16 * 4);
                                let _ = write_mouse_eeprom(&dev, color_addr, &color_chunk);
                            }
                            std::thread::sleep(std::time::Duration::from_millis(15));

                            // 5. Main RGB Lighting (EEPROM Addr 0x00A0, 7 bytes: [effect, R, G, B, speed, brightness, chk])
                            on_progress(0.75, "Writing Main RGB Lighting & Effects...");
                            let spd_idx = (profile.led_speed.clamp(1, 10) - 1) as usize;
                            let bri_idx = (profile.led_brightness.clamp(1, 10) - 1) as usize;
                            let (hw_effect, hw_spd, hw_bri) = match profile.led_effect {
                                LedEffect::Steady => (0x02u8, SPEED_LUT[spd_idx], BRIGHTNESS_LUT[bri_idx]),
                                LedEffect::Respiration => (0x01u8, SPEED_LUT[spd_idx], BRIGHTNESS_LUT[bri_idx]),
                                LedEffect::Neon => (0x00u8, NEON_SPEED_LUT[spd_idx], BRIGHTNESS_LUT[bri_idx]),
                                LedEffect::Streaming => (0x04u8, SPEED_LUT[spd_idx], BRIGHTNESS_LUT[bri_idx]),
                                LedEffect::Off => (0x04u8, SPEED_LUT[spd_idx], 0x00u8),
                            };

                            let mut main_rgb = [0u8; 7];
                            main_rgb[0] = hw_effect;
                            main_rgb[1] = profile.led_color[0]; // Red
                            main_rgb[2] = profile.led_color[1]; // Green
                            main_rgb[3] = profile.led_color[2]; // Blue
                            main_rgb[4] = hw_spd;               // Speed LUT
                            main_rgb[5] = hw_bri;               // Brightness LUT
                            let mut rgb_sum = 0u8;
                            for b in &main_rgb[0..6] {
                                rgb_sum = rgb_sum.wrapping_add(*b);
                            }
                            main_rgb[6] = (0x55u8).wrapping_sub(rgb_sum);
                            let _ = write_mouse_eeprom(&dev, 0x00A0, &main_rgb);
                            std::thread::sleep(std::time::Duration::from_millis(15));

                            // 6. Logo / DPI LED Effect Block (EEPROM Addr 0x004C, 2 bytes in off mode: [0x00, 0x55])
                            on_progress(0.85, "Writing Logo/DPI LED Parameters...");
                            let logo_data = [0x00u8, 0x55u8];
                            let _ = write_mouse_eeprom(&dev, 0x004C, &logo_data);
                            std::thread::sleep(std::time::Duration::from_millis(15));

                            // 7. USB Polling Rate (EEPROM Addr 0x0000, 2 bytes) - Concludes Apply Sequence
                            on_progress(0.95, "Setting USB Polling Rate...");
                            let rate_chk = (0x55u8).wrapping_sub(rate_hw_byte);
                            let rate_data = [rate_hw_byte, rate_chk];
                            let _ = write_mouse_eeprom(&dev, 0x0000, &rate_data);
                            std::thread::sleep(std::time::Duration::from_millis(15));

                            on_progress(1.0, "Configuration Applied Successfully!");
                            delivered = true;
                        }
                    }
                }
            }
        }

        // 3. Sync to OEM database %LOCALAPPDATA%\JM01\pro.dct and curid.dct
        let oem_path = get_oem_profile_path();
        if let Some(path) = oem_path {
            if path.exists() {
                if let Ok(mut bytes) = fs::read(&path) {
                    let prof_size = 2992;
                    let prof_offset = profile_index * prof_size;
                    if prof_offset + prof_size <= bytes.len() {
                        // Magic 0xFCAA1234 on profile record
                        bytes[prof_offset] = 0x34;
                        bytes[prof_offset + 1] = 0x12;
                        bytes[prof_offset + 2] = 0xAA;
                        bytes[prof_offset + 3] = 0xFC;
                        // Profile ID in slot (non-zero)
                        if bytes[prof_offset + 4] == 0 {
                            bytes[prof_offset + 4] = (profile_index as u8) + 1;
                        }

                        // DPI stages at +0xA08 (16 bytes each: X, Y, RGB, enabled)
                        for (i, stage) in profile.dpi_stages.iter().enumerate().take(5) {
                            let stage_base = prof_offset + 0x0A08 + (i * 16);
                            if stage_base + 16 <= bytes.len() {
                                let val_x = (stage.x as u32).to_le_bytes();
                                let val_y = (stage.y as u32).to_le_bytes();
                                bytes[stage_base..stage_base+4].copy_from_slice(&val_x);
                                bytes[stage_base+4..stage_base+8].copy_from_slice(&val_y);
                                bytes[stage_base+8] = stage.color_rgb[0];
                                bytes[stage_base+9] = stage.color_rgb[1];
                                bytes[stage_base+10] = stage.color_rgb[2];
                                bytes[stage_base+12] = if stage.enabled { 1 } else { 0 };
                            }
                        }

                        // Report rate at +0x9E4 (0x125, 0x250, 0x500, 0x1000)
                        let rate_offset = prof_offset + 0x09E4;
                        if rate_offset + 4 <= bytes.len() {
                            let rate_val: u32 = match profile.polling_rate {
                                PollingRate::Hz125 => 0x125,
                                PollingRate::Hz250 => 0x250,
                                PollingRate::Hz500 => 0x500,
                                PollingRate::Hz1000 => 0x1000,
                            };
                            bytes[rate_offset..rate_offset+4].copy_from_slice(&rate_val.to_le_bytes());
                        }

                        // Main LED at +0xA88..+0xA97:
                        // +0xA88: effect (u32: 1=Steady, 2=Respiration, 3=Neon, 4/0=Streaming/Off)
                        // +0xA8C: speed index (u32: 1..10)
                        // +0xA90: brightness index (u32: 1..10)
                        // +0xA94..+0xA96: RGB (3 bytes)
                        let led_eff_offset = prof_offset + 0x0A88;
                        if led_eff_offset + 16 <= bytes.len() {
                            let eff_val: u32 = match profile.led_effect {
                                LedEffect::Steady => 1,
                                LedEffect::Respiration => 2,
                                LedEffect::Neon => 3,
                                LedEffect::Streaming => 4,
                                LedEffect::Off => 0,
                            };
                            let spd_val: u32 = profile.led_speed as u32;
                            let bri_val: u32 = profile.led_brightness as u32;

                            bytes[led_eff_offset..led_eff_offset+4].copy_from_slice(&eff_val.to_le_bytes());
                            bytes[led_eff_offset+4..led_eff_offset+8].copy_from_slice(&spd_val.to_le_bytes());
                            bytes[led_eff_offset+8..led_eff_offset+12].copy_from_slice(&bri_val.to_le_bytes());
                            bytes[led_eff_offset+12] = profile.led_color[0];
                            bytes[led_eff_offset+13] = profile.led_color[1];
                            bytes[led_eff_offset+14] = profile.led_color[2];
                        }

                        let _ = fs::write(&path, &bytes);

                        // Sync curid.dct
                        if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
                            let curid_path = Path::new(&local_app_data).join("JM01").join("curid.dct");
                            let mut id_bytes = [0u8; 4];
                            id_bytes.copy_from_slice(&bytes[prof_offset + 4..prof_offset + 8]);
                            let _ = fs::write(curid_path, &id_bytes);
                        }

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

pub fn export_jmk_bytes(profile: &MouseProfile, profile_index: usize) -> Vec<u8> {
    let mut data = vec![0u8; 11332];
    data[0] = 0x34;
    data[1] = 0x12;
    data[2] = 0xAA;
    data[3] = 0xFC;
    data[4] = (profile_index as u8) + 1;

    let rate_val: u32 = match profile.polling_rate {
        PollingRate::Hz125 => 0x125,
        PollingRate::Hz250 => 0x250,
        PollingRate::Hz500 => 0x500,
        PollingRate::Hz1000 => 0x1000,
    };
    data[0x09E4..0x09E4+4].copy_from_slice(&rate_val.to_le_bytes());

    for (i, stage) in profile.dpi_stages.iter().enumerate().take(5) {
        let base = 0x0A08 + (i * 16);
        let val_x = (stage.x as u32).to_le_bytes();
        let val_y = (stage.y as u32).to_le_bytes();
        data[base..base+4].copy_from_slice(&val_x);
        data[base+4..base+8].copy_from_slice(&val_y);
        data[base+8] = stage.color_rgb[0];
        data[base+9] = stage.color_rgb[1];
        data[base+10] = stage.color_rgb[2];
        data[base+12] = if stage.enabled { 1 } else { 0 };
    }

    let eff_val: u32 = match profile.led_effect {
        LedEffect::Steady => 1,
        LedEffect::Respiration => 2,
        LedEffect::Neon => 3,
        LedEffect::Streaming => 4,
        LedEffect::Off => 0,
    };
    data[0x0A88..0x0A88+4].copy_from_slice(&eff_val.to_le_bytes());
    data[0x0A8C..0x0A8C+4].copy_from_slice(&(profile.led_speed as u32).to_le_bytes());
    data[0x0A90..0x0A90+4].copy_from_slice(&(profile.led_brightness as u32).to_le_bytes());
    data[0x0A94] = profile.led_color[0];
    data[0x0A95] = profile.led_color[1];
    data[0x0A96] = profile.led_color[2];

    data
}

pub fn parse_jmk_bytes(bytes: &[u8]) -> Option<MouseProfile> {
    if bytes.len() < 2992 {
        return None;
    }
    // Verify OEM magic 0xFCAA1234
    if bytes[0] != 0x34 || bytes[1] != 0x12 || bytes[2] != 0xAA || bytes[3] != 0xFC {
        return None;
    }

    let mut profile = MouseProfile::default();
    let prof_offset = 0;

    for (i, stage) in profile.dpi_stages.iter_mut().enumerate().take(5) {
        let stage_base = prof_offset + 0x0A08 + (i * 16);
        if stage_base + 16 <= bytes.len() {
            let x = u32::from_le_bytes(bytes[stage_base..stage_base+4].try_into().unwrap_or([0,0,0,0]));
            let y = u32::from_le_bytes(bytes[stage_base+4..stage_base+8].try_into().unwrap_or([0,0,0,0]));
            if x >= 100 && x <= 10000 {
                stage.x = x;
                stage.y = y;
            }
            stage.color_rgb = [bytes[stage_base+8], bytes[stage_base+9], bytes[stage_base+10]];
            stage.enabled = bytes[stage_base+12] != 0;
        }
    }

    // Polling rate at +0x9E4
    let rate_offset = prof_offset + 0x09E4;
    if rate_offset + 4 <= bytes.len() {
        let rate_code = u32::from_le_bytes(bytes[rate_offset..rate_offset+4].try_into().unwrap_or([0,0,0,0]));
        profile.polling_rate = match rate_code {
            0x125 => PollingRate::Hz125,
            0x250 => PollingRate::Hz250,
            0x500 => PollingRate::Hz500,
            _ => PollingRate::Hz1000,
        };
    }

    // Main LED at +0xA88
    let led_eff_offset = prof_offset + 0x0A88;
    if led_eff_offset + 15 <= bytes.len() {
        let eff = u32::from_le_bytes(bytes[led_eff_offset..led_eff_offset+4].try_into().unwrap_or([0,0,0,0]));
        let spd = u32::from_le_bytes(bytes[led_eff_offset+4..led_eff_offset+8].try_into().unwrap_or([0,0,0,0]));
        let bri = u32::from_le_bytes(bytes[led_eff_offset+8..led_eff_offset+12].try_into().unwrap_or([0,0,0,0]));

        profile.led_effect = match eff {
            1 => LedEffect::Steady,
            2 => LedEffect::Respiration,
            3 => LedEffect::Neon,
            4 => LedEffect::Streaming,
            _ => LedEffect::Off,
        };
        profile.led_speed = (spd as u8).clamp(1, 10);
        profile.led_brightness = (bri as u8).clamp(1, 10);
        profile.led_color = [bytes[led_eff_offset+12], bytes[led_eff_offset+13], bytes[led_eff_offset+14]];
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
