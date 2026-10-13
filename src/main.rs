#![windows_subsystem = "windows"]

mod hardware;

use std::fs;
use std::time::Instant;
use eframe::egui;
use hardware::{parse_jmk_bytes, DpiEffect, HardwareBridge, LedEffect, MacroItem, MouseProfile, PollingRate};

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

        Self {
            bridge: HardwareBridge::new(),
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
        }
    }
}

impl eframe::App for RedragonApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Automatic background polling of USB bus every 1.5 seconds
        if self.last_poll_time.elapsed().as_millis() > 1500 {
            self.bridge.detect();
            self.profiles[self.active_profile_idx].battery_percent = self.bridge.battery_percent;
            self.last_poll_time = Instant::now();
        }

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

                            if let Some(m) = self.macros.get_mut(self.selected_macro_idx) {
                                if self.auto_record_delay && elapsed > 5 && elapsed < 10000 {
                                    m.key_events.push(format!("Delay: {}ms", elapsed));
                                }
                                if pressed {
                                    m.key_events.push(format!("Key Down: {:?}", key));
                                } else {
                                    m.key_events.push(format!("Key Up: {:?}", key));
                                }
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
                        } else {
                            (egui::Color32::from_rgb(220, 50, 50), "Disconnected")
                        };

                        ui.add_space(16.0);
                        let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
                        ui.painter().circle_filled(rect.center(), 5.0, dot_color);
                        ui.label(egui::RichText::new(mode_text).color(dot_color).size(12.0).strong());
                    });
                    ctx.request_repaint();
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
                                        imported = true;
                                    }
                                }
                                if !imported {
                                    if let Some(loaded_jmk) = parse_jmk_bytes(&bytes) {
                                        self.profiles[self.active_profile_idx] = loaded_jmk;
                                        self.status_msg = "Original JMK profile loaded successfully!".to_string();
                                        imported = true;
                                    }
                                }
                                if !imported {
                                    self.status_msg = "Unrecognized profile format.".to_string();
                                }
                            }
                        }
                    }

                    if ui.button("Export Profile").clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .set_file_name("profile.json")
                            .add_filter("JSON Profile", &["json"])
                            .save_file() {
                            let json = serde_json::to_string_pretty(&self.profiles[self.active_profile_idx]).unwrap_or_default();
                            let _ = fs::write(path, json);
                            self.status_msg = "Profile exported!".to_string();
                        }
                    }

                    ui.separator();

                    if ui.button("Restore").clicked() {
                        self.profiles[self.active_profile_idx] = MouseProfile::default();
                        self.status_msg = "Reset to default profile.".to_string();
                    }

                    let apply_btn = egui::Button::new(egui::RichText::new("Apply").color(egui::Color32::WHITE).strong())
                        .fill(accent);
                    if ui.add(apply_btn).clicked() {
                        match self.bridge.apply_profile(&self.profiles[self.active_profile_idx], self.active_profile_idx) {
                            Ok(_) => self.status_msg = "Settings applied to mouse EEPROM & hardware!".to_string(),
                            Err(e) => self.status_msg = format!("Apply error: {}", e),
                        }
                    }

                    if ui.button("Cancel").clicked() {
                        self.status_msg = "Operation cancelled.".to_string();
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(egui::RichText::new(&self.status_msg).color(egui::Color32::from_rgb(0, 200, 255)));
                    });
                });
            });

        // 3. MAIN CENTER WORKSPACE
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
                            // Native aspect ratio for 382 x 263
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
                                        ui.selectable_value(&mut active_profile.button_actions[b_idx], action.to_string(), *action);
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
                            ctx.request_repaint();
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

                        for (idx, stage) in active_profile.dpi_stages.iter_mut().enumerate() {
                            ui.horizontal(|ui| {
                                ui.checkbox(&mut stage.enabled, format!("Stage {}", idx + 1));

                                let mut c = egui::Color32::from_rgb(stage.color_rgb[0], stage.color_rgb[1], stage.color_rgb[2]);
                                if egui::color_picker::color_edit_button_srgba(ui, &mut c, egui::color_picker::Alpha::Opaque).changed() {
                                    stage.color_rgb = [c.r(), c.g(), c.b()];
                                }

                                ui.add_sized([300.0, 18.0], egui::Slider::new(&mut stage.x, 100..=10000).step_by(100.0));
                                stage.y = stage.x;

                                ui.label(egui::RichText::new(format!("{} DPI", stage.x)).color(accent).strong());
                            });
                            ui.add_space(4.0);
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
                        // Left column: Macro List
                        columns[0].vertical(|ui| {
                            ui.label(egui::RichText::new("Macro list").strong().size(14.0));
                            ui.add_space(4.0);

                            egui::Frame::group(ui.style())
                                .inner_margin(8.0)
                                .show(ui, |ui| {
                                    egui::ScrollArea::vertical()
                                        .min_scrolled_height(240.0)
                                        .max_height(260.0)
                                        .show(ui, |ui| {
                                            for (idx, m) in self.macros.iter().enumerate() {
                                                let is_sel = self.selected_macro_idx == idx;
                                                if ui.selectable_label(is_sel, &m.name).clicked() {
                                                    self.selected_macro_idx = idx;
                                                    self.selected_key_event_idx = None;
                                                }
                                            }
                                        });
                                });

                            ui.add_space(8.0);
                            ui.horizontal(|ui| {
                                ui.add(egui::TextEdit::singleline(&mut self.new_macro_name).desired_width(110.0));
                                if ui.button("+ New").clicked() {
                                    if !self.new_macro_name.trim().is_empty() {
                                        self.macros.push(MacroItem {
                                            name: self.new_macro_name.trim().to_string(),
                                            key_events: Vec::new(),
                                        });
                                        self.selected_macro_idx = self.macros.len() - 1;
                                        self.selected_key_event_idx = None;
                                        self.new_macro_name.clear();
                                    }
                                }
                                if ui.button("Delete").clicked() {
                                    if !self.macros.is_empty() && self.selected_macro_idx < self.macros.len() {
                                        self.macros.remove(self.selected_macro_idx);
                                        if self.selected_macro_idx > 0 {
                                            self.selected_macro_idx -= 1;
                                        }
                                        self.selected_key_event_idx = None;
                                    }
                                }
                            });
                        });

                        // Right column: Key List & Actions
                        columns[1].vertical(|ui| {
                            ui.label(egui::RichText::new("Key list").strong().size(14.0));
                            ui.add_space(4.0);

                            egui::Frame::group(ui.style())
                                .inner_margin(8.0)
                                .show(ui, |ui| {
                                    egui::ScrollArea::vertical()
                                        .min_scrolled_height(170.0)
                                        .max_height(190.0)
                                        .show(ui, |ui| {
                                            if let Some(m) = self.macros.get(self.selected_macro_idx) {
                                                if m.key_events.is_empty() {
                                                    ui.label(egui::RichText::new("No recorded keys yet.").italics());
                                                } else {
                                                    for (e_idx, evt) in m.key_events.iter().enumerate() {
                                                        let is_sel = self.selected_key_event_idx == Some(e_idx);
                                                        if ui.selectable_label(is_sel, evt).clicked() {
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
                                }

                                if ui.button("Clear").clicked() {
                                    if let Some(m) = self.macros.get_mut(self.selected_macro_idx) {
                                        m.key_events.clear();
                                        self.selected_key_event_idx = None;
                                    }
                                }

                                if ui.button("Delete Event").clicked() {
                                    if let Some(m) = self.macros.get_mut(self.selected_macro_idx) {
                                        if let Some(idx) = self.selected_key_event_idx {
                                            if idx < m.key_events.len() {
                                                m.key_events.remove(idx);
                                                self.selected_key_event_idx = None;
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

                                        if let Some(m) = self.macros.get_mut(self.selected_macro_idx) {
                                            if let Some(idx) = self.selected_key_event_idx {
                                                m.key_events.insert(idx.min(m.key_events.len()), evt_str);
                                            } else {
                                                m.key_events.push(evt_str);
                                            }
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
                            ui.radio_value(&mut self.theme, ThemeMode::Dark, "🌙 Dark Theme (#101010 OLED)");
                            ui.radio_value(&mut self.theme, ThemeMode::Light, "☀ Light Theme");
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

                    ui.add_space(12.0);
                    ui.group(|ui| {
                        ui.heading("Hardware Diagnostics");
                        ui.label(format!("Device Model: {}", self.bridge.device_name));
                        ui.label(format!("Vendor ID: 0x{:04X}", self.bridge.vid));
                        ui.label(format!("Product ID: 0x{:04X}", self.bridge.pid));
                        ui.label(format!("Connection Bus: {}", if self.bridge.is_wireless { "Wireless 2.4G" } else { "Wired USB" }));
                        ui.label(format!("Battery / Power: {} %", self.bridge.battery_percent));
                        ui.label(format!("Charging State: {}", if self.bridge.is_charging { "Yes (Plugged into USB ⚡)" } else { "No (Battery power)" }));
                        if ui.button("⟳ Re-probe USB/Wireless Bus").clicked() {
                            self.bridge.detect();
                            self.profiles[self.active_profile_idx].battery_percent = self.bridge.battery_percent;
                            self.status_msg = format!("Bus rescanned: {} (Battery {}%)", self.bridge.device_name, self.bridge.battery_percent);
                        }
                    });
                }
            }
        });
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([900.0, 620.0])
            .with_min_inner_size([840.0, 560.0])
            .with_title("Redragon INVADER PRO (M719RGB-PRO) Suite"),
        ..Default::default()
    };

    eframe::run_native(
        "Redragon INVADER PRO Suite",
        options,
        Box::new(|cc| {
            // Register Segoe UI Emoji from Windows so all emoji icons render cleanly without [] boxes
            let mut fonts = egui::FontDefinitions::default();
            if let Ok(font_data) = std::fs::read("C:\\Windows\\Fonts\\seguiemj.ttf") {
                fonts.font_data.insert(
                    "seguiemj".to_owned(),
                    egui::FontData::from_owned(font_data),
                );
                fonts
                    .families
                    .entry(egui::FontFamily::Proportional)
                    .or_default()
                    .push("seguiemj".to_owned());
            }
            cc.egui_ctx.set_fonts(fonts);

            egui_extras::install_image_loaders(&cc.egui_ctx);
            Ok(Box::new(RedragonApp::default()))
        }),
    )
}
