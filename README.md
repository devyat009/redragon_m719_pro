# Redragon INVADER PRO Suite (M719RGB-PRO)

[![Rust](https://img.shields.io/badge/Language-Rust%202021-orange.svg?style=flat-square&logo=rust)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/Platform-Windows%2010%20%2F%2011-blue.svg?style=flat-square&logo=windows)](https://microsoft.com/windows)
[![Sensor](https://img.shields.io/badge/Sensor-PixArt%20PAW3325-red.svg?style=flat-square)](#hardware-specifications)
[![Framework](https://img.shields.io/badge/GUI-egui%20%2F%20eframe-darkgreen.svg?style=flat-square)](https://github.com/emilk/egui)
[![License](https://img.shields.io/badge/License-MIT-lightgrey.svg?style=flat-square)](LICENSE)

A high-performance, lightweight, and bloat-free open-source control suite for the **Redragon INVADER PRO (M719RGB-PRO)** gaming mouse, written from scratch in Rust.

This software completely replaces the proprietary OEM drivers (`OemDrv.exe` / `Lowerdev.dll`), communicating directly with the mouse MCU over raw USB Human Interface Device (HID) feature reports without requiring third-party background services or Electron dependencies.

---

## Hardware Specifications & Compatibility

| Parameter | Value |
|---|---|
| **Product Model** | Redragon M719RGB-PRO (INVADER PRO) |
| **Optical Sensor** | PixArt PAW3325 (Up to 10,000 DPI via software interpolation) |
| **Vendor ID (VID)** | `0x25A7` |
| **Product ID (Wireless 2.4G Dongle)** | `0xFA7E` |
| **Product ID (Wired USB Mode)** | `0xFAB0` |
| **Wireless Architecture** | 2.4GHz RF with low-latency nano USB receiver |
| **MCU Protocol Checksum** | Custom 4-byte checksum constraint: `(B0 + B1 + B2 + Checksum) & 0xFF == 0x55` |
| **Configuration Storage** | Non-volatile onboard EEPROM (5 hardware profiles) |

---

## Key Features

- **Direct Hardware Communication:**
  Reads and writes settings directly to the mouse onboard EEPROM across USB and 2.4G wireless channels using `hidapi`.

- **Live Power & Link Status Monitoring:**
  Actively probes the 2.4GHz RF connection status. Accurately distinguishes between receiver-only connection and active mouse power state (detecting when the mouse switch is turned off or when entering deep sleep).

- **Dynamic Connection Detection:**
  Auto-detects USB cable connection, displaying wired mode and real-time battery charging animations.

- **Full Profile Management:**
  Configure up to 5 hardware profiles stored in onboard EEPROM. Supports importing and exporting profiles in both modern JSON format and legacy binary `.jmk` OEM backup formats.

- **DPI Stage Configuration:**
  Full 5-stage DPI management (100 to 10,000 DPI in 100 DPI steps) with independent X/Y sensitivity, custom stage colors, and DPI lighting indicators.

- **RGB Lighting Engine:**
  Full control over lighting modes (Steady, Respiration, Neon, Streaming, Off), brightness (1 to 10), animation speed (1 to 10), and custom RGB presets with a 10-slot palette manager.

- **Key Mapping & Coordinate Visualizer:**
  Interactive mouse chassis overlay with coordinate-mapped buttons (1 through 8). Remap standard buttons, rapid fire, multimedia keys, and custom DPI switches.

- **Macro Studio:**
  Built-in macro recorder and editor with cycle loop controls, millisecond delay injection, and file-based profile persistence.

- **Modern Native UI:**
  OLED Dark Mode (`#101010`) and Light Mode support, custom accent color selection (including Redragon Red, Dark Forest Green, Dark Royal Blue, Cyber Cyan, and Solar Amber presets), clean Segoe UI icons, and zero font box artifacts.

- **Background Daemon & System Tray:**
  System tray integration (`tray-icon`), minimize-to-tray on close, and optional Windows startup configuration (`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`).

- **Diagnostics & Event Console:**
  Integrated live terminal console in the Settings view logging all USB bus probes, EEPROM writes, profile operations, and hardware errors with copy and export capabilities.

---

## Reverse-Engineered Hardware Protocol

Communication with the onboard controller is performed over USB HID Feature Reports:

### Communication Endpoints
- **Command Dispatch Endpoint:** Report ID `0x08`, Usage Page `0xFF02` (Usage `0x02`)
- **Notification / Response Endpoint:** Report ID `0x09`, Usage Page `0xFF01` (Usage `0x01`)
- **Battery Feature Report:** Report ID `0x06`, Usage Page `0xFF04` (Usage `0x04`)

### Packet Checksum Algorithm
Every EEPROM write payload is validated by the mouse MCU using a 4-byte checksum constraint where the sum of the 3 payload bytes plus the checksum byte equals `0x55` modulo 256:

```rust
fn make_checksum_block(b0: u8, b1: u8, b2: u8) -> [u8; 4] {
    let chk = (0x55u8).wrapping_sub(b0).wrapping_sub(b1).wrapping_sub(b2);
    [b0, b1, b2, chk]
}
```

### Memory Map Layout
- **Offset `0x00` - `0x07`:** Profile metadata and active profile index.
- **Offset `0x08` - `0x1F`:** Button remapping table (8 buttons, 3 bytes each).
- **Offset `0x20` - `0x47`:** DPI stages, X/Y axes configuration, and stage color descriptors.
- **Offset `0x48` - `0x5F`:** Lighting parameters (effect ID, brightness, frequency speed, RGB color).
- **Offset `0x60` - `0x6F`:** USB Polling Rate (`125Hz`, `250Hz`, `500Hz`, `1000Hz`) and sensor performance flags.

---

## Prerequisites

Before building the application on Windows, verify that the following tools are installed:

1. **Rust Toolchain (2021 edition or newer):**
   Install via [rustup.rs](https://rustup.rs/):
   ```powershell
   winget install Rustlang.Rustup
   ```

2. **Visual Studio C++ Build Tools:**
   Ensure the MSVC C++ toolset is available (Visual Studio 2019/2022/2026 Build Tools with Windows 10/11 SDK).

---

## Build Instructions

Clone the repository and compile the release binary:

```powershell
git clone https://github.com/username/redragon-m719-pro.git
cd redragon-m719-pro
cargo build --release
```

The optimized binary with Windows subsystem enabled (`windows_subsystem = "windows"`, running without a console window) will be located at:

```
target/release/redragon_m719_pro.exe
```

---

## Running the Application

To launch the compiled release executable:

```powershell
.\target\release\redragon_m719_pro.exe
```

Alternatively, to run directly during development:

```powershell
cargo run --release
```

---

## Project Structure

```
redragon_m719_pro/
├── Cargo.toml               # Project dependencies and release configuration
├── assets/
│   ├── icon.ico             # Application window and system tray icon
│   ├── logo.png             # Redragon branding mark
│   └── mouse.png            # High-resolution mouse chassis asset for coordinate mapping
├── src/
│   ├── main.rs              # egui application lifecycle, layout panels, tabs, and event handling
│   └── hardware.rs          # HID bridge, packet serialization, EEPROM writer, and JMK parser
└── README.md                # Documentation and protocol specification
```

---

## License

This project is licensed under the MIT License. See the [LICENSE](LICENSE) file for full details.
