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

### Packet Framing & Checksum Algorithms
1. **HID Feature Report Framing (17 Bytes):**
   - Transmitted on Usage Page `0xFF02` (Command endpoint).
   - Byte `0`: Report ID `0x08`.
   - Byte `1`: Command ID (`0x07` Write EEPROM, `0x08` Read EEPROM, `0x09` Commit, `0x02` TellDrvStatus, `0x03` GetConnectStatus).
   - Bytes `2..15`: Command payload parameters.
   - Byte `16`: Frame checksum constraint: `(sum(pkt[0..15]) + pkt[16]) & 0xFF == 0x55`.

2. **EEPROM Block Data Checksums:**
   - Every block written to the onboard memory carries its own checksum trailer:
   - For 4-byte key/stage blocks: `chk = 0x55 - (b0 + b1 + b2)`.
   - For 7-byte Main RGB block: `chk = 0x55 - sum(bytes 0..5)`.
   - For 2-byte register blocks: `chk = 0x55 - byte0`.

### Real EEPROM Hardware Memory Map Layout
- **Address `0x0000` (2 Bytes):** USB Polling Rate (`0x01` = 1000Hz, `0x02` = 500Hz, `0x04` = 250Hz, `0x08` = 125Hz).
- **Address `0x0002` (2 Bytes):** Count of disabled DPI stages (`[count, 0x55 - count]`).
- **Address `0x000C + 4*i` (4 Bytes/stage):** Sensor DPI stage calibrations (`[Xcode, Ycode, hiRange, checksum]`). Codes mapped to PixArt PAW3325 LUT.
- **Address `0x002C + 4*i` (4 Bytes/stage):** Sensor DPI stage LED indicator colors (`[Red, Green, Blue, checksum]`).
- **Address `0x004C` (6 Bytes):** DPI Indicator LED effect mode (`[effect, chk, color_idx, chk, level, chk]`).
- **Address `0x0054` - `0x005C`:** Logo LED effect parameters and mode registers.
- **Address `0x0060 + 4*i` (4 Bytes/slot):** Hardware Key Matrix (8 slots, `[b0, b1, b2, checksum]`).
- **Address `0x00A0` (7 Bytes):** Main RGB Lighting (`[effect, Red, Green, Blue, speed, brightness, checksum]`).
- **Address `0x0100 + 0x20*i`:** Combo-key sequence memory.
- **Address `0x0300 + 0x180*i`:** Onboard macro event storage (16 hardware slots).

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
git clone https://github.com/devyat009/redragon-m719-pro.git
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
