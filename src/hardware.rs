use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex};
use serde::{Deserialize, Serialize};

pub static IS_APPLYING: AtomicBool = AtomicBool::new(false);

pub static NOTIFY_RESPONSE: Mutex<Option<[u8; 64]>> = Mutex::new(None);
pub static NOTIFY_CONDVAR: Condvar = Condvar::new();

pub fn clear_notify_response() {
    if let Ok(mut lock) = NOTIFY_RESPONSE.lock() {
        *lock = None;
    }
}

pub fn wait_notify(expected_cmd: u8, timeout: std::time::Duration) -> Result<[u8; 64], String> {
    let mut lock = NOTIFY_RESPONSE.lock().map_err(|e| e.to_string())?;
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        if let Some(pkt) = *lock {
            if (pkt[0] == 0x09 && pkt[1] == expected_cmd) || (pkt[0] == expected_cmd) {
                *lock = None;
                return Ok(pkt);
            }
        }
        let remaining = timeout.saturating_sub(start.elapsed());
        if remaining.is_zero() {
            break;
        }
        let (new_lock, _) = NOTIFY_CONDVAR.wait_timeout(lock, remaining).map_err(|e| e.to_string())?;
        lock = new_lock;
    }
    *lock = None;
    Err(format!("Timeout waiting for response to cmd 0x{:02X}", expected_cmd))
}

pub const DEFAULT_VID: u16 = 0x25A7;
pub const DEFAULT_PID_WIRED: u16 = 0xFAB0;
pub const DEFAULT_PID_WIRELESS: u16 = 0xFA7E;

pub static ACTIVE_TARGET_PID: std::sync::atomic::AtomicU16 =
    std::sync::atomic::AtomicU16::new(DEFAULT_PID_WIRELESS);

pub const BUTTON_SLOT_MAP: [usize; 8] = [
    0,  // Button 1: Left Click (0x0060)
    1,  // Button 2: Right Click (0x0064)
    2,  // Button 3: Middle Wheel (0x0068)
    4,  // Button 4: Forward (0x0070)
    3,  // Button 5: Back (0x006C)
    8,  // Button 6: Rapid Fire / Fire Key (0x0080) - Verified via Wireshark & pro.dct
    10, // Button 7: DPI + (0x0088)
    11, // Button 8: DPI - (0x008C)
];

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
    Off = 4,
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

