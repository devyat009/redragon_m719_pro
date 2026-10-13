#![windows_subsystem = "windows"]

mod hardware;

use std::fs;
use std::time::Instant;
use eframe::egui;
use hardware::{parse_jmk_bytes, DpiEffect, HardwareBridge, LedEffect, MacroItem, MouseProfile, PollingRate, DEFAULT_PID_WIRED, DEFAULT_PID_WIRELESS};
use tray_icon::{
    menu::{Menu, MenuEvent, MenuItem},
    Icon, TrayIcon, TrayIconBuilder, TrayIconEvent,
};

const MOUSE_PNG_BYTES: &[u8] = include_bytes!("../assets/mouse.png");
const LOGO_PNG_BYTES: &[u8] = include_bytes!("../assets/logo.png");

// Exact centers of the button circle marks on mouse.png (382 x 263)
const BUTTON_COORDS: [(f32, f32); 8] = [
    (0.3610, 0.6726), // 1. Left Click
    (0.1031, 0.5379), // 2. Right Click
    (0.2492, 0.4455), // 3. Middle Wheel
    (0.7206, 0.5927), // 4. Forward
    (0.8313, 0.5077), // 5. Back
    (0.5426, 0.7344), // 6. Rapid Fire
    (0.4005, 0.2340), // 7. DPI +
    (0.4961, 0.1242), // 8. DPI -
];

#[derive(PartialEq, Clone, Copy)]
enum AppTab {
    Main,
    Advanced,
    Macro,
    Lighting,
    Settings,
}

#[derive(PartialEq, Clone, Copy)]
enum ThemeMode {
    Dark,
    Light,
}

#[derive(Clone, Copy, PartialEq)]
enum LogLevel {
    Info,
    Success,
    Warn,
    Error,
    Status,
}

#[derive(Clone)]
struct LogEntry {
    time: String,
    level: LogLevel,
    message: String,
}

#[cfg(windows)]
fn trim_memory() {
    extern "system" {
        fn SetProcessWorkingSetSize(
            h_process: *mut std::ffi::c_void,
            min_size: usize,
            max_size: usize,
        ) -> i32;
        fn GetCurrentProcess() -> *mut std::ffi::c_void;
    }
    unsafe {
        SetProcessWorkingSetSize(GetCurrentProcess(), usize::MAX, usize::MAX);
    }
}

#[cfg(not(windows))]
fn trim_memory() {}

fn is_autostart_enabled() -> bool {
    let output = std::process::Command::new("reg")
        .args(["query", r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run", "/v", "RedragonInvaderPro"])
        .output();
    matches!(output, Ok(o) if o.status.success())
}

fn set_autostart(enable: bool) {
    if enable {
        if let Ok(exe) = std::env::current_exe() {
            let path_str = exe.to_string_lossy().to_string();
            let _ = std::process::Command::new("reg")
                .args([
                    "add",
                    r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                    "/v",
                    "RedragonInvaderPro",
                    "/t",
                    "REG_SZ",
                    "/d",
                    &format!("\"{}\"", path_str),
                    "/f",
                ])
                .output();
        }
    } else {
        let _ = std::process::Command::new("reg")
            .args([
                "delete",
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                "/v",
                "RedragonInvaderPro",
                "/f",
            ])
            .output();
    }
}

#[derive(Clone, Copy, PartialEq)]
enum TrayAction {
    Show,
    Hide,
}

fn build_tray() -> (Option<TrayIcon>, std::sync::mpsc::Receiver<TrayAction>) {
    let (tx, rx) = std::sync::mpsc::channel();
    let menu = Menu::new();
    let item_show = MenuItem::new("Show Redragon Suite", true, None);
    let item_hide = MenuItem::new("Hide to Tray", true, None);
    let item_quit = MenuItem::new("Quit", true, None);

    let id_show = item_show.id().clone();
    let id_hide = item_hide.id().clone();
    let id_quit = item_quit.id().clone();

    let _ = menu.append(&item_show);
    let _ = menu.append(&item_hide);
    let _ = menu.append(&item_quit);

    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        if event.id == id_quit {
            std::process::exit(0);
        } else if event.id == id_show {
            let _ = tx.send(TrayAction::Show);
        } else if event.id == id_hide {
            let _ = tx.send(TrayAction::Hide);
        }
    }));

    let icon = if let Ok(img) = image::load_from_memory(LOGO_PNG_BYTES) {
        let rgba = img.resize_exact(32, 32, image::imageops::FilterType::Lanczos3).to_rgba8();
        let (w, h) = (rgba.width(), rgba.height());
        Icon::from_rgba(rgba.into_raw(), w, h).ok()
    } else {
        None
    };

    if let Some(ic) = icon {
        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("Redragon INVADER PRO Suite")
            .with_icon(ic)
            .build()
            .ok();

        (tray, rx)
    } else {
        (None, rx)
    }
}

struct RedragonApp {
    bridge: HardwareBridge,
    profiles: Vec<MouseProfile>,
    active_profile_idx: usize,
    current_tab: AppTab,
    theme: ThemeMode,
    accent_color: egui::Color32,
    status_msg: String,
    mouse_texture: Option<egui::TextureHandle>,
    logo_texture: Option<egui::TextureHandle>,
    last_poll_time: Instant,
    // Macro state
    macros: Vec<MacroItem>,
    selected_macro_idx: usize,
    selected_key_event_idx: Option<usize>,
    macro_recording: bool,
    auto_record_delay: bool,
    macro_loop_mode: usize,
    macro_cycles: u32,
    new_macro_name: String,
    last_record_event_time: Instant,
    insert_event_type: String,
    insert_event_param: String,
    // Custom color picker state
    temp_custom_color: [u8; 3],
    // Logs state
    logs: Vec<LogEntry>,
    auto_scroll_logs: bool,
    // Previous hardware state for change detection logging
    prev_connected: bool,
    prev_charging: bool,
    prev_wireless: bool,
    // Background and Autostart
    minimize_to_tray: bool,
    autostart_with_windows: bool,
    _tray: Option<TrayIcon>,
    tray_rx: std::sync::mpsc::Receiver<TrayAction>,
    // Apply progress worker state
    apply_rx: Option<std::sync::mpsc::Receiver<(f32, String)>>,
    apply_progress: Option<(f32, String)>,
    // DPI switch floating overlay (toast)
    dpi_toast_time: Option<Instant>,
    dpi_toast_stage: usize,
    dpi_toast_val: u32,
    dpi_toast_color: egui::Color32,
}

impl RedragonApp {
    fn add_log(&mut self, level: LogLevel, msg: impl Into<String>) {
        let message = msg.into();
        let time = chrono::Local::now().format("%H:%M:%S").to_string();
        self.logs.push(LogEntry {
            time,
            level,
            message,
        });
        if self.logs.len() > 600 {
            self.logs.remove(0);
        }
    }

    fn show_dpi_toast(&mut self, stage: usize, dpi: u32, color_rgb: [u8; 3]) {
        self.dpi_toast_time = Some(Instant::now());
        self.dpi_toast_stage = stage;
        self.dpi_toast_val = dpi;
        self.dpi_toast_color = egui::Color32::from_rgb(color_rgb[0], color_rgb[1], color_rgb[2]);
    }
}