#[derive(Debug, Clone)]
pub enum HardwareEvent {
    DpiChanged { stage: usize },
    ProfileChanged { profile: usize },
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
    pub event_rx: std::sync::mpsc::Receiver<HardwareEvent>,
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
#[allow(dead_code)]
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
pub fn perform_handshake(cmd_dev: &hidapi::HidDevice, notify_dev: Option<&hidapi::HidDevice>) -> bool {
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

        clear_notify_response();
        if cmd_dev.send_feature_report(&pkt).is_ok() {
            if let Ok(buf) = wait_notify(0x01, std::time::Duration::from_millis(150)) {
                if buf[6] == exp0 && buf[7] == exp1 && buf[8] == exp2 && buf[9] == exp3 {
                    return true;
                }
            } else if let Some(notify) = notify_dev {
                let mut buf = [0u8; 64];
                if let Ok(n) = notify.read_timeout(&mut buf, 80) {
                    if n >= 10 && buf[0] == 0x09 && buf[1] == 0x01 {
                        if buf[6] == exp0 && buf[7] == exp1 && buf[8] == exp2 && buf[9] == exp3 {
                            return true;
                        }
                    }
                }
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    false
}

// Queries connection status (Cmd 0x03)
pub fn check_connect_status(cmd_dev: &hidapi::HidDevice, notify_dev: Option<&hidapi::HidDevice>) -> bool {
    let mut pkt = [0u8; 17];
    pkt[0] = 0x08;
    pkt[1] = 0x03; // GetConnectStatus
    pkt[16] = calc_packet_checksum(&pkt); // 0x4A

    clear_notify_response();
    if cmd_dev.send_feature_report(&pkt).is_ok() {
        if let Ok(buf) = wait_notify(0x03, std::time::Duration::from_millis(150)) {
            if buf[5] != 0 || buf[6] != 0 {
                return true;
            }
        } else if let Some(notify) = notify_dev {
            let mut buf = [0u8; 64];
            for _ in 0..3 {
                if let Ok(n) = notify.read_timeout(&mut buf, 80) {
                    if n >= 7 && buf[0] == 0x09 && buf[1] == 0x03 {
                        if buf[5] != 0 || buf[6] != 0 {
                            return true;
                        }
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
    }
    false
}

// Queries battery percentage and charging state via Cmd 0x04 (FUN_00410180 / FUN_004434e0)
// Report 0x08 Cmd 0x04 -> Device responds on Report 0x09: [0x09, 0x04, 0x00, 0x00, 0x00, 0x02, raw_level, charging_flag, 00..., chk]
pub fn query_mouse_battery(cmd_dev: &hidapi::HidDevice, notify_dev: Option<&hidapi::HidDevice>) -> Result<(u8, bool), String> {
    clear_notify_response();
    let mut pkt = [0u8; 17];
    pkt[0] = 0x08;
    pkt[1] = 0x04; // Cmd 0x04 Battery / Power query
    pkt[16] = calc_packet_checksum(&pkt); // 0x49

    cmd_dev.send_feature_report(&pkt).map_err(|e| e.to_string())?;

    if let Ok(buf) = wait_notify(0x04, std::time::Duration::from_millis(250)) {
        let raw_level = buf[6] & 0x7F;
        let charging = buf[7] != 0 || (buf[6] & 0x80 != 0);
        let percent = if raw_level <= 10 {
            (raw_level * 10).clamp(0, 100)
        } else {
            raw_level.clamp(0, 100)
        };
        return Ok((percent, charging));
    }

    if let Some(notify) = notify_dev {
        let mut buf = [0u8; 64];
        for _ in 0..3 {
            if let Ok(n) = notify.read_timeout(&mut buf, 80) {
                if n >= 8 && buf[0] == 0x09 && buf[1] == 0x04 {
                    let raw_level = buf[6] & 0x7F;
                    let charging = buf[7] != 0 || (buf[6] & 0x80 != 0);
                    let percent = if raw_level <= 10 {
                        (raw_level * 10).clamp(0, 100)
                    } else {
                        raw_level.clamp(0, 100)
                    };
                    return Ok((percent, charging));
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    Err("Query battery timeout".to_string())
}

// Reads up to 10 bytes from mouse EEPROM via Cmd 0x08 on UsagePage 0xFF02 (FUN_004438f0).
// Packet layout verified from PCAP Frame 493: [0x08, 0x08, 0x00, addr_hi, addr_lo, len, ...]
pub fn read_mouse_eeprom(cmd_dev: &hidapi::HidDevice, notify_dev: Option<&hidapi::HidDevice>, addr: u16, len: u8) -> Result<Vec<u8>, String> {
    let chunk_len = len.min(10);
    let mut pkt = [0u8; 17];
    pkt[0] = 0x08;
    pkt[1] = 0x08; // Cmd 0x08: Read EEPROM
    pkt[2] = 0x00; // Sub-op / pad
    pkt[3] = (addr >> 8) as u8;
    pkt[4] = (addr & 0xFF) as u8;
    pkt[5] = chunk_len;
    pkt[16] = calc_packet_checksum(&pkt);

    for _attempt in 0..3 {
        clear_notify_response();
        cmd_dev.send_feature_report(&pkt).map_err(|e| e.to_string())?;

        if let Ok(buf) = wait_notify(0x08, std::time::Duration::from_millis(180)) {
            if buf.len() >= 6 + (chunk_len as usize) {
                return Ok(buf[6..6 + (chunk_len as usize)].to_vec());
            }
        }

        if let Some(notify) = notify_dev {
            let mut buf = [0u8; 64];
            for _ in 0..5 {
                if let Ok(n) = notify.read_timeout(&mut buf, 80) {
                    if n >= 6 + (chunk_len as usize) && buf[0] == 0x09 && buf[1] == 0x08 {
                        return Ok(buf[6..6 + (chunk_len as usize)].to_vec());
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }

    Err("ReadEEPROM timeout".to_string())
}

// Writes arbitrary data to mouse EEPROM via Cmd 0x07 in up to 10-byte chunks (FUN_004435d0 & FUN_004427d0)
// Packet layout verified from PCAP Frame 1555/1575: [0x08, 0x07, 0x00, addr_hi, addr_lo, len, data..., chk]
// Waits for MCU ACK (09 07 00) on UsagePage 0xFF01 and retries on NACK (09 07 01) or timeout,
// followed by a 25ms settling delay (~45ms total cadence matching OemDrv.exe).
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

        for attempt in 0..3 {
            clear_notify_response();
            dev.send_feature_report(&pkt)?;
            match wait_notify(0x07, std::time::Duration::from_millis(150)) {
                Ok(resp) if resp[2] == 0x00 => {
                    break;
                }
                _ => {
                    // MCU returned 0x01 (Busy/NACK) or timed out; wait before retry
                    if attempt < 2 {
                        std::thread::sleep(std::time::Duration::from_millis(30));
                    }
                }
            }
        }

        std::thread::sleep(std::time::Duration::from_millis(25));

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
#[allow(dead_code)]
pub fn send_commit(dev: &hidapi::HidDevice) -> Result<(), hidapi::HidError> {
    let mut pkt = [0u8; 17];
    pkt[0] = 0x08;
    pkt[1] = 0x09; // Cmd 0x09 Commit
    pkt[16] = calc_packet_checksum(&pkt);
    dev.send_feature_report(&pkt)
}

static WAKE_CTX: std::sync::RwLock<Option<eframe::egui::Context>> = std::sync::RwLock::new(None);
pub static CURRENT_DPI_STAGES: std::sync::RwLock<Vec<(u32, [u8; 3])>> = std::sync::RwLock::new(Vec::new());

pub fn update_current_dpi_stages(stages: &[(u32, [u8; 3])]) {
    if let Ok(mut lock) = CURRENT_DPI_STAGES.write() {
        *lock = stages.to_vec();
    }
}

pub fn set_wake_context(ctx: eframe::egui::Context) {
    if let Ok(mut lock) = WAKE_CTX.write() {
        *lock = Some(ctx);
    }
}

// Persistent reader thread on UsagePage 0xFF01 (FUN_004428e0)
// Listens for asynchronous MCU push events (Cmd 0x0A):
// - Bit 0 (0x01): Physical DPI button pressed -> buf[7] = stage index (0..4)
// - Bit 1 (0x02): Profile switch button pressed -> buf[8] = profile index (0..4)
pub fn start_notification_listener(
    vid: u16,
    event_tx: std::sync::mpsc::Sender<HardwareEvent>,
    osd: crate::overlay::OsdController,
) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name("hid_notify_listener".to_string())
        .spawn(move || {
            let mut api_opt = hidapi::HidApi::new().ok();
            let mut current_dev: Option<hidapi::HidDevice> = None;
            let mut opened_pid: u16 = 0;
            let mut buf = [0u8; 64];

            loop {
                // IMPORTANT: Do NOT pause when IS_APPLYING is true!
                // The MCU sends 09 07 00 ACKs on UsagePage 0xFF01 after every 08 07 EEPROM write chunk.
                // This listener MUST stay active to deliver those ACKs via NOTIFY_CONDVAR.
                let target_pid = ACTIVE_TARGET_PID.load(Ordering::SeqCst);
                if current_dev.is_some() && opened_pid != target_pid {
                    current_dev = None;
                }

                if current_dev.is_none() {
                    if let Some(api) = &mut api_opt {
                        let _ = api.refresh_devices();
                        for dev_info in api.device_list() {
                            if dev_info.vendor_id() == vid
                                && dev_info.product_id() == target_pid
                                && dev_info.usage_page() == 0xFF01
                            {
                                if let Ok(d) = dev_info.open_device(api) {
                                    current_dev = Some(d);
                                    opened_pid = target_pid;
                                    break;
                                }
                            }
                        }
                        if current_dev.is_none() {
                            for dev_info in api.device_list() {
                                if dev_info.vendor_id() == vid && dev_info.usage_page() == 0xFF01 {
                                    if let Ok(d) = dev_info.open_device(api) {
                                        current_dev = Some(d);
                                        opened_pid = dev_info.product_id();
                                        break;
                                    }
                                }
                            }
                        }
                    } else {
                        api_opt = hidapi::HidApi::new().ok();
                    }

                    if current_dev.is_none() {
                        std::thread::sleep(std::time::Duration::from_millis(800));
                        continue;
                    }
                }

                if let Some(dev) = &current_dev {
                    match dev.read_timeout(&mut buf, 150) {
                        Ok(n) if n >= 3 => {
                            let (cmd, flags, dpi_raw, prof_raw) = if buf[0] == 0x09 {
                                (buf[1], buf[6], buf[7], buf[8])
                            } else if buf[0] == 0x0A {
                                (buf[0], buf[5], buf[6], buf[7])
                            } else {
                                (buf[0], 0, 0, 0)
                            };

                            if cmd == 0x0A {
                                // Bit 0 (0x01): DPI level changed
                                if flags & 0x01 != 0 {
                                    let stage = ((dpi_raw & 0x0F) as usize) + 1;

                                    // Look up active DPI value and color from cache
                                    let (dpi_val, color) = if let Ok(lock) = CURRENT_DPI_STAGES.read() {
                                        if let Some(&(val, clr)) = lock.get(stage.saturating_sub(1)) {
                                            (val, clr)
                                        } else {
                                            (stage as u32 * 1000, [0, 255, 255])
                                        }
                                    } else {
                                        (1000, [0, 255, 255])
                                    };

                                    // 1. Instantly trigger native desktop Win32 OSD overlay on screen!
                                    osd.show_dpi(stage, dpi_val, color);

                                    // 2. Notify UI thread to update slider and logs
                                    let _ = event_tx.send(HardwareEvent::DpiChanged { stage });
                                    if let Ok(lock) = WAKE_CTX.read() {
                                        if let Some(ctx) = &*lock {
                                            ctx.request_repaint();
                                        }
                                    }
                                }
                                // Bit 1 (0x02): Profile switch
                                if flags & 0x02 != 0 {
                                    let profile = ((prof_raw & 0x0F) as usize) + 1;
                                    let _ = event_tx.send(HardwareEvent::ProfileChanged { profile });
                                    if let Ok(lock) = WAKE_CTX.read() {
                                        if let Some(ctx) = &*lock {
                                            ctx.request_repaint();
                                        }
                                    }
                                }
                            } else if buf[0] == 0x09 || buf[0] == 0x08 {
                                // Synchronous command response from device (0x01, 0x03, 0x04, 0x07, 0x08, etc.)
                                if let Ok(mut lock) = NOTIFY_RESPONSE.lock() {
                                    *lock = Some(buf);
                                    NOTIFY_CONDVAR.notify_all();
                                }
                            }
                        }
                        Ok(_) => {
                            // normal read timeout, keep listening
                        }
                        Err(_) => {
                            // Device disconnected or asleep
                            current_dev = None;
                            std::thread::sleep(std::time::Duration::from_millis(500));
                        }
                    }
                }
            }
        })
        .expect("Failed to spawn hid_notify_listener thread")
}

impl HardwareBridge {
    pub fn new(osd: crate::overlay::OsdController) -> Self {
        let (event_tx, event_rx) = std::sync::mpsc::channel();
        start_notification_listener(DEFAULT_VID, event_tx, osd);
        std::thread::sleep(std::time::Duration::from_millis(30));
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
            event_rx,
        };
        bridge.detect();
        bridge
    }

    pub fn detect(&mut self) -> bool {
        if IS_APPLYING.load(Ordering::SeqCst) {
            return self.connected;
        }

        let mut found_wired = false;
        let mut found_dongle = false;
        let mut mouse_responsive = false;
        let mut handshake_passed = self.handshake_ok;
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
                }
            }

            let target_pid = if found_wired {
                DEFAULT_PID_WIRED
            } else {
                DEFAULT_PID_WIRELESS
            };
            ACTIVE_TARGET_PID.store(target_pid, Ordering::SeqCst);

            for device in api.device_list() {
                if device.vendor_id() == DEFAULT_VID && device.product_id() == target_pid {
                    if device.usage_page() == 0xFF02 && dev_cmd.is_none() {
                        dev_cmd = device.open_device(api).ok();
                    }
                    if !self.handshake_ok && device.usage_page() == 0xFF01 && dev_notify.is_none() {
                        dev_notify = device.open_device(api).ok();
                    }
                }
            }

            // Real Hardware Handshake & Connection Verification
            if found_dongle && !found_wired {
                if let Some(cmd) = &dev_cmd {
                    if !self.handshake_ok {
                        let is_online = check_connect_status(cmd, dev_notify.as_ref());
                        if is_online {
                            mouse_responsive = true;
                            if perform_handshake(cmd, dev_notify.as_ref()) {
                                handshake_passed = true;
                                let _ = send_tell_drv_status(cmd);
                            }
                        }
                    } else {
                        // Already authenticated and connected
                        mouse_responsive = true;
                        handshake_passed = true;
                    }
                }
            } else if found_wired {
                mouse_responsive = true;
                if let Some(cmd) = &dev_cmd {
                    if !self.handshake_ok {
                        if perform_handshake(cmd, dev_notify.as_ref()) {
                            handshake_passed = true;
                            let _ = send_tell_drv_status(cmd);
                        }
                    } else {
                        handshake_passed = true;
                    }
                }
            }

            // Live Battery Query via authentic Cmd 0x04
            if mouse_responsive {
                if let Some(cmd) = &dev_cmd {
                    if let Ok((bat, chg)) = query_mouse_battery(cmd, dev_notify.as_ref()) {
                        if bat > 0 {
                            live_battery = bat;
                            self.is_charging = chg;
                        }
                    }
                }
            }
        }

        self.found_dongle = found_dongle;
        self.found_wired = found_wired;
        self.handshake_ok = handshake_passed;

        if !found_dongle && !found_wired {
            self.handshake_ok = false;
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
            self.last_status_detail = "Wired USB cable connected and active.".to_string();
            true
        } else if found_dongle && mouse_responsive {
            self.connected = true;
            self.is_wireless = true;
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

    #[allow(dead_code)]
    pub fn apply_profile(&self, profile: &MouseProfile, profile_index: usize) -> Result<(), String> {
        Self::apply_profile_direct(self.vid, profile, profile_index, &[], |_, _| {})
    }

    #[allow(dead_code)]
    pub fn apply_profile_with_progress<F: Fn(f32, &str)>(&self, profile: &MouseProfile, profile_index: usize, on_progress: F) -> Result<(), String> {
        Self::apply_profile_direct(self.vid, profile, profile_index, &[], on_progress)
    }

    pub fn apply_profile_direct<F: Fn(f32, &str)>(
        vid: u16,
        profile: &MouseProfile,
        profile_index: usize,
        macros: &[MacroItem],
        on_progress: F,
    ) -> Result<(), String> {
        IS_APPLYING.store(true, Ordering::SeqCst);
        let res = Self::apply_profile_direct_inner(vid, profile, profile_index, macros, on_progress);
        IS_APPLYING.store(false, Ordering::SeqCst);
        res
    }

    fn apply_profile_direct_inner<F: Fn(f32, &str)>(
        vid: u16,
        profile: &MouseProfile,
        profile_index: usize,
        macros: &[MacroItem],
        on_progress: F,
    ) -> Result<(), String> {
        const SPEED_LUT: [u8; 10] = [0xFF, 0xE6, 0xD2, 0xBE, 0xAA, 0x96, 0x82, 0x6E, 0x46, 0x28];
        const NEON_SPEED_LUT: [u8; 10] = [0x2D, 0x28, 0x23, 0x1E, 0x19, 0x13, 0x0F, 0x0A, 0x05, 0x03];
        const BRIGHTNESS_LUT: [u8; 10] = [0x19, 0x32, 0x4B, 0x64, 0x7D, 0x96, 0xAF, 0xC8, 0xE1, 0xFF];

        let mut delivered = false;
        let oem_macros = if macros.is_empty() {
            load_oem_macros()
        } else {
            macros.to_vec()
        };

        on_progress(0.05, "Connecting to Mouse MCU...");

        if let Ok(api) = hidapi::HidApi::new() {
            // Polling rate code: 1000Hz -> 0x01, 500Hz -> 0x02, 250Hz -> 0x04, 125Hz -> 0x08
            let rate_hw_byte = match profile.polling_rate {
                PollingRate::Hz1000 => 0x01,
                PollingRate::Hz500 => 0x02,
                PollingRate::Hz250 => 0x04,
                PollingRate::Hz125 => 0x08,
            };

            // Prioritize Wired USB (0xFAB0) over Wireless Dongle (0xFA7E), matching detect() & read_device_config()
            let mut target_dev: Option<(hidapi::HidDevice, u16)> = None;
            for dev_info in api.device_list() {
                if dev_info.vendor_id() == vid
                    && dev_info.product_id() == DEFAULT_PID_WIRED
                    && dev_info.usage_page() == 0xFF02
                {
                    if let Ok(d) = dev_info.open_device(&api) {
                        target_dev = Some((d, DEFAULT_PID_WIRED));
                        break;
                    }
                }
            }
            if target_dev.is_none() {
                for dev_info in api.device_list() {
                    if dev_info.vendor_id() == vid
                        && dev_info.product_id() == DEFAULT_PID_WIRELESS
                        && dev_info.usage_page() == 0xFF02
                    {
                        if let Ok(d) = dev_info.open_device(&api) {
                            target_dev = Some((d, DEFAULT_PID_WIRELESS));
                            break;
                        }
                    }
                }
            }

            if let Some((dev, chosen_pid)) = target_dev {
                ACTIVE_TARGET_PID.store(chosen_pid, Ordering::SeqCst);

                // Query connection status (08 03) before EEPROM write burst (matches Wireshark Frames 7322, 9868, 15386)
                let _ = check_connect_status(&dev, None);
                std::thread::sleep(std::time::Duration::from_millis(25));

                // 1. Button Matrix (EEPROM Addr 0x0060..0x009F, 16 slots of 4 bytes each)
                // Authentic layout verified against Wireshark OEM PCAP & pro.dct:
                // Slot 0  (0x60): Button 1 - Left Click (01 01 00 53)
                // Slot 1  (0x64): Button 2 - Right Click (01 02 00 52)
                // Slot 2  (0x68): Button 3 - Middle Wheel (01 04 00 50)
                // Slot 3  (0x6C): Button 5 - Back (01 08 00 4C)
                // Slot 4  (0x70): Button 4 - Forward (01 10 00 44)
                // Slot 6  (0x78): Physical Bottom Profile Switch Button (09 00 00 4C)
                // Slot 8  (0x80): Button 6 - Rapid Fire / Fire Key (04 32 03 1C or Macro 06 08 FE 49)
                // Slot 10 (0x88): Button 7 - DPI + (02 02 00 51)
                // Slot 11 (0x8C): Button 8 - DPI - (02 03 00 50)
                on_progress(0.15, "Writing Hardware Button Matrix...");
                let mut matrix = [0u8; 64];
                for k in 0..16 {
                    matrix[k * 4] = 0x00;
                    matrix[k * 4 + 1] = 0x00;
                    matrix[k * 4 + 2] = 0x00;
                    matrix[k * 4 + 3] = 0x55;
                }
                // Slot 6 (0x0078) is the physical bottom Profile Switch button (FUN_0040b770 line 1236)
                matrix[6 * 4] = 0x09;
                matrix[6 * 4 + 1] = 0x00;
                matrix[6 * 4 + 2] = 0x00;
                matrix[6 * 4 + 3] = 0x4C;

                for (btn_idx, b_action) in profile.button_actions.iter().enumerate().take(8) {
                    let slot = BUTTON_SLOT_MAP[btn_idx];
                    let mut combo_payload: Option<Vec<u8>> = None;
                    let mut macro_payload: Option<Vec<u8>> = None;

                    let (b0, b1, b2): (u8, u8, u8) = if let Some(code) = multimedia_to_consumer_code(b_action.as_str()) {
                        let mut buf = vec![0u8; 10];
                        buf[0] = 2; // 2 events (down, up)
                        buf[1] = 0x82; // Consumer Key Down
                        buf[2] = (code & 0xFF) as u8;
                        buf[3] = (code >> 8) as u8;
                        buf[4] = 0x42; // Consumer Key Up
                        buf[5] = (code & 0xFF) as u8;
                        buf[6] = (code >> 8) as u8;
                        let sum: u8 = buf[0..7].iter().fold(0u8, |acc, &x| acc.wrapping_add(x));
                        buf[7] = (0x55u8).wrapping_sub(sum);
                        combo_payload = Some(buf);
                        (0x05, 0x00, 0x00)
                    } else if b_action.starts_with("Combo:") || b_action.starts_with("Key combination") {
                        combo_payload = Some(encode_combo_eeprom_payload(b_action.as_str()));
                        (0x05, 0x00, 0x00)
                    } else if b_action.starts_with("Macro") {
                        let macro_name = b_action
                            .trim_start_matches("Macro - ")
                            .trim_start_matches("Macro: ")
                            .trim_start_matches("Macro ")
                            .trim();
                        let found_macro = oem_macros
                            .iter()
                            .find(|m| m.name.eq_ignore_ascii_case(macro_name))
                            .cloned()
                            .or_else(|| oem_macros.first().cloned())
                            .unwrap_or_else(|| MacroItem {
                                name: if macro_name.is_empty() { "Macro 1".to_string() } else { macro_name.to_string() },
                                key_events: vec![
                                    "MouseDown: Left (20ms)".to_string(),
                                    "MouseUp: Left (20ms)".to_string(),
                                ],
                            });
                        macro_payload = Some(encode_macro_eeprom_payload(&found_macro));
                        // Verified from FUN_0040b770 & Wireshark Frame 15608: b1 is hardware slot index (e.g. 0x08 for Slot 8)
                        (0x06, slot as u8, 0xFE)
                    } else {
                        // Verified from FUN_0040acc0 (dec1.c lines 1837-1917) & Wireshark PCAP
                        match b_action.as_str() {
                            "Left button" => (0x01, 0x01, 0x00),
                            "Right button" => (0x01, 0x02, 0x00),
                            "Middle button" => (0x01, 0x04, 0x00),
                            "Forward" => (0x01, 0x10, 0x00),
                            "Back" => (0x01, 0x08, 0x00),
                            "Three click" => (0x04, 0x32, 0x03),
                            "Fire key" | "Rapid Fire" => (0x04, 0x32, 0x03),
                            "DPI +" => (0x02, 0x02, 0x00),
                            "DPI -" => (0x02, 0x03, 0x00),
                            "DPI Loop" => (0x02, 0x01, 0x00),
                            "RGB ON/OFF" | "RGB Switch" => (0x08, 0x00, 0x00),
                            "Polling rate switch" => (0x07, 0x00, 0x00),
                            "Profile Switch" => (0x09, 0x00, 0x00),
                            "Disable" => (0x00, 0x00, 0x00),
                            _ => match btn_idx {
                                0 => (0x01, 0x01, 0x00),
                                1 => (0x01, 0x02, 0x00),
                                2 => (0x01, 0x04, 0x00),
                                3 => (0x01, 0x10, 0x00),
                                4 => (0x01, 0x08, 0x00),
                                5 => (0x04, 0x32, 0x03),
                                6 => (0x02, 0x02, 0x00),
                                7 => (0x02, 0x03, 0x00),
                                _ => (0x00, 0x00, 0x00),
                            },
                        }
                    };

                    if let Some(payload) = combo_payload {
                        let combo_addr = 0x0100 + (slot as u16) * 0x20;
                        let _ = write_mouse_eeprom(&dev, combo_addr, &payload);
                    }

                    if let Some(payload) = macro_payload {
                        // Verified from FUN_0040b450 & Wireshark Frame 15500: Macro EEPROM base is 0x0300 + slot * 0x0180
                        let macro_addr = 0x0300 + (slot as u16) * 0x0180;
                        let _ = write_mouse_eeprom(&dev, macro_addr, &payload);
                    }

                    let btn_chk = (0x55u8).wrapping_sub(b0.wrapping_add(b1).wrapping_add(b2));
                    matrix[slot * 4] = b0;
                    matrix[slot * 4 + 1] = b1;
                    matrix[slot * 4 + 2] = b2;
                    matrix[slot * 4 + 3] = btn_chk;
                }
                for chunk_idx in 0..8 {
                    let chunk_addr = 0x0060 + (chunk_idx * 8) as u16;
                    let chunk_data = &matrix[chunk_idx * 8..(chunk_idx + 1) * 8];
                    let _ = write_mouse_eeprom(&dev, chunk_addr, chunk_data);
                }

                // 2. DPI Stage Values (EEPROM Addr 0x000C, 20 bytes in chunks of 8, 8, 4 bytes)
                // Uses PixArt PAW3325 lookup code table
                on_progress(0.30, "Calibrating Sensor DPI Stages...");
                let mut dpi_stages_data = [0u8; 20];
                for (idx, stage) in profile.dpi_stages.iter().enumerate().take(5) {
                    let (x_code, hi_range_x) = dpi_to_paw3325_code(stage.x);
                    let (y_code, _) = dpi_to_paw3325_code(stage.y);
                    let hi_byte = if hi_range_x { 0x01 } else { 0x00 };
                    let stage_sum = x_code.wrapping_add(y_code).wrapping_add(hi_byte);
                    let stage_chk = (0x55u8).wrapping_sub(stage_sum);
                    let off = idx * 4;
                    dpi_stages_data[off] = x_code;
                    dpi_stages_data[off + 1] = y_code;
                    dpi_stages_data[off + 2] = hi_byte;
                    dpi_stages_data[off + 3] = stage_chk;
                }
                let _ = write_mouse_eeprom(&dev, 0x000C, &dpi_stages_data[0..8]);
                let _ = write_mouse_eeprom(&dev, 0x0014, &dpi_stages_data[8..16]);
                let _ = write_mouse_eeprom(&dev, 0x001C, &dpi_stages_data[16..20]);

                // 3. Active DPI Stage Count (EEPROM Addr 0x0002, 2 bytes)
                on_progress(0.45, "Writing DPI stage configuration count...");
                let active_count: u8 = profile.dpi_stages.iter().take(5).filter(|s| s.enabled).count() as u8;
                let count_byte = if active_count == 0 { 5 } else { active_count };
                let count_chk = (0x55u8).wrapping_sub(count_byte);
                let _ = write_mouse_eeprom(&dev, 0x0002, &[count_byte, count_chk]);

                // 4. DPI Stage Indicator Colors (EEPROM Addr 0x002C, 20 bytes in chunks of 8, 8, 4 bytes)
                on_progress(0.60, "Writing DPI Stage Color Indicators...");
                let mut dpi_colors_data = [0u8; 20];
                for (idx, stage) in profile.dpi_stages.iter().enumerate().take(5) {
                    let r = stage.color_rgb[0];
                    let g = stage.color_rgb[1];
                    let b = stage.color_rgb[2];
                    let color_sum = r.wrapping_add(g).wrapping_add(b);
                    let color_chk = (0x55u8).wrapping_sub(color_sum);
                    let off = idx * 4;
                    dpi_colors_data[off] = r;
                    dpi_colors_data[off + 1] = g;
                    dpi_colors_data[off + 2] = b;
                    dpi_colors_data[off + 3] = color_chk;
                }
                let _ = write_mouse_eeprom(&dev, 0x002C, &dpi_colors_data[0..8]);
                let _ = write_mouse_eeprom(&dev, 0x0034, &dpi_colors_data[8..16]);
                let _ = write_mouse_eeprom(&dev, 0x003C, &dpi_colors_data[16..20]);

                // 5. Main RGB Lighting (EEPROM Addr 0x00A0, 7 bytes: [effect, R, G, B, speed, brightness, chk])
                on_progress(0.75, "Writing Main RGB Lighting & Effects...");
                let spd_idx = (profile.led_speed.clamp(1, 10) - 1) as usize;
                let bri_idx = (profile.led_brightness.clamp(1, 10) - 1) as usize;
                let (hw_effect, hw_spd, hw_bri) = match profile.led_effect {
                    LedEffect::Steady => (0x02u8, SPEED_LUT[spd_idx], BRIGHTNESS_LUT[bri_idx]),
                    LedEffect::Respiration => (0x01u8, SPEED_LUT[spd_idx], BRIGHTNESS_LUT[bri_idx]),
                    LedEffect::Neon => (0x00u8, NEON_SPEED_LUT[spd_idx], BRIGHTNESS_LUT[bri_idx]),
                    LedEffect::Off => (0x04u8, 0x00u8, 0x00u8),
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

                // 6. Logo / DPI LED Effect Block (EEPROM Addr 0x004C)
                // Off: 2 bytes [0x00, 0x55]; Steady/Respiration: 6 bytes [eff, 0x55-eff, bri, 0x55-bri, clr_idx, 0x55-clr_idx]
                on_progress(0.85, "Writing Logo/DPI LED Parameters...");
                match profile.dpi_effect {
                    DpiEffect::Off => {
                        let logo_data = [0x00u8, 0x55u8];
                        let _ = write_mouse_eeprom(&dev, 0x004C, &logo_data);
                    }
                    DpiEffect::Steady => {
                        let eff = 0x01u8;
                        let eff_chk = (0x55u8).wrapping_sub(eff);
                        let bri = 0xFFu8;
                        let bri_chk = (0x55u8).wrapping_sub(bri);
                        let clr_idx = (profile.current_stage as u8).clamp(1, 5) - 1;
                        let clr_chk = (0x55u8).wrapping_sub(clr_idx);
                        let logo_data = [eff, eff_chk, bri, bri_chk, clr_idx, clr_chk];
                        let _ = write_mouse_eeprom(&dev, 0x004C, &logo_data);
                    }
                    DpiEffect::Respiration => {
                        let eff = 0x02u8;
                        let eff_chk = (0x55u8).wrapping_sub(eff);
                        let bri = 0xFFu8;
                        let bri_chk = (0x55u8).wrapping_sub(bri);
                        let clr_idx = (profile.current_stage as u8).clamp(1, 5) - 1;
                        let clr_chk = (0x55u8).wrapping_sub(clr_idx);
                        let logo_data = [eff, eff_chk, bri, bri_chk, clr_idx, clr_chk];
                        let _ = write_mouse_eeprom(&dev, 0x004C, &logo_data);
                    }
                }

                // 7. USB Polling Rate (EEPROM Addr 0x0000, 2 bytes) - Concludes EEPROM Apply Sequence
                on_progress(0.95, "Setting USB Polling Rate...");
                let rate_chk = (0x55u8).wrapping_sub(rate_hw_byte);
                let rate_data = [rate_hw_byte, rate_chk];
                let _ = write_mouse_eeprom(&dev, 0x0000, &rate_data);

                // 8. Apply Windows Mouse Parameter Tuning (Sensitivity, Precision, Scroll Speed, Double-click)
                apply_mouse_parameters(profile);

                on_progress(1.0, "Configuration Applied Successfully!");
                delivered = true;
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

                        // Sync Key[0..7] at +0x0264 (40 bytes per key) and Macro names at +0x03A4 (64 bytes per key)
                        for (btn_idx, b_action) in profile.button_actions.iter().enumerate().take(8) {
                            let key_base = prof_offset + 0x0264 + (btn_idx * 40);
                            if key_base + 40 <= bytes.len() {
                                let slot = BUTTON_SLOT_MAP[btn_idx];
                                let oem_code: u32 = match b_action.as_str() {
                                    "Left button" => 0x01000011,
                                    "Right button" => 0x01000013,
                                    "Middle button" => 0x01000012,
                                    "Forward" => 0x01000014,
                                    "Back" => 0x01000015,
                                    "Three click" => 0x01000017,
                                    "Fire key" | "Rapid Fire" => 0x080000A4,
                                    "DPI +" => 0x080000A8,
                                    "DPI -" => 0x080000A9,
                                    "DPI Loop" => 0x080000AA,
                                    "RGB ON/OFF" | "RGB Switch" => 0x080000A7,
                                    "Polling rate switch" => 0x080000AC,
                                    "Profile Switch" => 0x080000B0,
                                    "Disable" => 0x080000A2,
                                    s if s.starts_with("Macro") => 0x05000000,
                                    s if s.starts_with("Combo:") || s.starts_with("Key combination") => 0x03000000,
                                    s if multimedia_to_consumer_code(s).is_some() => 0x02000000,
                                    _ => 0x01000011,
                                };
                                bytes[key_base..key_base + 4].copy_from_slice(&oem_code.to_le_bytes());
                                let matrix_inx_plus_1 = (slot as u32) + 1;
                                bytes[key_base + 20..key_base + 24].copy_from_slice(&matrix_inx_plus_1.to_le_bytes());

                                if b_action.starts_with("Macro") {
                                    let m_name = b_action
                                        .trim_start_matches("Macro - ")
                                        .trim_start_matches("Macro: ")
                                        .trim_start_matches("Macro ")
                                        .trim();
                                    let mac_name_base = prof_offset + 0x03A4 + (btn_idx * 64);
                                    if mac_name_base + 64 <= bytes.len() {
                                        for b in &mut bytes[mac_name_base..mac_name_base + 64] {
                                            *b = 0;
                                        }
                                        for (ci, ch) in m_name.encode_utf16().take(30).enumerate() {
                                            let le = ch.to_le_bytes();
                                            bytes[mac_name_base + ci * 2] = le[0];
                                            bytes[mac_name_base + ci * 2 + 1] = le[1];
                                        }
                                    }
                                }
                            }
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
                        // +0xA88: effect (u32: 1=Steady, 2=Respiration, 3=Neon, 0=Off)
                        // +0xA8C: speed index (u32: 1..10)
                        // +0xA90: brightness index (u32: 1..10)
                        // +0xA94..+0xA96: RGB (3 bytes)
                        let led_eff_offset = prof_offset + 0x0A88;
                        if led_eff_offset + 16 <= bytes.len() {
                            let eff_val: u32 = match profile.led_effect {
                                LedEffect::Steady => 1,
                                LedEffect::Respiration => 2,
                                LedEffect::Neon => 3,
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

    // Active stage index from +0x09F0
    let cur_stg_offset = prof_offset + 0x09F0;
    if cur_stg_offset + 4 <= bytes.len() {
        let cur_stg = u32::from_le_bytes(bytes[cur_stg_offset..cur_stg_offset+4].try_into().unwrap_or([0,0,0,0]));
        if cur_stg >= 1 && cur_stg <= 5 {
            profile.current_stage = cur_stg as usize;
        }
    }

    for (i, stage) in profile.dpi_stages.iter_mut().enumerate().take(5) {
        let stage_base = prof_offset + 0x0A08 + (i * 16);
        if stage_base + 16 <= bytes.len() {
            let x = u32::from_le_bytes(bytes[stage_base..stage_base+4].try_into().unwrap_or([0,0,0,0]));
            let y = u32::from_le_bytes(bytes[stage_base+4..stage_base+8].try_into().unwrap_or([0,0,0,0]));
            if x >= 100 && x <= 10000 {
                stage.x = x;
                stage.y = y;
                stage.enabled = true;
            }
            stage.color_rgb = [bytes[stage_base+8], bytes[stage_base+9], bytes[stage_base+10]];
            if bytes[stage_base+12] != 0 {
                stage.enabled = true;
            }
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

#[cfg(windows)]
pub fn apply_mouse_parameters(profile: &MouseProfile) {
    use windows::Win32::UI::WindowsAndMessaging::{
        SystemParametersInfoW,
        SPI_SETMOUSESPEED, SPI_SETMOUSE, SPI_SETWHEELSCROLLLINES,
        SPIF_UPDATEINIFILE, SPIF_SENDCHANGE,
    };

    extern "system" {
        fn SetDoubleClickTime(u_interval: u32) -> windows::Win32::Foundation::BOOL;
    }

    unsafe {
        // 1. Mouse sensitivity (1..20)
        let speed = profile.mouse_sensitivity.clamp(1, 20);
        let _ = SystemParametersInfoW(
            SPI_SETMOUSESPEED,
            0,
            Some(speed as usize as *mut _),
            SPIF_UPDATEINIFILE | SPIF_SENDCHANGE,
        );

        // 2. Enhance pointer precision ([6, 10, 1] if true, [0, 0, 0] if false)
        let mut mouse_params: [i32; 3] = if profile.enhance_pointer_precision {
            [6, 10, 1]
        } else {
            [0, 0, 0]
        };
        let _ = SystemParametersInfoW(
            SPI_SETMOUSE,
            0,
            Some(mouse_params.as_mut_ptr() as *mut _),
            SPIF_UPDATEINIFILE | SPIF_SENDCHANGE,
        );

        // 3. Wheel scroll lines (u32::MAX for page scroll, else 1..10)
        let scroll_lines: u32 = if profile.scroll_one_page {
            u32::MAX
        } else {
            profile.scroll_speed.clamp(1, 10)
        };
        let _ = SystemParametersInfoW(
            SPI_SETWHEELSCROLLLINES,
            scroll_lines,
            None,
            SPIF_UPDATEINIFILE | SPIF_SENDCHANGE,
        );

        // 4. Double click time (slider 1..10 -> 900ms slow to ~200ms fast)
        let dbl_click_ms = 900u32.saturating_sub((profile.double_click_speed.clamp(1, 10) - 1) * 78);
        let _ = SetDoubleClickTime(dbl_click_ms);
    }
}

#[cfg(target_os = "linux")]
pub fn apply_mouse_parameters(profile: &MouseProfile) {
    let accel = if profile.enhance_pointer_precision { "2/1" } else { "1/1" };
    let threshold = if profile.enhance_pointer_precision { "4" } else { "0" };
    let _ = std::process::Command::new("xset")
        .args(["m", accel, threshold])
        .output();
}

#[cfg(target_os = "macos")]
pub fn apply_mouse_parameters(profile: &MouseProfile) {
    let scaling = (profile.mouse_sensitivity.clamp(1, 20) as f32) / 6.0;
    let _ = std::process::Command::new("defaults")
        .args(["write", "-g", "com.apple.mouse.scaling", "-float", &format!("{:.2}", scaling)])
        .output();
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
pub fn apply_mouse_parameters(_profile: &MouseProfile) {}

pub fn reverse_lut(lut: &[u8], val: u8) -> u8 {
    let mut best_idx = 0;
    let mut min_diff = u8::MAX;
    for (i, &lut_val) in lut.iter().enumerate() {
        let diff = if val > lut_val { val - lut_val } else { lut_val - val };
        if diff < min_diff {
            min_diff = diff;
            best_idx = i;
        }
    }
    (best_idx as u8) + 1
}

pub fn multimedia_to_consumer_code(action: &str) -> Option<u16> {
    match action {
        "Media Player" => Some(0x0183),
        "Play/Pause" => Some(0x00CD),
        "Next" => Some(0x00B5),
        "Previous" => Some(0x00B6),
        "Stop playing" => Some(0x00B7),
        "Mute" => Some(0x00E2),
        "Volume up" => Some(0x00E9),
        "Volume down" => Some(0x00EA),
        "Email" => Some(0x018A),
        "Calculator" => Some(0x0192),
        "My Computer" => Some(0x0194),
        "Home page" => Some(0x0223),
        "Search" => Some(0x0221),
        "WWW Forward" => Some(0x0225),
        "WWW Back" => Some(0x0224),
        "WWW Stop" => Some(0x0226),
        "WWW Refresh" => Some(0x0227),
        "Favorites" => Some(0x022A),
        _ => None,
    }
}

pub fn decode_consumer_code(code: u16) -> String {
    match code {
        0x0183 => "Media Player".to_string(),
        0x00CD => "Play/Pause".to_string(),
        0x00B5 => "Next".to_string(),
        0x00B6 => "Previous".to_string(),
        0x00B7 => "Stop playing".to_string(),
        0x00E2 => "Mute".to_string(),
        0x00E9 => "Volume up".to_string(),
        0x00EA => "Volume down".to_string(),
        0x018A => "Email".to_string(),
        0x0192 => "Calculator".to_string(),
        0x0194 => "My Computer".to_string(),
        0x0223 => "Home page".to_string(),
        0x0221 => "Search".to_string(),
        0x0225 => "WWW Forward".to_string(),
        0x0224 => "WWW Back".to_string(),
        0x0226 => "WWW Stop".to_string(),
        0x0227 => "WWW Refresh".to_string(),
        0x022A => "Favorites".to_string(),
        _ => format!("Consumer (0x{:04X})", code),
    }
}

pub fn key_name_to_hid(key_part: &str) -> u8 {
    match key_part.trim().to_ascii_uppercase().as_str() {
        "A" => 0x04, "B" => 0x05, "C" => 0x06, "D" => 0x07, "E" => 0x08,
        "F" => 0x09, "G" => 0x0A, "H" => 0x0B, "I" => 0x0C, "J" => 0x0D,
        "K" => 0x0E, "L" => 0x0F, "M" => 0x10, "N" => 0x11, "O" => 0x12,
        "P" => 0x13, "Q" => 0x14, "R" => 0x15, "S" => 0x16, "T" => 0x17,
        "U" => 0x18, "V" => 0x19, "W" => 0x1A, "X" => 0x1B, "Y" => 0x1C,
        "Z" => 0x1D,
        "1" | "NUM1" => 0x1E, "2" | "NUM2" => 0x1F, "3" | "NUM3" => 0x20,
        "4" | "NUM4" => 0x21, "5" | "NUM5" => 0x22, "6" | "NUM6" => 0x23,
        "7" | "NUM7" => 0x24, "8" | "NUM8" => 0x25, "9" | "NUM9" => 0x26,
        "0" | "NUM0" => 0x27,
        "ENTER" | "RETURN" => 0x28,
        "ESCAPE" | "ESC" => 0x29,
        "BACKSPACE" => 0x2A,
        "TAB" => 0x2B,
        "SPACE" => 0x2C,
        "MINUS" | "-" => 0x2D,
        "EQUALS" | "PLUS" | "=" => 0x2E,
        "F1" => 0x3A, "F2" => 0x3B, "F3" => 0x3C, "F4" => 0x3D,
        "F5" => 0x3E, "F6" => 0x3F, "F7" => 0x40, "F8" => 0x41,
        "F9" => 0x42, "F10" => 0x43, "F11" => 0x44, "F12" => 0x45,
        "INSERT" => 0x49, "HOME" => 0x4A, "PAGEUP" => 0x4B,
        "DELETE" => 0x4C, "END" => 0x4D, "PAGEDOWN" => 0x4E,
        "ARROWRIGHT" | "RIGHT" => 0x4F, "ARROWLEFT" | "LEFT" => 0x50,
        "ARROWDOWN" | "DOWN" => 0x51, "ARROWUP" | "UP" => 0x52,
        _ => 0x00,
    }
}

pub fn hid_to_key_name(hid: u8) -> String {
    match hid {
        0x04..=0x1D => format!("{}", (b'A' + (hid - 0x04)) as char),
        0x1E..=0x26 => format!("{}", (b'1' + (hid - 0x1E)) as char),
        0x27 => "0".to_string(),
        0x28 => "Enter".to_string(),
        0x29 => "Esc".to_string(),
        0x2A => "Backspace".to_string(),
        0x2B => "Tab".to_string(),
        0x2C => "Space".to_string(),
        0x2D => "-".to_string(),
        0x2E => "=".to_string(),
        0x3A..=0x45 => format!("F{}", hid - 0x39),
        0x49 => "Insert".to_string(),
        0x4A => "Home".to_string(),
        0x4B => "PageUp".to_string(),
        0x4C => "Delete".to_string(),
        0x4D => "End".to_string(),
        0x4E => "PageDown".to_string(),
        0x4F => "Right".to_string(),
        0x50 => "Left".to_string(),
        0x51 => "Down".to_string(),
        0x52 => "Up".to_string(),
        _ => format!("Key0x{:02X}", hid),
    }
}

// Verified from FUN_0040af50 (dec2.c lines 3466-3521):
// Modifier bits: 0x01 = Ctrl, 0x02 = Shift, 0x04 = Alt, 0x08 = Win
pub fn parse_key_combo(s: &str) -> (u8, u8) {
    let mut mod_mask = 0u8;
    if s.contains("Ctrl") { mod_mask |= 0x01; }
    if s.contains("Shift") { mod_mask |= 0x02; }
    if s.contains("Alt") { mod_mask |= 0x04; }
    if s.contains("Win") { mod_mask |= 0x08; }

    let clean = s
        .trim_start_matches("Combo:")
        .trim_start_matches("Key combination")
        .trim();
    let key_part = clean.split('+').last().unwrap_or("").trim();
    let hid_code = if key_part.eq_ignore_ascii_case("Ctrl")
        || key_part.eq_ignore_ascii_case("Shift")
        || key_part.eq_ignore_ascii_case("Alt")
        || key_part.eq_ignore_ascii_case("Win")
    {
        0x00
    } else {
        key_name_to_hid(key_part)
    };
    (mod_mask, hid_code)
}

// Encodes key combination into EEPROM format at 0x0100 + slot * 0x20 (FUN_0040af50 in dec2.c).
// Emits key-down events (0x80 for modifier, 0x81 for key) followed by key-up events (0x40, 0x41).
pub fn encode_combo_eeprom_payload(s: &str) -> Vec<u8> {
    let (mask, hid) = parse_key_combo(s);
    let mut events: Vec<[u8; 3]> = Vec::new();

    // Key-down sequence
    for &bit in &[0x01u8, 0x02, 0x04, 0x08] {
        if mask & bit != 0 {
            events.push([0x80, bit, 0x00]);
        }
    }
    if hid != 0 {
        events.push([0x81, hid, 0x00]);
    }
    // Key-up sequence
    for &bit in &[0x01u8, 0x02, 0x04, 0x08] {
        if mask & bit != 0 {
            events.push([0x40, bit, 0x00]);
        }
    }
    if hid != 0 {
        events.push([0x41, hid, 0x00]);
    }

    let ev_count = events.len();
    let raw_len = 1 + ev_count * 3 + 1;
    let total_len = ((raw_len + 9) / 10).max(1) * 10;
    let mut buf = vec![0u8; total_len];
    buf[0] = ev_count as u8;
    let mut idx = 1;
    for ev in events {
        buf[idx] = ev[0];
        buf[idx + 1] = ev[1];
        buf[idx + 2] = ev[2];
        idx += 3;
    }
    let sum: u8 = buf[0..idx].iter().fold(0u8, |acc, &x| acc.wrapping_add(x));
    buf[idx] = (0x55u8).wrapping_sub(sum);
    buf
}

pub fn decode_combo_data(data: &[u8]) -> String {
    let mut mods: Vec<String> = Vec::new();
    let mut key_str = String::new();
    let mut i = 1;
    while i + 2 < data.len() {
        match data[i] {
            0x80 => {
                let mask = data[i + 1];
                if mask & 0x01 != 0 && !mods.iter().any(|m| m == "Ctrl") { mods.push("Ctrl".to_string()); }
                if mask & 0x02 != 0 && !mods.iter().any(|m| m == "Shift") { mods.push("Shift".to_string()); }
                if mask & 0x04 != 0 && !mods.iter().any(|m| m == "Alt") { mods.push("Alt".to_string()); }
                if mask & 0x08 != 0 && !mods.iter().any(|m| m == "Win") { mods.push("Win".to_string()); }
                i += 3;
            }
            0x81 => {
                let hid = data[i + 1];
                key_str = hid_to_key_name(hid);
                i += 3;
            }
            _ => break,
        }
    }
    if !key_str.is_empty() {
        mods.push(key_str);
    }
    if mods.is_empty() {
        "Key combination".to_string()
    } else {
        format!("Combo: {}", mods.join("+"))
    }
}

// Encodes a MacroItem into the exact MCU EEPROM format at 0x0300 + slot * 0x0180
// Verified from FUN_0040b450 (dec1.c lines 2047-2175) and Wireshark Frames 15500-15606:
// - Byte 0: UTF-16LE name byte length (max 30)
// - Bytes 1..30: UTF-16LE macro name (30 bytes)
// - Byte 31: Event count N (max 70)
// - Bytes 32..32+N*5: 5-byte events [dir_and_type, code, 0x00, delay_hi, delay_lo]
// - Byte 32+N*5: Checksum = 0x55 - sum(buf[31..32+N*5])
// - Followed by 3 zero pad bytes.
pub fn encode_macro_eeprom_payload(macro_item: &MacroItem) -> Vec<u8> {
    let mut name_utf16: Vec<u16> = macro_item.name.encode_utf16().collect();
    if name_utf16.len() > 15 {
        name_utf16.truncate(15);
    }

    let mut raw_events: Vec<(u8, u8, u16)> = Vec::new();
    for line in &macro_item.key_events {
        let s = line.trim();
        if let Some(rest) = s.strip_prefix("Delay:") .or_else(|| s.strip_prefix("Delay")) {
            let ms: u16 = rest
                .trim()
                .trim_end_matches("ms")
                .trim()
                .parse::<u16>()
                .unwrap_or(20);
            if let Some(last) = raw_events.last_mut() {
                last.2 = last.2.saturating_add(ms);
            }
            continue;
        }

        // Parse optional "(XXms)" suffix
        let (main_part, inline_delay) = if let Some(open_paren) = s.rfind('(') {
            let delay_str = s[open_paren + 1..]
                .trim_end_matches(')')
                .trim_end_matches("ms")
                .trim();
            let d = delay_str.parse::<u16>().unwrap_or(3);
            (s[..open_paren].trim(), d)
        } else {
            (s, 0u16)
        };

        if let Some(btn_str) = main_part.strip_prefix("MouseDown:").or_else(|| main_part.strip_prefix("Mouse Down:")) {
            let code = match btn_str.trim().to_ascii_lowercase().as_str() {
                "left" | "lbutton" | "l-button" => 0x01,
                "right" | "rbutton" | "r-button" => 0x02,
                "middle" | "mbutton" | "m-button" => 0x04,
                "back" | "xbutton1" => 0x08,
                "forward" | "xbutton2" => 0x10,
                _ => 0x01,
            };
            raw_events.push((0x84, code, inline_delay));
        } else if let Some(btn_str) = main_part.strip_prefix("MouseUp:").or_else(|| main_part.strip_prefix("Mouse Up:")) {
            let code = match btn_str.trim().to_ascii_lowercase().as_str() {
                "left" | "lbutton" | "l-button" => 0x01,
                "right" | "rbutton" | "r-button" => 0x02,
                "middle" | "mbutton" | "m-button" => 0x04,
                "back" | "xbutton1" => 0x08,
                "forward" | "xbutton2" => 0x10,
                _ => 0x01,
            };
            raw_events.push((0x44, code, inline_delay));
        } else if let Some(k_str) = main_part.strip_prefix("KeyDown:").or_else(|| main_part.strip_prefix("Key Down:")) {
            let k = k_str.trim();
            let (dir_type, code) = match k.to_ascii_uppercase().as_str() {
                "L-BUTTON" | "LBUTTON" | "LEFT" => (0x84, 0x01),
                "R-BUTTON" | "RBUTTON" | "RIGHT" => (0x84, 0x02),
                "M-BUTTON" | "MBUTTON" | "MIDDLE" => (0x84, 0x04),
                "CTRL" | "CONTROL" | "LCTRL" | "RCTRL" => (0x80, 0x01),
                "SHIFT" | "LSHIFT" | "RSHIFT" => (0x80, 0x02),
                "ALT" | "LALT" | "RALT" => (0x80, 0x04),
                "WIN" | "LWIN" | "RWIN" => (0x80, 0x08),
                _ => (0x81, key_name_to_hid(k).max(0x04)),
            };
            raw_events.push((dir_type, code, inline_delay));
        } else if let Some(k_str) = main_part.strip_prefix("KeyUp:").or_else(|| main_part.strip_prefix("Key Up:")) {
            let k = k_str.trim();
            let (dir_type, code) = match k.to_ascii_uppercase().as_str() {
                "L-BUTTON" | "LBUTTON" | "LEFT" => (0x44, 0x01),
                "R-BUTTON" | "RBUTTON" | "RIGHT" => (0x44, 0x02),
                "M-BUTTON" | "MBUTTON" | "MIDDLE" => (0x44, 0x04),
                "CTRL" | "CONTROL" | "LCTRL" | "RCTRL" => (0x40, 0x01),
                "SHIFT" | "LSHIFT" | "RSHIFT" => (0x40, 0x02),
                "ALT" | "LALT" | "RALT" => (0x40, 0x04),
                "WIN" | "LWIN" | "RWIN" => (0x40, 0x08),
                _ => (0x41, key_name_to_hid(k).max(0x04)),
            };
            raw_events.push((dir_type, code, inline_delay));
        }
    }

    if raw_events.is_empty() {
        raw_events.push((0x84, 0x01, 20));
        raw_events.push((0x44, 0x01, 20));
    }
    if raw_events.len() > 70 {
        raw_events.truncate(70);
    }

    let ev_count = raw_events.len();
    // 32 header bytes + ev_count * 5 + 1 checksum + 3 zero bytes
    let total_len = 32 + ev_count * 5 + 4;
    let mut buf = vec![0u8; total_len];

    buf[0] = (name_utf16.len() * 2) as u8;
    for (i, &ch) in name_utf16.iter().enumerate() {
        let le = ch.to_le_bytes();
        buf[1 + i * 2] = le[0];
        buf[1 + i * 2 + 1] = le[1];
    }

    buf[31] = ev_count as u8;
    for (i, &(dir_type, code, delay)) in raw_events.iter().enumerate() {
        let d = delay.max(3);
        let base = 32 + i * 5;
        buf[base] = dir_type;
        buf[base + 1] = code;
        buf[base + 2] = 0x00;
        buf[base + 3] = (d >> 8) as u8;
        buf[base + 4] = (d & 0xFF) as u8;
    }

    let chk_idx = 32 + ev_count * 5;
    let mut sum = 0u8;
    for &b in &buf[31..chk_idx] {
        sum = sum.wrapping_add(b);
    }
    buf[chk_idx] = (0x55u8).wrapping_sub(sum);
    buf
}

// Loads authentic OEM macro files (*.mck) from %LOCALAPPDATA%\JM01\Mac\
// Each .mck file is 834 bytes obfuscated via right-rotation by 2 bits (decoded via b.rotate_left(2)).
pub fn load_oem_macros() -> Vec<MacroItem> {
    let mut list: Vec<MacroItem> = Vec::new();

    if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
        let mac_dir = Path::new(&local_app_data).join("JM01").join("Mac");
        if let Ok(entries) = fs::read_dir(mac_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()).map(|s| s.eq_ignore_ascii_case("mck")).unwrap_or(false) {
                    if let Ok(raw) = fs::read(&path) {
                        if raw.len() >= 100 {
                            let dec: Vec<u8> = raw.into_iter().map(|b| b.rotate_left(2)).collect();
                            let mut u16s = Vec::new();
                            for i in 0..30 {
                                let ch = u16::from_le_bytes([dec[i * 2], dec[i * 2 + 1]]);
                                if ch == 0 {
                                    break;
                                }
                                u16s.push(ch);
                            }
                            let name = String::from_utf16_lossy(&u16s).trim().to_string();
                            let final_name = if name.is_empty() {
                                path.file_stem().and_then(|s| s.to_str()).unwrap_or("Macro").to_string()
                            } else {
                                name
                            };

                            let mut events = Vec::new();
                            let mut off = 66usize;
                            while off + 6 <= dec.len() {
                                let t = dec[off];
                                let code = dec[off + 1];
                                let delay = u16::from_le_bytes([dec[off + 2], dec[off + 3]]);
                                if t == 0 {
                                    break;
                                }
                                let is_down = (t & 0x80) != 0;
                                let kind = t & 0x0F;
                                let ev_str = match kind {
                                    0x04 => {
                                        let btn_name = match code {
                                            0x01 => "Left",
                                            0x02 => "Right",
                                            0x04 => "Middle",
                                            0x08 => "Back",
                                            0x10 => "Forward",
                                            _ => "Left",
                                        };
                                        if is_down {
                                            format!("MouseDown: {}", btn_name)
                                        } else {
                                            format!("MouseUp: {}", btn_name)
                                        }
                                    }
                                    0x00 => {
                                        let mod_name = match code {
                                            0x01 => "Ctrl",
                                            0x02 => "Shift",
                                            0x04 => "Alt",
                                            0x08 => "Win",
                                            _ => "Ctrl",
                                        };
                                        if is_down {
                                            format!("KeyDown: {}", mod_name)
                                        } else {
                                            format!("KeyUp: {}", mod_name)
                                        }
                                    }
                                    _ => {
                                        let kname = hid_to_key_name(code);
                                        if is_down {
                                            format!("KeyDown: {}", kname)
                                        } else {
                                            format!("KeyUp: {}", kname)
                                        }
                                    }
                                };
                                events.push(ev_str);
                                if delay > 0 {
                                    events.push(format!("Delay {}ms", delay));
                                }
                                off += 6;
                            }

                            if !events.is_empty() {
                                list.push(MacroItem {
                                    name: final_name,
                                    key_events: events,
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    if list.is_empty() {
        list.push(MacroItem {
            name: "fast".to_string(),
            key_events: vec![
                "MouseDown: Left".to_string(),
                "Delay 20ms".to_string(),
                "MouseUp: Left".to_string(),
                "Delay 20ms".to_string(),
            ],
        });
        list.push(MacroItem {
            name: "40ms".to_string(),
            key_events: vec![
                "MouseDown: Left".to_string(),
                "Delay 40ms".to_string(),
                "MouseUp: Left".to_string(),
                "Delay 40ms".to_string(),
            ],
        });
    }

    list
}

// Verified from FUN_0040acc0 (dec1.c lines 1837-1917):
// (0x04, 0x32, 0x03) -> Fire key / Three click
// (0x07, 0x00, 0x00) -> Polling rate switch
// (0x08, 0x00, 0x00) -> RGB ON/OFF
// (0x09, 0x00, 0x00) -> Profile Switch
pub fn decode_button_action(b0: u8, b1: u8, b2: u8) -> String {
    match (b0, b1, b2) {
        (0x01, 0x01, 0x00) => "Left button".to_string(),
        (0x01, 0x02, 0x00) => "Right button".to_string(),
        (0x01, 0x04, 0x00) => "Middle button".to_string(),
        (0x01, 0x10, 0x00) => "Forward".to_string(),
        (0x01, 0x08, 0x00) => "Back".to_string(),
        (0x04, _, _) => "Fire key".to_string(),
        (0x02, 0x02, 0x00) => "DPI +".to_string(),
        (0x02, 0x03, 0x00) => "DPI -".to_string(),
        (0x02, 0x01, 0x00) => "DPI Loop".to_string(),
        (0x08, 0x00, 0x00) => "RGB ON/OFF".to_string(),
        (0x07, 0x00, 0x00) => "Polling rate switch".to_string(),
        (0x09, 0x00, 0x00) => "Profile Switch".to_string(),
        (0x00, 0x00, 0x00) => "Disable".to_string(),
        (0x06, id, _) => format!("Macro - Slot {}", id),
        (0x05, _, _) => "Multimedia / Key combination".to_string(),
        _ => "Left button".to_string(),
    }
}

#[derive(Debug, Clone)]
pub struct MouseHardwareConfig {
    pub polling_rate: PollingRate,
    pub active_stage_count: usize,
    pub active_stage_idx: usize,
    pub dpi_stages: Vec<DpiStage>,
    pub main_led_effect: LedEffect,
    pub main_led_color: [u8; 3],
    pub main_led_speed: u8,
    pub main_led_brightness: u8,
    pub dpi_led_effect: DpiEffect,
    pub button_actions: [String; 8],
    pub battery_percent: u8,
    pub is_charging: bool,
}

pub fn read_device_config(vid: u16) -> Result<MouseHardwareConfig, String> {
    if IS_APPLYING.load(Ordering::SeqCst) {
        return Err("Cannot read device config while applying settings".to_string());
    }

    let api = hidapi::HidApi::new().map_err(|e| e.to_string())?;
    let mut cmd_dev = None;
    let mut is_wired = false;
    for dev_info in api.device_list() {
        if dev_info.vendor_id() == vid && dev_info.product_id() == DEFAULT_PID_WIRED && dev_info.usage_page() == 0xFF02 {
            if let Ok(dev) = dev_info.open_device(&api) {
                cmd_dev = Some(dev);
                is_wired = true;
                ACTIVE_TARGET_PID.store(DEFAULT_PID_WIRED, Ordering::SeqCst);
                break;
            }
        }
    }
    if cmd_dev.is_none() {
        for dev_info in api.device_list() {
            if dev_info.vendor_id() == vid && dev_info.product_id() == DEFAULT_PID_WIRELESS && dev_info.usage_page() == 0xFF02 {
                if let Ok(dev) = dev_info.open_device(&api) {
                    cmd_dev = Some(dev);
                    ACTIVE_TARGET_PID.store(DEFAULT_PID_WIRELESS, Ordering::SeqCst);
                    break;
                }
            }
        }
    }

    let dev = cmd_dev.ok_or_else(|| "No command device (0xFF02) found".to_string())?;

    // 1. Polling rate (0x0000, 2 bytes)
    let rate_data = read_mouse_eeprom(&dev, None, 0x0000, 2)?;
    let polling_rate = match rate_data.get(0).copied().unwrap_or(0x01) {
        0x08 => PollingRate::Hz125,
        0x04 => PollingRate::Hz250,
        0x02 => PollingRate::Hz500,
        _ => PollingRate::Hz1000,
    };

    // 2. Active DPI stage count (0x0002, 2 bytes)
    let count_data = read_mouse_eeprom(&dev, None, 0x0002, 2)?;
    let active_stage_count = count_data.get(0).copied().unwrap_or(5).clamp(1, 5) as usize;

    // 3. Active DPI stage index (0x0004, 2 bytes)
    let idx_data = read_mouse_eeprom(&dev, None, 0x0004, 2)?;
    let active_stage_idx = ((idx_data.get(0).copied().unwrap_or(0) & 0x0F) as usize).clamp(0, 4) + 1;

    // 4. Sensor DPI stages (0x000C, 20 bytes total in chunks: 8, 8, 4)
    let d1 = read_mouse_eeprom(&dev, None, 0x000C, 8)?;
    let d2 = read_mouse_eeprom(&dev, None, 0x0014, 8)?;
    let d3 = read_mouse_eeprom(&dev, None, 0x001C, 4)?;
    let mut stages_raw = Vec::new();
    stages_raw.extend_from_slice(&d1);
    stages_raw.extend_from_slice(&d2);
    stages_raw.extend_from_slice(&d3);

    // 5. DPI Colors (0x002C, 20 bytes total in chunks: 8, 8, 4)
    let c1 = read_mouse_eeprom(&dev, None, 0x002C, 8)?;
    let c2 = read_mouse_eeprom(&dev, None, 0x0034, 8)?;
    let c3 = read_mouse_eeprom(&dev, None, 0x003C, 4)?;
    let mut colors_raw = Vec::new();
    colors_raw.extend_from_slice(&c1);
    colors_raw.extend_from_slice(&c2);
    colors_raw.extend_from_slice(&c3);

    let mut dpi_stages = Vec::new();
    for i in 0..5 {
        let x_code = stages_raw.get(i * 4).copied().unwrap_or(0x1B);
        let hi = stages_raw.get(i * 4 + 2).copied().unwrap_or(0) != 0;
        let dpi_val = paw3325_code_to_dpi(x_code, hi);

        let r = colors_raw.get(i * 4).copied().unwrap_or(255);
        let g = colors_raw.get(i * 4 + 1).copied().unwrap_or(0);
        let b = colors_raw.get(i * 4 + 2).copied().unwrap_or(0);

        dpi_stages.push(DpiStage {
            enabled: i < active_stage_count,
            x: dpi_val,
            y: dpi_val,
            color_rgb: [r, g, b],
        });
    }

    // 6. DPI LED Effect (0x004C, 6 bytes)
    let d_eff = read_mouse_eeprom(&dev, None, 0x004C, 6).unwrap_or_else(|_| vec![0x00, 0x55]);
    let dpi_led_effect = match d_eff.get(0).copied().unwrap_or(0) {
        0x01 => DpiEffect::Steady,
        0x02 => DpiEffect::Respiration,
        _ => DpiEffect::Off,
    };

    // 7. Button matrix (0x0060..0x008F)
    const SPEED_LUT: [u8; 10] = [0xFF, 0xE6, 0xD2, 0xBE, 0xAA, 0x96, 0x82, 0x6E, 0x46, 0x28];
    const NEON_SPEED_LUT: [u8; 10] = [0x2D, 0x28, 0x23, 0x1E, 0x19, 0x13, 0x0F, 0x0A, 0x05, 0x03];
    const BRIGHTNESS_LUT: [u8; 10] = [0x19, 0x32, 0x4B, 0x64, 0x7D, 0x96, 0xAF, 0xC8, 0xE1, 0xFF];

    let b_chunk0 = read_mouse_eeprom(&dev, None, 0x0060, 8).unwrap_or_else(|_| vec![1, 1, 0, 0x53, 1, 2, 0, 0x52]);
    let b_chunk1 = read_mouse_eeprom(&dev, None, 0x0068, 8).unwrap_or_else(|_| vec![1, 4, 0, 0x50, 1, 8, 0, 0x4C]);
    let b_chunk2 = read_mouse_eeprom(&dev, None, 0x0070, 8).unwrap_or_else(|_| vec![1, 0x10, 0, 0x44, 0, 0, 0, 0x55]);
    // Button 6 (Rapid Fire / Fire key) is at Slot 8 (0x0080), NOT 0x0078!
    let b_chunk3 = read_mouse_eeprom(&dev, None, 0x0080, 8).unwrap_or_else(|_| vec![4, 0x32, 3, 0x1C, 0, 0, 0, 0x55]);
    let b_chunk4 = read_mouse_eeprom(&dev, None, 0x0088, 8).unwrap_or_else(|_| vec![2, 2, 0, 0x51, 2, 3, 0, 0x50]);

    let decode_slot = |b0: u8, b1: u8, b2: u8, slot: usize| -> String {
        if b0 == 0x05 {
            let combo_addr = 0x0100 + (slot as u16) * 0x20;
            if let Ok(combo) = read_mouse_eeprom(&dev, None, combo_addr, 10) {
                if combo.get(1) == Some(&0x82) && combo.len() >= 4 {
                    let code = (combo[2] as u16) | ((combo[3] as u16) << 8);
                    return decode_consumer_code(code);
                } else if combo.get(1) == Some(&0x80) || combo.get(1) == Some(&0x81) {
                    return decode_combo_data(&combo);
                }
            }
        } else if b0 == 0x06 {
            let macro_slot = if b1 <= 15 { b1 as u16 } else { slot as u16 };
            let macro_addr = 0x0300 + macro_slot * 0x0180;
            if let Ok(m_hdr) = read_mouse_eeprom(&dev, None, macro_addr, 10) {
                let byte_len = m_hdr.get(0).copied().unwrap_or(0).min(30) as usize;
                if byte_len >= 2 {
                    let mut full_hdr = m_hdr;
                    if byte_len + 1 > 10 {
                        if let Ok(m_hdr2) = read_mouse_eeprom(&dev, None, macro_addr + 10, 10) {
                            full_hdr.extend_from_slice(&m_hdr2);
                        }
                    }
                    if byte_len + 1 > 20 {
                        if let Ok(m_hdr3) = read_mouse_eeprom(&dev, None, macro_addr + 20, 10) {
                            full_hdr.extend_from_slice(&m_hdr3);
                        }
                    }
                    let mut u16s = Vec::new();
                    let mut idx = 1;
                    while idx + 1 < (1 + byte_len).min(full_hdr.len()) {
                        let ch = u16::from_le_bytes([full_hdr[idx], full_hdr[idx + 1]]);
                        if ch == 0 {
                            break;
                        }
                        u16s.push(ch);
                        idx += 2;
                    }
                    let name = String::from_utf16_lossy(&u16s).trim().to_string();
                    if !name.is_empty() {
                        return format!("Macro - {}", name);
                    }
                }
            }
        }
        decode_button_action(b0, b1, b2)
    };

    let btn1 = decode_slot(b_chunk0[0], b_chunk0[1], b_chunk0[2], BUTTON_SLOT_MAP[0]);
    let btn2 = decode_slot(b_chunk0[4], b_chunk0[5], b_chunk0[6], BUTTON_SLOT_MAP[1]);
    let btn3 = decode_slot(b_chunk1[0], b_chunk1[1], b_chunk1[2], BUTTON_SLOT_MAP[2]);
    let btn5 = decode_slot(b_chunk1[4], b_chunk1[5], b_chunk1[6], BUTTON_SLOT_MAP[4]);
    let btn4 = decode_slot(b_chunk2[0], b_chunk2[1], b_chunk2[2], BUTTON_SLOT_MAP[3]);
    let btn6 = decode_slot(b_chunk3[0], b_chunk3[1], b_chunk3[2], BUTTON_SLOT_MAP[5]);
    let btn7 = decode_slot(b_chunk4[0], b_chunk4[1], b_chunk4[2], BUTTON_SLOT_MAP[6]);
    let btn8 = decode_slot(b_chunk4[4], b_chunk4[5], b_chunk4[6], BUTTON_SLOT_MAP[7]);

    let button_actions = [btn1, btn2, btn3, btn4, btn5, btn6, btn7, btn8];

    // 8. Main RGB LED (0x00A0, 7 bytes)
    let rgb_data = read_mouse_eeprom(&dev, None, 0x00A0, 7).unwrap_or_else(|_| vec![0x02, 255, 0, 0, 0x96, 0x7D, 0x00]);
    let main_led_effect = match rgb_data.get(0).copied().unwrap_or(0x02) {
        0x00 => LedEffect::Neon,
        0x01 => LedEffect::Respiration,
        0x02 => LedEffect::Steady,
        _ => LedEffect::Off,
    };
    let main_led_color = [
        rgb_data.get(1).copied().unwrap_or(255),
        rgb_data.get(2).copied().unwrap_or(0),
        rgb_data.get(3).copied().unwrap_or(0),
    ];
    let spd_byte = rgb_data.get(4).copied().unwrap_or(0x96);
    let bri_byte = rgb_data.get(5).copied().unwrap_or(0x7D);
    let main_led_speed = if main_led_effect == LedEffect::Neon {
        reverse_lut(&NEON_SPEED_LUT, spd_byte)
    } else {
        reverse_lut(&SPEED_LUT, spd_byte)
    };
    let main_led_brightness = reverse_lut(&BRIGHTNESS_LUT, bri_byte);

    // 9. Battery
    let (battery_percent, chg) = query_mouse_battery(&dev, None).unwrap_or((100, false));
    let is_charging = if is_wired { true } else { chg };

    Ok(MouseHardwareConfig {
        polling_rate,
        active_stage_count,
        active_stage_idx,
        dpi_stages,
        main_led_effect,
        main_led_color,
        main_led_speed,
        main_led_brightness,
        dpi_led_effect,
        button_actions,
        battery_percent,
        is_charging,
    })
}