impl Default for RedragonApp {
    fn default() -> Self {
        let mut profiles = Vec::new();
        for i in 1..=5 {
            let mut p = MouseProfile::default();
            p.name = format!("Profile {}", i);
            profiles.push(p);
        }

        let macros = vec![
            MacroItem {
                name: "40ms Rapid".to_string(),
                key_events: vec![
                    "Key Down: L-Button".to_string(),
                    "Delay: 40ms".to_string(),
                    "Key Up: L-Button".to_string(),
                ],
            },
            MacroItem {
                name: "Fast Double".to_string(),
                key_events: vec![
                    "Key Down: R-Button".to_string(),
                    "Delay: 20ms".to_string(),
                    "Key Up: R-Button".to_string(),
                ],
            },
            MacroItem {
                name: "Jumpshot".to_string(),
                key_events: vec![
                    "Key Down: Space".to_string(),
                    "Delay: 35ms".to_string(),
                    "Key Down: L-Button".to_string(),
                    "Delay: 50ms".to_string(),
                    "Key Up: L-Button".to_string(),
                    "Key Up: Space".to_string(),
                ],
            },
        ];

        let autostart = is_autostart_enabled();
        let (tray, tray_rx) = build_tray();
        let bridge = HardwareBridge::new();
        let conn = bridge.connected;
        let charg = bridge.is_charging;
        let wire = bridge.is_wireless;

        let mut app = Self {
            bridge,
            profiles,
            active_profile_idx: 0,
            current_tab: AppTab::Main,
            theme: ThemeMode::Dark,
            accent_color: egui::Color32::from_rgb(215, 38, 56), // Redragon Red default
            status_msg: "Ready".to_string(),
            mouse_texture: None,
            logo_texture: None,
            last_poll_time: Instant::now(),
            macros,
            selected_macro_idx: 0,
            selected_key_event_idx: None,
            macro_recording: false,
            auto_record_delay: true,
            macro_loop_mode: 0,
            macro_cycles: 1,
            new_macro_name: String::new(),
            last_record_event_time: Instant::now(),
            insert_event_type: "Delay (ms)".to_string(),
            insert_event_param: "50".to_string(),
            temp_custom_color: [27, 94, 32],
            logs: Vec::new(),
            auto_scroll_logs: true,
            prev_connected: conn,
            prev_charging: charg,
            prev_wireless: wire,
            minimize_to_tray: true,
            autostart_with_windows: autostart,
            _tray: tray,
            tray_rx,
            apply_rx: None,
            apply_progress: None,
            dpi_toast_time: None,
            dpi_toast_stage: 1,
            dpi_toast_val: 1000,
            dpi_toast_color: egui::Color32::from_rgb(0, 255, 255),
        };

        trim_memory();

        app.add_log(LogLevel::Info, "Redragon Invader Pro Suite initialized.");
        if app.bridge.connected {
            let mode = if app.bridge.is_charging {
                "USB Charging"
            } else if app.bridge.is_wireless {
                "Wireless 2.4G"
            } else {
                "Wired USB"
            };
            app.add_log(LogLevel::Status, format!("Device detected: {} [{}] (Battery: {}%)", app.bridge.device_name, mode, app.bridge.battery_percent));
        } else if app.bridge.found_dongle {
            app.add_log(LogLevel::Warn, format!("2.4G USB Receiver connected (PID 0x{:04X}), but mouse is not responding: verify mouse switch is ON underneath or wake up mouse from sleep.", app.bridge.pid));
        } else {
            app.add_log(LogLevel::Warn, format!("No Redragon USB device found: neither 2.4G dongle (PID 0x{:04X}) nor wired USB cable (PID 0x{:04X}) detected on system USB bus.", DEFAULT_PID_WIRELESS, DEFAULT_PID_WIRED));
        }

        if autostart {
            app.add_log(LogLevel::Info, "System auto-start on boot is active.");
        }

        app
    }
}

impl eframe::App for RedragonApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Handle Tray Icon Events
        if let Ok(event) = TrayIconEvent::receiver().try_recv() {
            match event {
                TrayIconEvent::Click { button: tray_icon::MouseButton::Left, .. } | TrayIconEvent::DoubleClick { .. } => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                _ => {}
            }
        }

        // Handle Tray Menu Events
        while let Ok(action) = self.tray_rx.try_recv() {
            match action {
                TrayAction::Show => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                TrayAction::Hide => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                    trim_memory();
                }
            }
        }

        // Process Apply Background Worker Channel
        if let Some(rx) = &self.apply_rx {
            while let Ok((pct, msg)) = rx.try_recv() {
                if pct < 0.0 {
                    // Error encountered
                    self.status_msg = msg.clone();
                    self.add_log(LogLevel::Error, format!("Hardware sync failure: {}", msg));
                    self.apply_progress = None;
                    self.apply_rx = None;
                    break;
                } else if pct >= 1.0 {
                    // Complete
                    self.status_msg = msg.clone();
                    let active = &self.profiles[self.active_profile_idx];
                    let current_dpi = active.dpi_stages.get(active.current_stage.saturating_sub(1)).map(|s| s.x).unwrap_or(1000);
                    self.add_log(LogLevel::Success, format!(
                        "Profile {} written to mouse EEPROM (LED: {:?}, Polling: {:?}, DPI: {}).",
                        self.active_profile_idx + 1,
                        active.led_effect,
                        active.polling_rate,
                        current_dpi
                    ));
                    self.apply_progress = None;
                    self.apply_rx = None;
                    break;
                } else {
                    // Progress update
                    self.apply_progress = Some((pct, msg));
                }
            }
        }
        if self.apply_progress.is_some() {
            ctx.request_repaint();
        }

        // Intercept window close button if minimize to tray is enabled
        if self.minimize_to_tray && ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            trim_memory();
            self.add_log(LogLevel::Info, "Application minimized to system tray notification area.");
        }

        // Automatic background polling of USB bus every 800ms
        if self.last_poll_time.elapsed().as_millis() > 800 {
            self.last_poll_time = std::time::Instant::now();
            self.bridge.detect();
            self.profiles[self.active_profile_idx].battery_percent = self.bridge.battery_percent;
            ctx.request_repaint();

            // Log status transitions
            if self.bridge.connected != self.prev_connected
                || self.bridge.is_charging != self.prev_charging
                || self.bridge.is_wireless != self.prev_wireless
            {
                if self.bridge.connected {
                    if self.bridge.is_charging {
                        self.add_log(LogLevel::Status, format!("Mouse plugged in: Charging via USB (Battery: {}%)", self.bridge.battery_percent));
                    } else if self.bridge.is_wireless {
                        self.add_log(LogLevel::Status, format!("Mouse connected via Wireless 2.4G receiver (Battery: {}%)", self.bridge.battery_percent));
                    } else {
                        self.add_log(LogLevel::Status, format!("Mouse connected via Wired USB (Battery: {}%)", self.bridge.battery_percent));
                    }
                } else if self.bridge.found_dongle {
                    self.add_log(LogLevel::Warn, "Mouse link lost: 2.4G receiver is plugged in, but mouse switch was turned OFF or entered standby sleep.");
                } else {
                    self.add_log(LogLevel::Warn, "Mouse disconnected: USB device unplugged from PC.");
                }
                self.prev_connected = self.bridge.connected;
                self.prev_charging = self.bridge.is_charging;
                self.prev_wireless = self.bridge.is_wireless;
            }

            self.last_poll_time = Instant::now();
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(800));

        // Embed & Load genuine mouse image texture
        if self.mouse_texture.is_none() {
            if let Ok(image) = image::load_from_memory(MOUSE_PNG_BYTES) {
                let image = image.to_rgba8();
                let (w, h) = (image.width() as usize, image.height() as usize);
                let color_image = egui::ColorImage::from_rgba_unmultiplied([w, h], &image.into_raw());
                self.mouse_texture = Some(ctx.load_texture("mouse_view", color_image, egui::TextureOptions::LINEAR));
            }
        }

        // Embed & Load logo texture
        if self.logo_texture.is_none() {
            if let Ok(image) = image::load_from_memory(LOGO_PNG_BYTES) {
                let image = image.to_rgba8();
                let (w, h) = (image.width() as usize, image.height() as usize);
                let color_image = egui::ColorImage::from_rgba_unmultiplied([w, h], &image.into_raw());
                self.logo_texture = Some(ctx.load_texture("logo_view", color_image, egui::TextureOptions::LINEAR));
                trim_memory();
            }
        }

        // Live Macro Keystroke Recorder listener
        if self.macro_recording {
            let events = ctx.input(|i| i.raw.events.clone());
            for event in events {
                match event {
                    egui::Event::Key { key, pressed, repeat, .. } => {
                        if !repeat {
                            let now = Instant::now();
                            let elapsed = now.duration_since(self.last_record_event_time).as_millis();
                            self.last_record_event_time = now;

                            let mut recorded_evt = None;
                            if let Some(m) = self.macros.get_mut(self.selected_macro_idx) {
                                if self.auto_record_delay && elapsed > 5 && elapsed < 10000 {
                                    m.key_events.push(format!("Delay: {}ms", elapsed));
                                }
                                if pressed {
                                    m.key_events.push(format!("Key Down: {:?}", key));
                                    recorded_evt = Some((m.name.clone(), format!("Key Down: {:?}", key)));
                                } else {
                                    m.key_events.push(format!("Key Up: {:?}", key));
                                    recorded_evt = Some((m.name.clone(), format!("Key Up: {:?}", key)));
                                }
                            }
                            if let Some((name, evt)) = recorded_evt {
                                self.add_log(LogLevel::Info, format!("Macro '{}' recorded: {}", name, evt));
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        // Apply Theme Visuals - Dark theme #101010 OLED
        match self.theme {
            ThemeMode::Dark => {
                let mut visuals = egui::Visuals::dark();
                let c_10 = egui::Color32::from_rgb(16, 16, 16);
                visuals.panel_fill = c_10;
                visuals.window_fill = c_10;
                visuals.extreme_bg_color = egui::Color32::from_rgb(10, 10, 10);
                visuals.faint_bg_color = egui::Color32::from_rgb(22, 22, 22);
                visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(20, 20, 20);
                visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(28, 28, 28);
                visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(38, 38, 38);
                visuals.widgets.active.bg_fill = self.accent_color;
                visuals.selection.bg_fill = self.accent_color;
                ctx.set_visuals(visuals);
            }
            ThemeMode::Light => {
                let mut visuals = egui::Visuals::light();
                visuals.selection.bg_fill = self.accent_color;
                ctx.set_visuals(visuals);
            }
        }

        let is_dark = self.theme == ThemeMode::Dark;
        let accent = self.accent_color;
        let text_color = if is_dark { egui::Color32::from_rgb(225, 225, 225) } else { egui::Color32::from_rgb(25, 25, 25) };

        // 1. LEFT SIDEBAR PANEL (Redragon Logo + Tabs with Icons)
        egui::SidePanel::left("left_panel")
            .resizable(false)
            .exact_width(215.0)
            .show(ctx, |ui| {
                ui.add_space(14.0);
                ui.vertical_centered(|ui| {
                    if let Some(logo) = &self.logo_texture {
                        ui.image(egui::load::SizedTexture::new(logo.id(), egui::vec2(54.0, 54.0)));
                    }
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new("REDRAGON").color(accent).size(19.0).strong());
                    ui.label(egui::RichText::new("INVADER PRO").color(text_color).size(12.0).strong());
                    ui.add_space(8.0);

                    // Connection Badge with Segoe Emoji + Pulsing Dot
                    ui.horizontal(|ui| {
                        let (dot_color, mode_text) = if self.bridge.connected {
                            let t = ctx.input(|i| i.time);
                            let pulse = ((t * 3.5).sin() * 0.25 + 0.75) as f32;
                            if self.bridge.is_charging {
                                let c = (220.0 * pulse) as u8;
                                (egui::Color32::from_rgb(255, c.max(160), 0), "⚡ USB Charging")
                            } else if self.bridge.is_wireless {
                                let g = (210.0 * pulse) as u8;
                                (egui::Color32::from_rgb(20, g.max(140), 60), "📶 Wireless 2.4G")
                            } else {
                                (egui::Color32::from_rgb(0, 200, 255), "🔌 Wired USB")
                            }
                        } else if self.bridge.is_wireless {
                            (egui::Color32::from_rgb(220, 50, 50), "💤 Off / Standby")
                        } else {
                            (egui::Color32::from_rgb(220, 50, 50), "Disconnected")
                        };

                        ui.add_space(16.0);
                        let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
                        ui.painter().circle_filled(rect.center(), 5.0, dot_color);
                        ui.label(egui::RichText::new(mode_text).color(dot_color).size(12.0).strong());
                    });
                });

                ui.add_space(14.0);
                ui.separator();
                ui.add_space(10.0);

                // Tab buttons with Icons
                let tab_btn = |ui: &mut egui::Ui, text: &str, active: bool| -> bool {
                    let fill = if active { accent } else { if is_dark { egui::Color32::from_rgb(26, 26, 26) } else { egui::Color32::from_rgb(230, 230, 235) } };
                    let text_clr = if active { egui::Color32::WHITE } else { text_color };
                    let btn = egui::Button::new(egui::RichText::new(text).color(text_clr).size(14.0).strong())
                        .fill(fill)
                        .min_size(egui::vec2(190.0, 42.0));
                    ui.add(btn).clicked()
                };

                if tab_btn(ui, "🖱  Main", self.current_tab == AppTab::Main) {
                    self.current_tab = AppTab::Main;
                }
                ui.add_space(6.0);
                if tab_btn(ui, "⚡  Advanced", self.current_tab == AppTab::Advanced) {
                    self.current_tab = AppTab::Advanced;
                }
                ui.add_space(6.0);
                if tab_btn(ui, "⌨  Macro", self.current_tab == AppTab::Macro) {
                    self.current_tab = AppTab::Macro;
                }
                ui.add_space(6.0);
                if tab_btn(ui, "💡  Lighting", self.current_tab == AppTab::Lighting) {
                    self.current_tab = AppTab::Lighting;
                }
                ui.add_space(6.0);
                if tab_btn(ui, "⚙  Settings", self.current_tab == AppTab::Settings) {
                    self.current_tab = AppTab::Settings;
                }
            });

        // 2. BOTTOM ACTION BAR (Import/Export JSON or JMK, Restore, Apply, Cancel)
        egui::TopBottomPanel::bottom("bottom_bar")
            .exact_height(54.0)
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    if ui.button("Import Profile").clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("Redragon Profile (*.json, *.jmk)", &["json", "jmk"])
                            .pick_file() {
                            if let Ok(bytes) = fs::read(&path) {
                                let mut imported = false;
                                if let Ok(json_str) = String::from_utf8(bytes.clone()) {
                                    if let Ok(prof) = serde_json::from_str::<MouseProfile>(&json_str) {
                                        self.profiles[self.active_profile_idx] = prof;
                                        self.status_msg = "Profile loaded from JSON!".to_string();
                                        self.add_log(LogLevel::Success, format!("Profile loaded from JSON file: {:?}", path.file_name().unwrap_or_default()));
                                        imported = true;
                                    }
                                }
                                if !imported {
                                    if let Some(loaded_jmk) = parse_jmk_bytes(&bytes) {
                                        self.profiles[self.active_profile_idx] = loaded_jmk;
                                        self.status_msg = "Original JMK profile loaded successfully!".to_string();
                                        self.add_log(LogLevel::Success, format!("Original JMK binary profile loaded: {:?}", path.file_name().unwrap_or_default()));
                                        imported = true;
                                    }
                                }
                                if !imported {
                                    self.status_msg = "Unrecognized profile format.".to_string();
                                    self.add_log(LogLevel::Error, "Failed to parse imported profile file format.");
                                }
                            }
                        }
                    }

                    if ui.button("Export Profile").clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .set_file_name("profile.jmk")
                            .add_filter("JMK Binary Profile (*.jmk)", &["jmk"])
                            .add_filter("JSON Profile (*.json)", &["json"])
                            .save_file() {
                            let is_jmk = path.extension().and_then(|e| e.to_str()).map(|e| e.eq_ignore_ascii_case("jmk")).unwrap_or(false);
                            if is_jmk {
                                let bytes = hardware::export_jmk_bytes(&self.profiles[self.active_profile_idx], self.active_profile_idx);
                                let _ = fs::write(&path, bytes);
                                self.status_msg = "Profile exported as OEM JMK binary format!".to_string();
                                self.add_log(LogLevel::Success, format!("Active Profile {} exported as JMK: {:?}", self.active_profile_idx + 1, path.file_name().unwrap_or_default()));
                            } else {
                                let json = serde_json::to_string_pretty(&self.profiles[self.active_profile_idx]).unwrap_or_default();
                                let _ = fs::write(&path, json);
                                self.status_msg = "Profile exported!".to_string();
                                self.add_log(LogLevel::Success, format!("Active Profile {} exported to JSON: {:?}", self.active_profile_idx + 1, path.file_name().unwrap_or_default()));
                            }
                        }
                    }

                    ui.separator();

                    if ui.button("Restore").clicked() {
                        self.profiles[self.active_profile_idx] = MouseProfile::default();
                        self.status_msg = "Reset to default profile.".to_string();
                        self.add_log(LogLevel::Info, format!("Profile {} restored to factory default parameters.", self.active_profile_idx + 1));
                    }

                    let is_applying = self.apply_progress.is_some();
                    let apply_btn = egui::Button::new(
                        egui::RichText::new(if is_applying { "Applying..." } else { "Apply" })
                            .color(egui::Color32::WHITE)
                            .strong(),
                    ).fill(if is_applying { egui::Color32::from_rgb(100, 100, 100) } else { accent });

                    if ui.add_enabled(!is_applying, apply_btn).clicked() {
                        let (tx, rx) = std::sync::mpsc::channel();
                        self.apply_rx = Some(rx);
                        self.apply_progress = Some((0.05, "Initializing hardware sync...".to_string()));
                        self.status_msg = "Applying settings to mouse...".to_string();

                        let profile_to_apply = self.profiles[self.active_profile_idx].clone();
                        let active_idx = self.active_profile_idx;
                        let vid = self.bridge.vid;

                        std::thread::spawn(move || {
                            let tx_cb = tx.clone();
                            let res = HardwareBridge::apply_profile_direct(vid, &profile_to_apply, active_idx, move |pct, step| {
                                let _ = tx_cb.send((pct, step.to_string()));
                            });
                            match res {
                                Ok(_) => {
                                    let _ = tx.send((1.0, "SUCCESS: Settings saved to hardware EEPROM!".to_string()));
                                }
                                Err(e) => {
                                    let _ = tx.send((-1.0, format!("ERROR: {}", e)));
                                }
                            }
                        });
                    }

                    if ui.button("Cancel").clicked() {
                        self.status_msg = "Operation cancelled.".to_string();
                        self.add_log(LogLevel::Info, "Changes cancelled by user.");
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(egui::RichText::new(&self.status_msg).color(egui::Color32::from_rgb(0, 200, 255)));
                    });
                });
            });

        // 3. MAIN CENTER WORKSPACE
        let mut switched_dpi_stage = None;
        egui::CentralPanel::default().show(ctx, |ui| {
            let active_profile = &mut self.profiles[self.active_profile_idx];

            // Top Profile selector tabs (P1 - P5)
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Profile:").strong());
                for i in 0..5 {
                    let is_active = self.active_profile_idx == i;
                    let fill = if is_active { accent } else { egui::Color32::TRANSPARENT };
                    let btn = egui::Button::new(egui::RichText::new(format!("Profile {}", i + 1)).strong())
                        .fill(fill);
                    if ui.add(btn).clicked() {
                        self.active_profile_idx = i;
                    }
                }
            });
            ui.separator();

            match self.current_tab {
                AppTab::Main => {
                    // MOUSE VIEW WITH DYNAMIC HIGHLIGHT PINS ON MOUSE IMAGE
                    ui.vertical_centered(|ui| {
                        if let Some(texture) = &self.mouse_texture {
                            let mouse_size = egui::vec2(340.0, 234.0);
                            let (rect, _resp) = ui.allocate_exact_size(mouse_size, egui::Sense::hover());
                            ui.painter().image(
                                texture.id(),
                                rect,
                                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                                egui::Color32::WHITE,
                            );

                            for b in 1..=8 {
                                let (rel_x, rel_y) = BUTTON_COORDS[b - 1];
                                let pin_center = egui::pos2(rect.min.x + (rect.width() * rel_x), rect.min.y + (rect.height() * rel_y));
                                let is_selected = active_profile.selected_button == b;

                                // Interactive clickable rect for the button on the mouse
                                let pin_rect = egui::Rect::from_center_size(pin_center, egui::vec2(26.0, 26.0));
                                let pin_resp = ui.allocate_rect(pin_rect, egui::Sense::click());
                                if pin_resp.clicked() {
                                    active_profile.selected_button = b;
                                }

                                if is_selected {
                                    // Highlight active selected button with prominent glowing ring and fill
                                    ui.painter().circle_filled(pin_center, 14.0, egui::Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 180));
                                    ui.painter().circle_stroke(pin_center, 14.5, egui::Stroke::new(2.5_f32, egui::Color32::WHITE));
                                    ui.painter().circle_stroke(pin_center, 16.5, egui::Stroke::new(1.5_f32, accent));
                                    ui.painter().text(
                                        pin_center,
                                        egui::Align2::CENTER_CENTER,
                                        format!("{}", b),
                                        egui::FontId::proportional(12.0),
                                        egui::Color32::WHITE,
                                    );
                                } else if pin_resp.hovered() {
                                    // Subtle hover ring
                                    ui.painter().circle_stroke(pin_center, 13.0, egui::Stroke::new(1.8_f32, egui::Color32::from_rgb(220, 220, 220)));
                                }
                            }
                        }
                    });

                    ui.add_space(8.0);
                    ui.group(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("Key setting").strong().size(14.0));
                            for b in 1..=8 {
                                let is_sel = active_profile.selected_button == b;
                                let fill = if is_sel { accent } else { if is_dark { egui::Color32::from_rgb(32, 32, 32) } else { egui::Color32::from_rgb(220, 220, 225) } };
                                let text_c = if is_sel { egui::Color32::WHITE } else { text_color };
                                let b_btn = egui::Button::new(egui::RichText::new(format!("{}", b)).color(text_c).strong())
                                    .fill(fill)
                                    .min_size(egui::vec2(26.0, 26.0));
                                if ui.add(b_btn).clicked() {
                                    active_profile.selected_button = b;
                                }
                            }
                        });

                        ui.add_space(6.0);
                        let b_idx = active_profile.selected_button.saturating_sub(1);
                        ui.horizontal(|ui| {
                            ui.label(format!("Button {} Action:", active_profile.selected_button));
                            let available_actions = [
                                "Left button",
                                "Right button",
                                "Middle button",
                                "Forward",
                                "Back",
                                "Fire key",
                                "RGB ON/OFF",
                                "DPI +",
                                "DPI -",
                            ];
                            egui::ComboBox::from_id_salt("key_action_select")
                                .selected_text(&active_profile.button_actions[b_idx])
                                .show_ui(ui, |ui| {
                                    for action in available_actions.iter() {
                                        if ui.selectable_value(&mut active_profile.button_actions[b_idx], action.to_string(), *action).clicked() {
                                            // Action changed
                                        }
                                    }
                                });
                        });

                        ui.add_space(10.0);
                        ui.separator();
                        ui.add_space(6.0);

                        // USB Polling Rate slider
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("USB Polling Rate").strong());
                            let mut rate_val = match active_profile.polling_rate {
                                PollingRate::Hz125 => 1,
                                PollingRate::Hz250 => 2,
                                PollingRate::Hz500 => 3,
                                PollingRate::Hz1000 => 4,
                            };
                            let slider = egui::Slider::new(&mut rate_val, 1..=4)
                                .custom_formatter(|n, _| match n as i32 {
                                    1 => "125 Hz".to_string(),
                                    2 => "250 Hz".to_string(),
                                    3 => "500 Hz".to_string(),
                                    _ => "1000 Hz".to_string(),
                                });
                            if ui.add(slider).changed() {
                                active_profile.polling_rate = match rate_val {
                                    1 => PollingRate::Hz125,
                                    2 => PollingRate::Hz250,
                                    3 => PollingRate::Hz500,
                                    _ => PollingRate::Hz1000,
                                };
                            }
                        });

                        ui.add_space(8.0);
                        // Battery Bar with Live Hardware Percentage & Slim Animated Indicator
                        ui.horizontal(|ui| {
                            let label_prefix = if self.bridge.is_charging { "Battery (Charging ⚡):" } else { "Battery:" };
                            ui.label(egui::RichText::new(label_prefix).strong());

                            let t = ctx.input(|i| i.time);
                            let bat = self.bridge.battery_percent;

                            let (rect, _response) = ui.allocate_exact_size(egui::vec2(220.0, 8.0), egui::Sense::hover());
                            ui.painter().rect_filled(rect, 4.0, egui::Color32::from_rgb(28, 30, 36));

                            let dark_green = egui::Color32::from_rgb(27, 94, 32); // Darker green #1B5E20
                            let gold = egui::Color32::from_rgb(255, 170, 0);

                            if self.bridge.is_charging {
                                // Smooth wave shimmer during charging
                                let cycle = ((t * 0.8).fract() as f32) * rect.width();
                                let fill_w = (cycle + 40.0).min(rect.width());
                                let fill_rect = egui::Rect::from_min_size(rect.min, egui::vec2(fill_w, rect.height()));
                                ui.painter().rect_filled(fill_rect, 4.0, gold);
                                ui.label(egui::RichText::new(format!("{} % (Charging)", bat))
                                    .color(gold)
                                    .size(12.0)
                                    .strong());
                            } else {
                                let fill_w = (rect.width() * (bat as f32 / 100.0)).clamp(4.0, rect.width());
                                let fill_rect = egui::Rect::from_min_size(rect.min, egui::vec2(fill_w, rect.height()));
                                ui.painter().rect_filled(fill_rect, 4.0, dark_green);
                                ui.label(egui::RichText::new(format!("{} %", bat))
                                    .color(dark_green)
                                    .size(12.0)
                                    .strong());
                            }
                        });
                    });
                }

                AppTab::Advanced => {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        ui.heading("DPI Stages & Sensor Calibration");
                        ui.add_space(8.0);

                        ui.horizontal(|ui| {
                            ui.label("DPI Effect:");
                            egui::ComboBox::from_id_salt("dpi_effect_combo")
                                .selected_text(match active_profile.dpi_effect {
                                    DpiEffect::Steady => "Steady",
                                    DpiEffect::Respiration => "Respiration",
                                    DpiEffect::Off => "OFF",
                                })
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut active_profile.dpi_effect, DpiEffect::Off, "OFF");
                                    ui.selectable_value(&mut active_profile.dpi_effect, DpiEffect::Steady, "Steady");
                                    ui.selectable_value(&mut active_profile.dpi_effect, DpiEffect::Respiration, "Respiration");
                                });
                        });
                        ui.add_space(6.0);

                        let mut stage_click = None;
                        for (idx, stage) in active_profile.dpi_stages.iter_mut().enumerate() {
                            ui.horizontal(|ui| {
                                let is_current = active_profile.current_stage == (idx + 1);
                                if ui.selectable_label(is_current, format!("Stage {}", idx + 1)).clicked() {
                                    stage_click = Some((idx + 1, stage.x, stage.color_rgb));
                                }

                                ui.checkbox(&mut stage.enabled, "");

                                let mut c = egui::Color32::from_rgb(stage.color_rgb[0], stage.color_rgb[1], stage.color_rgb[2]);
                                if egui::color_picker::color_edit_button_srgba(ui, &mut c, egui::color_picker::Alpha::Opaque).changed() {
                                    stage.color_rgb = [c.r(), c.g(), c.b()];
                                }

                                if ui.add_sized([300.0, 18.0], egui::Slider::new(&mut stage.x, 100..=10000).step_by(100.0)).changed() {
                                    if is_current {
                                        stage_click = Some((idx + 1, stage.x, stage.color_rgb));
                                    }
                                }
                                stage.y = stage.x;

                                ui.label(egui::RichText::new(format!("{} DPI", stage.x)).color(accent).strong());
                            });
                            ui.add_space(4.0);
                        }

                        if let Some((stg, dpi_val, clr)) = stage_click {
                            active_profile.current_stage = stg;
                            switched_dpi_stage = Some((stg, dpi_val, clr));
                        }

                        ui.add_space(14.0);
                        ui.separator();
                        ui.add_space(10.0);

                        ui.heading("Mouse Parameter Tuning");
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            ui.label("Mouse sensitivity:");
                            ui.add_sized([220.0, 18.0], egui::Slider::new(&mut active_profile.mouse_sensitivity, 1..=20));
                            ui.label(format!("{}", active_profile.mouse_sensitivity));
                        });
                        ui.checkbox(&mut active_profile.enhance_pointer_precision, "Improve pointer precision");

                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            ui.label("Scrolling speed:");
                            ui.add_sized([220.0, 18.0], egui::Slider::new(&mut active_profile.scroll_speed, 1..=10));
                            ui.label(format!("{}", active_profile.scroll_speed));
                        });
                        ui.checkbox(&mut active_profile.scroll_one_page, "Scroll one page");

                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            ui.label("Double-click speed:");
                            ui.label("Slow");
                            ui.add_sized([180.0, 18.0], egui::Slider::new(&mut active_profile.double_click_speed, 1..=10));
                            ui.label("Fast");
                        });
                    });
                }

                AppTab::Macro => {
                    ui.columns(2, |columns| {
                        // Left column: Macro List & Slots
                        columns[0].vertical(|ui| {
                            let slots_used = self.macros.len();
                            let slots_remaining = 16usize.saturating_sub(slots_used);
                            let slot_badge_color = if slots_used >= 16 {
                                egui::Color32::from_rgb(220, 53, 69) // Red (Full)
                            } else if slots_used >= 12 {
                                egui::Color32::from_rgb(255, 165, 0) // Orange (Near limit)
                            } else {
                                egui::Color32::from_rgb(40, 167, 69) // Green (Available)
                            };

                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new("Macro list").strong().size(14.0));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    ui.label(
                                        egui::RichText::new(format!("{}/16 Slots ({} free)", slots_used, slots_remaining))
                                            .size(11.0)
                                            .color(slot_badge_color)
                                            .strong()
                                    );
                                });
                            });

                            // Hardware EEPROM Storage Allocation Bar
                            let slot_ratio = (slots_used as f32) / 16.0;
                            ui.add(
                                egui::ProgressBar::new(slot_ratio)
                                    .desired_height(4.0)
                                    .fill(slot_badge_color)
                            );
                            ui.add_space(4.0);

                            egui::Frame::group(ui.style())
                                .inner_margin(8.0)
                                .show(ui, |ui| {
                                    egui::ScrollArea::vertical()
                                        .id_salt("macro_list_scroll_area")
                                        .min_scrolled_height(230.0)
                                        .max_height(250.0)
                                        .show(ui, |ui| {
                                            if self.macros.is_empty() {
                                                ui.label(egui::RichText::new("No macros created. Create one below.").italics().weak());
                                            }
                                            for (idx, m) in self.macros.iter().enumerate() {
                                                let is_sel = self.selected_macro_idx == idx;
                                                let label_text = format!("[Slot {:02}] {} ({} evts)", idx + 1, m.name, m.key_events.len());
                                                if ui.selectable_label(is_sel, label_text).clicked() {
                                                    self.selected_macro_idx = idx;
                                                    self.selected_key_event_idx = None;
                                                }
                                            }
                                        });
                                });

                            ui.add_space(8.0);

                            // Macro Name Input & Character Counter (Max 15 chars per OEM EEPROM spec)
                            if self.new_macro_name.chars().count() > 15 {
                                self.new_macro_name = self.new_macro_name.chars().take(15).collect();
                            }
                            let char_count = self.new_macro_name.chars().count();
                            let char_color = if char_count >= 15 {
                                egui::Color32::from_rgb(255, 140, 0)
                            } else {
                                egui::Color32::GRAY
                            };

                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new("Name (max 15 chars):").size(11.0).weak());
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    ui.label(egui::RichText::new(format!("{}/15", char_count)).size(10.5).color(char_color).monospace());
                                });
                            });

                            ui.horizontal(|ui| {
                                let name_edit = egui::TextEdit::singleline(&mut self.new_macro_name)
                                    .char_limit(15)
                                    .hint_text("Macro Name")
                                    .desired_width(115.0);
                                ui.add(name_edit);

                                let can_create = slots_used < 16 && !self.new_macro_name.trim().is_empty();
                                let mut new_btn = ui.add_enabled(can_create, egui::Button::new("+ New"));
                                if slots_used >= 16 {
                                    new_btn = new_btn.on_disabled_hover_text("Hardware storage limit reached: Mouse EEPROM stores up to 16 macros maximum.");
                                } else if self.new_macro_name.trim().is_empty() {
                                    new_btn = new_btn.on_disabled_hover_text("Enter a name (1..15 characters) to create a macro.");
                                }

                                if new_btn.clicked() && can_create {
                                    let name: String = self.new_macro_name.trim().chars().take(15).collect();
                                    self.macros.push(MacroItem {
                                        name: name.clone(),
                                        key_events: Vec::new(),
                                    });
                                    self.selected_macro_idx = self.macros.len() - 1;
                                    self.selected_key_event_idx = None;
                                    self.new_macro_name.clear();
                                    self.add_log(LogLevel::Info, format!("Created macro '[Slot {:02}] {}' (Storage: {}/16)", self.macros.len(), name, self.macros.len()));
                                }

                                if ui.button("Rename").clicked() {
                                    if !self.new_macro_name.trim().is_empty() && self.selected_macro_idx < self.macros.len() {
                                        let new_name: String = self.new_macro_name.trim().chars().take(15).collect();
                                        let old_name = self.macros[self.selected_macro_idx].name.clone();
                                        self.macros[self.selected_macro_idx].name = new_name.clone();
                                        self.new_macro_name.clear();
                                        self.add_log(LogLevel::Info, format!("Renamed macro '{}' to '{}'", old_name, new_name));
                                    }
                                }

                                if ui.button("Delete").clicked() {
                                    if !self.macros.is_empty() && self.selected_macro_idx < self.macros.len() {
                                        let removed = self.macros.remove(self.selected_macro_idx);
                                        if self.selected_macro_idx > 0 {
                                            self.selected_macro_idx -= 1;
                                        }
                                        self.selected_key_event_idx = None;
                                        self.add_log(LogLevel::Info, format!("Deleted macro '{}' (Freed slot, {}/16 used)", removed.name, self.macros.len()));
                                    }
                                }
                            });
                        });

                        // Right column: Key List & Actions
                        columns[1].vertical(|ui| {
                            let evt_count = self.macros.get(self.selected_macro_idx).map(|m| m.key_events.len()).unwrap_or(0);
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new("Key list").strong().size(14.0));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    ui.label(egui::RichText::new(format!("{} recorded events", evt_count)).size(11.0).weak());
                                });
                            });
                            ui.add_space(4.0);

                            egui::Frame::group(ui.style())
                                .inner_margin(8.0)
                                .show(ui, |ui| {
                                    egui::ScrollArea::vertical()
                                        .id_salt("key_list_scroll_area")
                                        .min_scrolled_height(170.0)
                                        .max_height(190.0)
                                        .show(ui, |ui| {
                                            if let Some(m) = self.macros.get(self.selected_macro_idx) {
                                                if m.key_events.is_empty() {
                                                    ui.label(egui::RichText::new("No recorded keys yet.").italics());
                                                } else {
                                                    for (e_idx, evt) in m.key_events.iter().enumerate() {
                                                        let is_sel = self.selected_key_event_idx == Some(e_idx);
                                                        let label_txt = format!("{:02}. {}", e_idx + 1, evt);
                                                        if ui.selectable_label(is_sel, label_txt).clicked() {
                                                            self.selected_key_event_idx = Some(e_idx);
                                                        }
                                                    }
                                                }
                                            }
                                        });
                                });

                            ui.add_space(6.0);
                            ui.horizontal(|ui| {
                                let rec_text = if self.macro_recording { "⏹ Stop record" } else { "⏺ Start record" };
                                let rec_btn = egui::Button::new(egui::RichText::new(rec_text).color(egui::Color32::WHITE).strong())
                                    .fill(if self.macro_recording { egui::Color32::from_rgb(255, 140, 0) } else { accent });
                                if ui.add(rec_btn).clicked() {
                                    self.macro_recording = !self.macro_recording;
                                    self.last_record_event_time = Instant::now();
                                    if self.macro_recording {
                                        self.add_log(LogLevel::Info, "Macro recorder started (listening for keys)...");
                                    } else {
                                        self.add_log(LogLevel::Info, "Macro recording stopped.");
                                    }
                                }

                                if ui.button("Clear").clicked() {
                                    let mut cleared_name = None;
                                    if let Some(m) = self.macros.get_mut(self.selected_macro_idx) {
                                        m.key_events.clear();
                                        cleared_name = Some(m.name.clone());
                                    }
                                    self.selected_key_event_idx = None;
                                    if let Some(name) = cleared_name {
                                        self.add_log(LogLevel::Info, format!("Cleared events in macro '{}'", name));
                                    }
                                }

                                if ui.button("Delete Event").clicked() {
                                    if let Some(m) = self.macros.get_mut(self.selected_macro_idx) {
                                        if let Some(idx) = self.selected_key_event_idx {
                                            if idx < m.key_events.len() {
                                                let del = m.key_events.remove(idx);
                                                self.selected_key_event_idx = None;
                                                self.add_log(LogLevel::Info, format!("Removed event '{}'", del));
                                            }
                                        }
                                    }
                                }
                            });

                            ui.add_space(6.0);
                            // Insert Event Controls
                            ui.group(|ui| {
                                ui.label(egui::RichText::new("Insert Event").strong());
                                ui.horizontal(|ui| {
                                    egui::ComboBox::from_id_salt("insert_event_type_combo")
                                        .selected_text(&self.insert_event_type)
                                        .show_ui(ui, |ui| {
                                            ui.selectable_value(&mut self.insert_event_type, "Delay (ms)".to_string(), "Delay (ms)");
                                            ui.selectable_value(&mut self.insert_event_type, "Key Down".to_string(), "Key Down");
                                            ui.selectable_value(&mut self.insert_event_type, "Key Up".to_string(), "Key Up");
                                            ui.selectable_value(&mut self.insert_event_type, "Mouse Left Down".to_string(), "Mouse Left Down");
                                            ui.selectable_value(&mut self.insert_event_type, "Mouse Left Up".to_string(), "Mouse Left Up");
                                            ui.selectable_value(&mut self.insert_event_type, "Mouse Right Down".to_string(), "Mouse Right Down");
                                            ui.selectable_value(&mut self.insert_event_type, "Mouse Right Up".to_string(), "Mouse Right Up");
                                        });

                                    ui.add(egui::TextEdit::singleline(&mut self.insert_event_param).desired_width(60.0));

                                    if ui.button("+ Insert").clicked() {
                                        let evt_str = match self.insert_event_type.as_str() {
                                            "Delay (ms)" => format!("Delay: {}ms", self.insert_event_param.trim()),
                                            "Key Down" => format!("Key Down: {}", self.insert_event_param.trim()),
                                            "Key Up" => format!("Key Up: {}", self.insert_event_param.trim()),
                                            "Mouse Left Down" => "Key Down: L-Button".to_string(),
                                            "Mouse Left Up" => "Key Up: L-Button".to_string(),
                                            "Mouse Right Down" => "Key Down: R-Button".to_string(),
                                            "Mouse Right Up" => "Key Up: R-Button".to_string(),
                                            _ => format!("{}: {}", self.insert_event_type, self.insert_event_param.trim()),
                                        };

                                        let mut inserted_info = None;
                                        if let Some(m) = self.macros.get_mut(self.selected_macro_idx) {
                                            if let Some(idx) = self.selected_key_event_idx {
                                                m.key_events.insert(idx.min(m.key_events.len()), evt_str.clone());
                                            } else {
                                                m.key_events.push(evt_str.clone());
                                            }
                                            inserted_info = Some((evt_str, m.name.clone()));
                                        }
                                        if let Some((evt, name)) = inserted_info {
                                            self.add_log(LogLevel::Info, format!("Inserted event '{}' into macro '{}'", evt, name));
                                        }
                                    }
                                });
                            });

                            ui.add_space(4.0);
                            ui.checkbox(&mut self.auto_record_delay, "Automatic recording time delay");
                            ui.radio_value(&mut self.macro_loop_mode, 0, "Cycle until the key released");
                            ui.radio_value(&mut self.macro_loop_mode, 1, "Cycle until any key pressed");
                            ui.horizontal(|ui| {
                                ui.radio_value(&mut self.macro_loop_mode, 2, "Cycle times");
                                if self.macro_loop_mode == 2 {
                                    ui.add(egui::DragValue::new(&mut self.macro_cycles).range(1..=999));
                                }
                            });
                        });
                    });
                }

                AppTab::Lighting => {
                    // MOUSE VIEW IN LIGHTING TAB
                    ui.vertical_centered(|ui| {
                        if let Some(texture) = &self.mouse_texture {
                            ui.image(egui::load::SizedTexture::new(texture.id(), egui::vec2(340.0, 234.0)));
                        }
                    });

                    ui.add_space(8.0);
                    ui.group(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("LED Effect").strong());
                            egui::ComboBox::from_id_salt("led_effect_select")
                                .selected_text(match active_profile.led_effect {
                                    LedEffect::Steady => "Steady",
                                    LedEffect::Respiration => "Respiration",
                                    LedEffect::Neon => "Neon",
                                    LedEffect::Streaming => "Streaming",
                                    LedEffect::Off => "OFF",
                                })
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut active_profile.led_effect, LedEffect::Steady, "Steady");
                                    ui.selectable_value(&mut active_profile.led_effect, LedEffect::Respiration, "Respiration");
                                    ui.selectable_value(&mut active_profile.led_effect, LedEffect::Neon, "Neon");
                                    ui.selectable_value(&mut active_profile.led_effect, LedEffect::Streaming, "Streaming");
                                    ui.selectable_value(&mut active_profile.led_effect, LedEffect::Off, "OFF");
                                });

                            ui.add_space(20.0);
                            ui.label(egui::RichText::new("Color:").strong());
                            let mut c = egui::Color32::from_rgb(active_profile.led_color[0], active_profile.led_color[1], active_profile.led_color[2]);
                            if egui::color_picker::color_edit_button_srgba(ui, &mut c, egui::color_picker::Alpha::Opaque).changed() {
                                active_profile.led_color = [c.r(), c.g(), c.b()];
                            }
                        });

                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("Brightness").strong());
                            ui.add_sized([260.0, 18.0], egui::Slider::new(&mut active_profile.led_brightness, 1..=10));
                        });

                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("Animation Speed").strong());
                            ui.add_sized([260.0, 18.0], egui::Slider::new(&mut active_profile.led_speed, 1..=10));
                        });

                        ui.add_space(10.0);
                        // Standard Presets (Dark green and dark blue included)
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("Standard Presets:").strong());
                            let presets = [
                                [215, 38, 56],  // Redragon Red
                                [27, 94, 32],   // Dark Forest Green #1B5E20
                                [13, 71, 161],  // Dark Royal Blue #0D47A1
                                [0, 150, 160],  // Dark Cyan
                                [255, 140, 0],  // Amber / Orange
                                [106, 27, 154], // Deep Purple
                                [240, 240, 240],// Pure White
                            ];
                            for p in presets {
                                let clr = egui::Color32::from_rgb(p[0], p[1], p[2]);
                                let (rect, resp) = ui.allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::click());
                                ui.painter().rect_filled(rect, 3.0, clr);
                                if resp.clicked() {
                                    active_profile.led_color = p;
                                }
                            }
                        });

                        ui.add_space(8.0);
                        // User Custom Presets (up to 10 colors)
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("Custom Colors (Max 10):").strong());

                            let mut to_delete = None;
                            for (c_idx, p) in active_profile.custom_colors.iter().enumerate() {
                                let clr = egui::Color32::from_rgb(p[0], p[1], p[2]);
                                let (rect, resp) = ui.allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::click());
                                ui.painter().rect_filled(rect, 3.0, clr);
                                if resp.clicked() {
                                    active_profile.led_color = *p;
                                }
                                if resp.secondary_clicked() {
                                    to_delete = Some(c_idx);
                                }
                            }
                            if let Some(del) = to_delete {
                                active_profile.custom_colors.remove(del);
                            }

                            if active_profile.custom_colors.len() < 10 {
                                let mut tc = egui::Color32::from_rgb(self.temp_custom_color[0], self.temp_custom_color[1], self.temp_custom_color[2]);
                                if egui::color_picker::color_edit_button_srgba(ui, &mut tc, egui::color_picker::Alpha::Opaque).changed() {
                                    self.temp_custom_color = [tc.r(), tc.g(), tc.b()];
                                }

                                if ui.button("+ Add Preset").clicked() {
                                    active_profile.custom_colors.push(self.temp_custom_color);
                                }
                            }
                        });
                    });
                }

                AppTab::Settings => {
                    ui.group(|ui| {
                        ui.heading("UI Appearance & Accent Customization");
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            ui.label("Theme Mode:");
                            ui.radio_value(&mut self.theme, ThemeMode::Dark, "Dark Theme");
                            ui.radio_value(&mut self.theme, ThemeMode::Light, "Light Theme");
                        });

                        ui.add_space(10.0);
                        ui.label(egui::RichText::new("Main Accent Color (5 Presets):").strong());
                        ui.horizontal(|ui| {
                            let accent_presets = [
                                ("Redragon Red", egui::Color32::from_rgb(215, 38, 56)),
                                ("Dark Forest Green", egui::Color32::from_rgb(27, 94, 32)),
                                ("Dark Royal Blue", egui::Color32::from_rgb(13, 71, 161)),
                                ("Cyber Cyan", egui::Color32::from_rgb(0, 180, 210)),
                                ("Solar Amber", egui::Color32::from_rgb(255, 140, 0)),
                            ];

                            for (name, color) in accent_presets {
                                let (rect, resp) = ui.allocate_exact_size(egui::vec2(28.0, 28.0), egui::Sense::click());
                                let is_active = self.accent_color == color;
                                ui.painter().rect_filled(rect, 4.0, color);
                                if is_active {
                                    ui.painter().rect_stroke(rect, 4.0, egui::Stroke::new(2.5_f32, egui::Color32::WHITE));
                                }
                                if resp.on_hover_text(name).clicked() {
                                    self.accent_color = color;
                                }
                            }

                            ui.add_space(10.0);
                            ui.label("Custom Accent:");
                            let mut ac = self.accent_color;
                            if egui::color_picker::color_edit_button_srgba(ui, &mut ac, egui::color_picker::Alpha::Opaque).changed() {
                                self.accent_color = ac;
                            }
                        });
                    });

                    ui.add_space(10.0);
                    // System Tray & Windows Autostart Options
                    ui.group(|ui| {
                        ui.heading("System & Background Execution");
                        ui.add_space(6.0);

                        ui.checkbox(
                            &mut self.minimize_to_tray,
                            "Minimize to System Tray on close (keeps software running in background taskbar)",
                        );

                        if ui.checkbox(
                            &mut self.autostart_with_windows,
                            "Start with Windows (automatically launch on PC boot)",
                        ).changed() {
                            set_autostart(self.autostart_with_windows);
                            if self.autostart_with_windows {
                                self.add_log(LogLevel::Success, "Enabled auto-start with Windows on system boot (HKCU Run).");
                            } else {
                                self.add_log(LogLevel::Info, "Disabled auto-start with Windows.");
                            }
                        }
                    });

                    ui.add_space(10.0);
                    ui.group(|ui| {
                        ui.heading("Hardware Diagnostics");
                        ui.label(format!("Device Model: {}", self.bridge.device_name));
                        ui.label(format!("Vendor ID: 0x{:04X}", self.bridge.vid));
                        ui.label(format!("Product ID: 0x{:04X}", self.bridge.pid));
                        ui.label(format!("2.4G Receiver Plugged: {}", if self.bridge.found_dongle { "Yes (Dongle PID 0xFA7E found)" } else { "No" }));
                        ui.label(format!("Wired Cable Plugged: {}", if self.bridge.found_wired { "Yes (Wired PID 0xFAB0 found)" } else { "No" }));
                        ui.label(format!("RF Link Active (Mouse ON): {}", if self.bridge.handshake_ok { "Yes (MCU responding)" } else { "No (Off/Standby/No RF)" }));
                        ui.label(format!("Connection Bus: {}", if self.bridge.is_wireless { "Wireless 2.4G" } else { "Wired USB" }));
                        ui.label(format!("Battery / Power: {} %", self.bridge.battery_percent));
                        ui.label(format!("Charging State: {}", if self.bridge.is_charging { "Yes (Plugged into USB ⚡)" } else { "No (Battery power)" }));
                        ui.label(egui::RichText::new(format!("Diagnostic State: {}", self.bridge.last_status_detail)).italics().color(egui::Color32::from_rgb(180, 180, 180)));
                        if ui.button("⟳ Re-probe USB/Wireless Bus").clicked() {
                            self.bridge.detect();
                            self.profiles[self.active_profile_idx].battery_percent = self.bridge.battery_percent;
                            self.status_msg = format!("Bus rescanned: {} (Battery {}%)", self.bridge.device_name, self.bridge.battery_percent);
                            let lvl = if self.bridge.connected { LogLevel::Success } else { LogLevel::Warn };
                            self.add_log(lvl, format!("Manual bus probe: {}", self.bridge.last_status_detail));
                        }
                    });

                    ui.add_space(10.0);
                    // DEDICATED LOG CONSOLE BOX
                    ui.group(|ui| {
                        ui.horizontal(|ui| {
                            ui.heading("Activity & Hardware Event Log");
                            ui.label(egui::RichText::new(format!("({} events)", self.logs.len())).color(egui::Color32::from_rgb(160, 160, 160)));

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.button("🗑 Clear").clicked() {
                                    self.logs.clear();
                                }
                                if ui.button("📋 Copy All").clicked() {
                                    let mut full_text = String::new();
                                    for entry in &self.logs {
                                        let lvl = match entry.level {
                                            LogLevel::Info => "INFO",
                                            LogLevel::Success => "SUCCESS",
                                            LogLevel::Warn => "WARN",
                                            LogLevel::Error => "ERROR",
                                            LogLevel::Status => "STATUS",
                                        };
                                        full_text.push_str(&format!("[{}] [{}] {}\n", entry.time, lvl, entry.message));
                                    }
                                    ui.output_mut(|o| o.copied_text = full_text);
                                }
                                if ui.button("💾 Export Log").clicked() {
                                    if let Some(path) = rfd::FileDialog::new()
                                        .set_file_name("redragon_events.log")
                                        .add_filter("Log file", &["log", "txt"])
                                        .save_file() {
                                        let mut full_text = String::new();
                                        for entry in &self.logs {
                                            let lvl = match entry.level {
                                                LogLevel::Info => "INFO",
                                                LogLevel::Success => "SUCCESS",
                                                LogLevel::Warn => "WARN",
                                                LogLevel::Error => "ERROR",
                                                LogLevel::Status => "STATUS",
                                            };
                                            full_text.push_str(&format!("[{}] [{}] {}\n", entry.time, lvl, entry.message));
                                        }
                                        let _ = fs::write(path, full_text);
                                    }
                                }
                                ui.checkbox(&mut self.auto_scroll_logs, "Auto-scroll");
                            });
                        });

                        ui.add_space(4.0);
                        // Terminal Box Frame
                        let console_bg = if is_dark { egui::Color32::from_rgb(10, 11, 14) } else { egui::Color32::from_rgb(240, 242, 245) };
                        egui::Frame::none()
                            .fill(console_bg)
                            .stroke(egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(38, 42, 50)))
                            .inner_margin(8.0)
                            .rounding(4.0)
                            .show(ui, |ui| {
                                let mut scroll = egui::ScrollArea::vertical()
                                    .min_scrolled_height(140.0)
                                    .max_height(170.0);
                                if self.auto_scroll_logs {
                                    scroll = scroll.stick_to_bottom(true);
                                }

                                scroll.show(ui, |ui| {
                                    if self.logs.is_empty() {
                                        ui.label(egui::RichText::new("No logged activity yet.").monospace().color(egui::Color32::GRAY));
                                    } else {
                                        for entry in &self.logs {
                                            ui.horizontal(|ui| {
                                                ui.label(egui::RichText::new(&entry.time).monospace().color(egui::Color32::from_rgb(120, 120, 130)));

                                                let (tag, tag_clr) = match entry.level {
                                                    LogLevel::Success => ("[SUCCESS]", egui::Color32::from_rgb(0, 230, 118)),
                                                    LogLevel::Status => ("[STATUS]", egui::Color32::from_rgb(0, 229, 255)),
                                                    LogLevel::Info => ("[INFO]", egui::Color32::from_rgb(144, 202, 249)),
                                                    LogLevel::Warn => ("[WARN]", egui::Color32::from_rgb(255, 179, 0)),
                                                    LogLevel::Error => ("[ERROR]", egui::Color32::from_rgb(255, 82, 82)),
                                                };
                                                ui.label(egui::RichText::new(tag).monospace().strong().color(tag_clr));
                                                ui.label(egui::RichText::new(&entry.message).monospace().color(text_color));
                                            });
                                        }
                                    }
                                });
                            });
                    });
                }
            }
        });

        if let Some((stg, dpi_val, clr)) = switched_dpi_stage {
            self.show_dpi_toast(stg, dpi_val, clr);
            self.add_log(LogLevel::Info, format!("Switched to DPI Stage {} ({} DPI)", stg, dpi_val));
        }

        // 4. FLOATING OVERLAYS (DPI TOAST & APPLY LOADING MODAL)
        // A) Apply Progress Modal
        if let Some((pct, step_text)) = &self.apply_progress {
            egui::Area::new("apply_progress_modal_area".into())
                .order(egui::Order::Foreground)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .show(ctx, |ui| {
                    egui::Frame::none()
                        .fill(egui::Color32::from_rgba_premultiplied(18, 18, 22, 240))
                        .stroke(egui::Stroke::new(1.5_f32, accent))
                        .rounding(10.0)
                        .inner_margin(24.0)
                        .shadow(egui::epaint::Shadow {
                            offset: egui::vec2(0.0, 4.0),
                            blur: 16.0,
                            spread: 0.0,
                            color: egui::Color32::from_black_alpha(180),
                        })
                        .show(ui, |ui| {
                            ui.set_width(340.0);
                            ui.vertical_centered(|ui| {
                                ui.label(egui::RichText::new("SYNCING HARDWARE").size(15.0).strong().color(egui::Color32::WHITE));
                                ui.add_space(8.0);
                                ui.label(egui::RichText::new(step_text).size(13.0).color(egui::Color32::from_rgb(200, 200, 210)));
                                ui.add_space(14.0);
                                let pbar = egui::ProgressBar::new(*pct)
                                    .animate(true)
                                    .fill(accent)
                                    .show_percentage();
                                ui.add(pbar);
                                ui.add_space(8.0);
                                ui.label(egui::RichText::new("Writing EEPROM registers... Please do not unplug device.").size(10.5).color(egui::Color32::from_rgb(140, 140, 140)));
                            });
                        });
                });
        }

        // B) Floating DPI Switch Overlay (Left-side toast like original OEM)
        if let Some(toast_start) = self.dpi_toast_time {
            let elapsed = toast_start.elapsed().as_secs_f32();
            if elapsed < 2.5 {
                let alpha = if elapsed > 1.8 {
                    ((2.5 - elapsed) / 0.7).clamp(0.0, 1.0)
                } else {
                    1.0
                };
                let bg_color = egui::Color32::from_rgba_premultiplied(
                    (20.0 * alpha) as u8,
                    (22.0 * alpha) as u8,
                    (28.0 * alpha) as u8,
                    (230.0 * alpha) as u8,
                );
                let border_color = egui::Color32::from_rgba_premultiplied(
                    (self.dpi_toast_color.r() as f32 * alpha) as u8,
                    (self.dpi_toast_color.g() as f32 * alpha) as u8,
                    (self.dpi_toast_color.b() as f32 * alpha) as u8,
                    (255.0 * alpha) as u8,
                );

                egui::Area::new("dpi_toast_overlay".into())
                    .order(egui::Order::Foreground)
                    .anchor(egui::Align2::LEFT_CENTER, egui::vec2(24.0, 0.0))
                    .show(ctx, |ui| {
                        egui::Frame::none()
                            .fill(bg_color)
                            .stroke(egui::Stroke::new(1.5_f32, border_color))
                            .rounding(8.0)
                            .inner_margin(16.0)
                            .shadow(egui::epaint::Shadow {
                                offset: egui::vec2(0.0, 3.0),
                                blur: 12.0,
                                spread: 0.0,
                                color: egui::Color32::from_black_alpha((160.0 * alpha) as u8),
                            })
                            .show(ui, |ui| {
                                ui.set_width(170.0);
                                ui.vertical(|ui| {
                                    ui.horizontal(|ui| {
                                        let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
                                        ui.painter().circle_filled(rect.center(), 5.0, border_color);
                                        ui.label(
                                            egui::RichText::new(format!("DPI STAGE {}", self.dpi_toast_stage))
                                                .size(13.0)
                                                .strong()
                                                .color(border_color),
                                        );
                                    });
                                    ui.add_space(4.0);
                                    ui.label(
                                        egui::RichText::new(format!("{} DPI", self.dpi_toast_val))
                                            .size(22.0)
                                            .strong()
                                            .color(egui::Color32::from_rgba_premultiplied(
                                                (255.0 * alpha) as u8,
                                                (255.0 * alpha) as u8,
                                                (255.0 * alpha) as u8,
                                                (255.0 * alpha) as u8,
                                            )),
                                    );
                                    ui.add_space(6.0);
                                    // 5-segment Stage Visualizer
                                    ui.horizontal(|ui| {
                                        for s in 1..=5 {
                                            let (seg_rect, _) = ui.allocate_exact_size(egui::vec2(28.0, 5.0), egui::Sense::hover());
                                            let seg_color = if s == self.dpi_toast_stage {
                                                border_color
                                            } else {
                                                egui::Color32::from_rgba_premultiplied(
                                                    (55.0 * alpha) as u8,
                                                    (60.0 * alpha) as u8,
                                                    (70.0 * alpha) as u8,
                                                    (180.0 * alpha) as u8,
                                                )
                                            };
                                            ui.painter().rect_filled(seg_rect, 2.5, seg_color);
                                        }
                                    });
                                });
                            });
                    });
                ctx.request_repaint();
            } else {
                self.dpi_toast_time = None;
            }
        }
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([900.0, 680.0])
            .with_min_inner_size([840.0, 600.0])
            .with_title("Redragon INVADER PRO (M719RGB-PRO) Suite"),
        ..Default::default()
    };

    eframe::run_native(
        "Redragon INVADER PRO Suite",
        options,
        Box::new(|cc| {
            // Register lightweight Segoe UI and Segoe UI Symbol fonts from Windows (< 1MB each)
            let mut fonts = egui::FontDefinitions::default();
            if let Ok(font_data) = std::fs::read("C:\\Windows\\Fonts\\segoeui.ttf") {
                fonts.font_data.insert(
                    "segoeui".to_owned(),
                    egui::FontData::from_owned(font_data),
                );
                fonts
                    .families
                    .entry(egui::FontFamily::Proportional)
                    .or_default()
                    .insert(0, "segoeui".to_owned());
            }
            if let Ok(font_data) = std::fs::read("C:\\Windows\\Fonts\\seguisym.ttf") {
                fonts.font_data.insert(
                    "seguisym".to_owned(),
                    egui::FontData::from_owned(font_data),
                );
                fonts
                    .families
                    .entry(egui::FontFamily::Proportional)
                    .or_default()
                    .push("seguisym".to_owned());
            }
            cc.egui_ctx.set_fonts(fonts);

            egui_extras::install_image_loaders(&cc.egui_ctx);
            Ok(Box::new(RedragonApp::default()))
        }),
    )
}
