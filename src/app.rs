use std::{
    path::{Path, PathBuf},
    sync::atomic::Ordering,
};

use anyhow::Error;
use eframe::{
    egui::{self, Checkbox, Style, ViewportCommand},
    epaint::{Rounding, Shadow},
};
use egui_modal::Modal;

use crate::{
    bin_file::BinFile,
    config::{read_json_config, write_json_config, Config, FileConfig},
    diff_state::DiffState,
    hex_view::{HexView, HexViewSelection, HexViewSelectionSide, HexViewSelectionState},
    settings::{read_json_settings, write_json_settings, ByteGrouping, Settings},
};

#[derive(Default)]
struct GotoModal {
    value: String,
    status: String,
}

#[derive(Default)]
struct OverwriteModal {
    open: bool,
}

struct Options {
    mirror_selection: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            mirror_selection: true,
        }
    }
}

#[derive(Default)]
pub struct BdiffApp {
    next_hv_id: usize,
    hex_views: Vec<HexView>,
    diff_state: DiffState,
    goto_modal: GotoModal,
    overwrite_modal: OverwriteModal,
    scroll_overflow: f32,
    options: Options,
    global_selection: HexViewSelection, // the selection that all hex views will mirror
    selecting_hv: Option<usize>,
    last_selected_hv: Option<usize>,
    settings_open: bool,
    settings: Settings,
    config: Config,
    started_with_arguments: bool,
}

impl BdiffApp {
    pub fn new(cc: &eframe::CreationContext<'_>, paths: Vec<PathBuf>) -> Self {
        set_up_custom_fonts(&cc.egui_ctx);

        let hex_views = Vec::new();

        let settings = if let Ok(settings) = read_json_settings() {
            settings
        } else {
            let sett = Settings::default();
            write_json_settings(&sett)
                .expect("Failed to write empty settings to the settings file!");
            sett
        };

        let started_with_arguments = !paths.is_empty();

        let mut ret = Self {
            next_hv_id: 0,
            hex_views,
            settings,
            started_with_arguments,
            ..Default::default()
        };

        log::info!("Loading project config from file");
        let config_path = Path::new("bdiff.json");

        let config = if started_with_arguments {
            let file_configs = paths
                .into_iter()
                .map(|a| a.into())
                .collect::<Vec<FileConfig>>();

            Config {
                files: file_configs,
                changed: true,
            }
        } else if config_path.exists() {
            read_json_config(config_path).unwrap()
        } else {
            Config::default()
        };

        for file in config.files.iter() {
            match ret.open_file(&file.path) {
                Ok(hv) => {
                    if let Some(map) = file.map.as_ref() {
                        hv.mt.load_file(map);
                    }
                }
                Err(e) => {
                    log::error!("Failed to open file: {}", e);
                }
            }
        }

        ret.config = config;

        ret.diff_state.recalculate(&ret.hex_views);

        ret
    }

    pub fn open_file(&mut self, path: &Path) -> Result<&mut HexView, Error> {
        let file = BinFile::from_path(path)?;
        self.config.files.push(path.into());
        self.config.changed = true;

        let hv = HexView::new(file, self.next_hv_id);
        self.hex_views.push(hv);
        self.next_hv_id += 1;

        Ok(self.hex_views.last_mut().unwrap())
    }

    fn get_hex_view_by_id(&mut self, id: usize) -> Option<&mut HexView> {
        self.hex_views.iter_mut().find(|hv| hv.id == id)
    }

    fn handle_hex_view_input(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.modifiers.shift) {
            // Move selection
            if let Some(hv) = self.last_selected_hv {
                if let Some(hv) = self.get_hex_view_by_id(hv) {
                    let mut changed = false;
                    if ctx.input(|i| i.key_pressed(egui::Key::ArrowLeft))
                        && hv.selection.start() > 0
                        && hv.selection.end() > 0
                    {
                        hv.selection.adjust_cur_pos(-1);
                        changed = true;
                    }
                    if ctx.input(|i| i.key_pressed(egui::Key::ArrowRight))
                        && hv.selection.start() < hv.file.data.len() - 1
                        && hv.selection.end() < hv.file.data.len() - 1
                    {
                        hv.selection.adjust_cur_pos(1);
                        changed = true;
                    }
                    if ctx.input(|i| i.key_pressed(egui::Key::ArrowUp))
                        && hv.selection.start() >= hv.bytes_per_row
                        && hv.selection.end() >= hv.bytes_per_row
                    {
                        hv.selection.adjust_cur_pos(-(hv.bytes_per_row as isize));
                        changed = true;
                    }
                    if ctx.input(|i| i.key_pressed(egui::Key::ArrowDown))
                        && hv.selection.start() < hv.file.data.len() - hv.bytes_per_row
                        && hv.selection.end() < hv.file.data.len() - hv.bytes_per_row
                    {
                        hv.selection.adjust_cur_pos(hv.bytes_per_row as isize);
                        changed = true;
                    }

                    if changed {
                        self.global_selection = hv.selection.clone();
                    }
                }
            }
        } else {
            // Move view
            for hv in self.hex_views.iter_mut() {
                // Keys
                if ctx.input(|i| i.key_pressed(egui::Key::Home)) {
                    hv.set_cur_pos(0);
                }
                if ctx.input(|i| i.key_pressed(egui::Key::End))
                    && hv.file.data.len() >= hv.bytes_per_screen()
                {
                    hv.set_cur_pos(hv.file.data.len() - hv.bytes_per_screen())
                }
                if ctx.input(|i| i.key_pressed(egui::Key::PageUp)) {
                    hv.adjust_cur_pos(-(hv.bytes_per_screen() as isize))
                }
                if ctx.input(|i| i.key_pressed(egui::Key::PageDown)) {
                    hv.adjust_cur_pos(hv.bytes_per_screen() as isize)
                }
                if ctx.input(|i| i.key_pressed(egui::Key::ArrowLeft)) {
                    hv.adjust_cur_pos(-1)
                }
                if ctx.input(|i| i.key_pressed(egui::Key::ArrowRight)) {
                    hv.adjust_cur_pos(1)
                }
                if ctx.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
                    hv.adjust_cur_pos(-(hv.bytes_per_row as isize))
                }
                if ctx.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
                    hv.adjust_cur_pos(hv.bytes_per_row as isize)
                }
                if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
                    let last_byte = hv.cur_pos + hv.bytes_per_screen();

                    if self.diff_state.enabled {
                        if last_byte < hv.file.data.len() {
                            match self.diff_state.get_next_diff(last_byte) {
                                Some(next_diff) => {
                                    // Move to the next diff
                                    let new_pos = next_diff - (next_diff % hv.bytes_per_row);
                                    hv.set_cur_pos(new_pos);
                                }
                                None => {
                                    // Move to the end of the file
                                    if hv.file.data.len() >= hv.bytes_per_screen() {
                                        hv.set_cur_pos(hv.file.data.len() - hv.bytes_per_screen());
                                    }
                                }
                            }
                        }
                    } else {
                        // Move one screen down
                        hv.adjust_cur_pos(hv.bytes_per_screen() as isize)
                    }
                }

                let scroll_y = ctx.input(|i| i.raw_scroll_delta.y);

                // Scrolling
                if scroll_y != 0.0 {
                    let lines_per_scroll = 1;
                    let scroll_threshold = 20; // One tick of the scroll wheel for me
                    let scroll_amt: isize;

                    if scroll_y.abs() >= scroll_threshold as f32 {
                        // Scroll wheels / very fast scrolling
                        scroll_amt = scroll_y as isize / scroll_threshold;
                        self.scroll_overflow = 0.0;
                    } else {
                        // Trackpads - Accumulate scroll amount until it reaches the threshold
                        self.scroll_overflow += scroll_y;
                        scroll_amt = self.scroll_overflow as isize / scroll_threshold;
                        if scroll_amt != 0 {
                            self.scroll_overflow -= (scroll_amt * scroll_threshold) as f32;
                        }
                    }
                    hv.adjust_cur_pos(-scroll_amt * lines_per_scroll * hv.bytes_per_row as isize)
                }
            }
        }
    }

    fn show_settings(&mut self, ctx: &egui::Context) {
        egui::Window::new("Settings")
            .default_open(true)
            .show(ctx, |ui| {
                if ui.button("Restore defaults").clicked() {
                    self.settings = Settings::default();
                    write_json_settings(&self.settings).expect("Failed to save settings!");
                }

                // Byte Grouping
                ui.horizontal(|ui| {
                    ui.label("Byte grouping");
                    egui::ComboBox::from_id_source("byte_grouping_dropdown")
                        .selected_text(self.settings.byte_grouping.to_string())
                        .show_ui(ui, |ui| {
                            for value in ByteGrouping::get_all_options() {
                                if ui
                                    .selectable_value(
                                        &mut self.settings.byte_grouping,
                                        value,
                                        value.to_string(),
                                    )
                                    .clicked()
                                {
                                    // A setting has been changed, save changes
                                    write_json_settings(&self.settings)
                                        .expect("Failed to save settings!");
                                }
                            }
                        });
                });

                egui::CollapsingHeader::new("Theme settings").show(ui, |ui| {
                    egui::Frame::group(&Style::default()).show(ui, |ui| {
                        egui::Grid::new("offset_colors").show(ui, |ui| {
                            ui.heading("Offset colors");
                            ui.end_row();

                            ui.label("Offset text color");
                            ui.color_edit_button_srgba_premultiplied(
                                self.settings
                                    .theme_settings
                                    .offset_text_color
                                    .as_bytes_mut(),
                            );
                            ui.end_row();

                            ui.label("Leading zero color");
                            ui.color_edit_button_srgba_premultiplied(
                                self.settings
                                    .theme_settings
                                    .offset_leading_zero_color
                                    .as_bytes_mut(),
                            );
                            ui.end_row();
                        });
                    });

                    egui::Frame::group(&Style::default()).show(ui, |ui| {
                        egui::Grid::new("hex_view_colors").show(ui, |ui| {
                            ui.heading("Hex area colors");
                            ui.end_row();

                            ui.label("Selection color");
                            ui.color_edit_button_srgba_premultiplied(
                                self.settings.theme_settings.selection_color.as_bytes_mut(),
                            );
                            ui.end_row();

                            ui.label("Diff color");
                            ui.color_edit_button_srgba_premultiplied(
                                self.settings.theme_settings.diff_color.as_bytes_mut(),
                            );
                            ui.end_row();

                            ui.label("Null color");
                            ui.color_edit_button_srgba_premultiplied(
                                self.settings.theme_settings.hex_null_color.as_bytes_mut(),
                            );
                            ui.end_row();

                            ui.label("Other color");
                            ui.color_edit_button_srgba_premultiplied(
                                self.settings.theme_settings.other_hex_color.as_bytes_mut(),
                            );
                            ui.end_row();
                        });
                    });

                    egui::Frame::group(&Style::default()).show(ui, |ui| {
                        egui::Grid::new("ascii_view_colors").show(ui, |ui| {
                            ui.heading("Ascii area colors");
                            ui.end_row();

                            ui.label("Null color");
                            ui.color_edit_button_srgba_premultiplied(
                                self.settings.theme_settings.ascii_null_color.as_bytes_mut(),
                            );
                            ui.end_row();

                            ui.label("Ascii color");
                            ui.color_edit_button_srgba_premultiplied(
                                self.settings.theme_settings.ascii_color.as_bytes_mut(),
                            );
                            ui.end_row();

                            ui.label("Other color");
                            ui.color_edit_button_srgba_premultiplied(
                                self.settings
                                    .theme_settings
                                    .other_ascii_color
                                    .as_bytes_mut(),
                            );
                            ui.end_row();
                        });
                    });

                    ui.horizontal(|ui| {
                        if ui.button("Reload").clicked() {
                            self.settings = read_json_settings().expect("Failed to read settings!");
                        }
                        if ui.button("Save").clicked() {
                            write_json_settings(&self.settings).expect("Failed to save settings!");
                        }
                    });
                })
            });
    }
}

fn set_up_custom_fonts(ctx: &egui::Context) {
    // Start with the default fonts (we will be adding to them rather than replacing them).
    let mut fonts = egui::FontDefinitions::default();

    let monospace_key = "jetbrains-mono";
    let string_key = "noto-sans-jp";

    fonts.font_data.insert(
        monospace_key.to_owned(),
        egui::FontData::from_static(include_bytes!(
            "../assets/fonts/jetbrains/JetBrainsMonoNL-Regular.ttf"
        )),
    );

    fonts.font_data.insert(
        string_key.to_owned(),
        egui::FontData::from_static(include_bytes!(
            "../assets/fonts/noto/NotoSansJP-Regular.ttf"
        )),
    );

    // Put custom fonts first (highest priority):
    fonts
        .families
        .entry(egui::FontFamily::Monospace)
        .or_default()
        .insert(0, monospace_key.to_owned());

    fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .insert(0, string_key.to_owned());

    // for egui-phosphor
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);

    // Finally store all the changes we made
    ctx.set_fonts(fonts);
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CursorState {
    Hovering,
    Pressed,
    StillDown,
    Released,
}

impl eframe::App for BdiffApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut style: egui::Style = (*ctx.style()).clone();
        style.visuals.popup_shadow = Shadow {
            extrusion: 0.0,
            color: egui::Color32::TRANSPARENT,
        };
        style.visuals.window_shadow = Shadow {
            extrusion: 0.0,
            color: egui::Color32::TRANSPARENT,
        };
        style.visuals.menu_rounding = Rounding::default();
        style.visuals.window_rounding = Rounding::default();
        style.interaction.selectable_labels = false;
        style.interaction.multi_widget_text_select = false;
        ctx.set_style(style);

        let cursor_state: CursorState = ctx.input(|i| {
            if i.pointer.primary_pressed() {
                CursorState::Pressed
            } else if i.pointer.primary_down() {
                CursorState::StillDown
            } else if i.pointer.primary_released() {
                CursorState::Released
            } else {
                CursorState::Hovering
            }
        });

        let goto_modal: Modal = Modal::new(ctx, "goto_modal");

        // Goto modal
        goto_modal.show(|ui| {
            self.show_goto_modal(&goto_modal, ui, ctx);
        });

        let overwrite_modal: Modal = Modal::new(ctx, "overwrite_modal");

        if self.overwrite_modal.open {
            self.overwrite_modal(&overwrite_modal);
            overwrite_modal.open();
        }

        // Standard HexView input
        if !(overwrite_modal.is_open() || goto_modal.is_open()) {
            self.handle_hex_view_input(ctx);
        }

        if ctx.input(|i| i.key_pressed(egui::Key::G)) {
            if goto_modal.is_open() {
                goto_modal.close();
            } else {
                self.goto_modal.value = "0x".to_owned();
                goto_modal.open();
            }
        }

        // Open dropped files
        if ctx.input(|i| !i.raw.dropped_files.is_empty()) {
            for file in ctx.input(|i| i.raw.dropped_files.clone()) {
                let _ = self.open_file(&file.path.unwrap());
                self.diff_state.recalculate(&self.hex_views);
            }
        }

        // Copy selection
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::C)) {
            let mut selection = String::new();

            for hv in self.hex_views.iter() {
                if self.last_selected_hv.is_some() && hv.id == self.last_selected_hv.unwrap() {
                    let selected_bytes = hv.get_selected_bytes();

                    let selected_bytes: String = match hv.selection.side {
                        HexViewSelectionSide::Hex => selected_bytes
                            .iter()
                            .map(|b| format!("{:02X}", b))
                            .collect::<Vec<String>>()
                            .join(" "),
                        HexViewSelectionSide::Ascii => {
                            String::from_utf8_lossy(&selected_bytes).to_string()
                        }
                    };
                    // convert selected_bytes to an ascii string

                    selection.push_str(&selected_bytes.to_string());
                }
            }

            if !selection.is_empty() {
                ctx.output_mut(|o| o.copied_text = selection);
            }
        }

        // Menu bar
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Open").clicked() {
                        if let Some(path) = rfd::FileDialog::new().pick_file() {
                            let _ = self.open_file(&path);
                            self.diff_state.recalculate(&self.hex_views);
                        }

                        ui.close_menu();
                    }
                    if ui.button("Save Workspace").clicked() {
                        if self.config.changed {
                            if self.started_with_arguments {
                                self.overwrite_modal.open = true;
                            } else {
                                write_json_config("bdiff.json", &self.config)
                                    .expect("Failed to write config");
                                self.config.changed = false;
                            };
                        }
                        ui.close_menu();
                    }
                    if ui.button("Quit").clicked() {
                        ctx.send_viewport_cmd(ViewportCommand::Close)
                    }
                });
                ui.menu_button("Options", |ui| {
                    let diff_checkbox = Checkbox::new(&mut self.diff_state.enabled, "Display diff");
                    let mirror_selection_checkbox = Checkbox::new(
                        &mut self.options.mirror_selection,
                        "Mirror selection across files",
                    );

                    if ui
                        .add_enabled(self.hex_views.len() > 1, diff_checkbox)
                        .clicked()
                        && self.diff_state.enabled
                    {
                        self.diff_state.recalculate(&self.hex_views);
                    }

                    ui.add_enabled(self.hex_views.len() > 1, mirror_selection_checkbox);
                    if ui.button("Settings").clicked() {
                        self.settings_open = !self.settings_open;
                    }
                });
                ui.menu_button("Action", |ui| {
                    if ui.button("Go to address (G)").clicked() {
                        self.goto_modal.value = "0x".to_owned();
                        goto_modal.open();
                        ui.close_menu();
                    }
                });
            })
        });

        // Reload changed files
        let mut calc_diff = false;

        // Main panel
        egui::CentralPanel::default().show(ctx, |_ui| {
            // TODO unused CentralPanel
            for hv in self.hex_views.iter_mut() {
                let cur_sel = hv.selection.clone();
                let can_selection_change = match self.selecting_hv {
                    Some(id) => id == hv.id,
                    None => true,
                };
                hv.show(
                    &mut self.config,
                    &self.settings,
                    &self.diff_state,
                    ctx,
                    cursor_state,
                    can_selection_change,
                );
                if hv.selection != cur_sel {
                    match hv.selection.state {
                        HexViewSelectionState::Selecting => {
                            self.selecting_hv = Some(hv.id);
                            self.last_selected_hv = Some(hv.id);
                        }
                        _ => {
                            self.selecting_hv = None;
                        }
                    }
                    self.global_selection = hv.selection.clone();
                }

                if cursor_state == CursorState::Released {
                    // If we released the mouse button somewhere else, end the selection
                    // The state wouldn't be Selecting if we had captured the release event inside the hv
                    if hv.selection.state == HexViewSelectionState::Selecting {
                        hv.selection.state = HexViewSelectionState::Selected;
                    }
                }
            }

            if cursor_state == CursorState::Released {
                self.selecting_hv = None;
                if self.global_selection.state == HexViewSelectionState::Selecting {
                    self.global_selection.state = HexViewSelectionState::Selected;
                }
            }

            if self.options.mirror_selection {
                for hv in self.hex_views.iter_mut() {
                    if hv.selection != self.global_selection {
                        hv.selection = self.global_selection.clone();
                        if hv.selection.start() >= hv.file.data.len()
                            || hv.selection.end() >= hv.file.data.len()
                        {
                            hv.selection.clear()
                        }
                    }
                }
            }

            // Delete any closed hex views
            self.hex_views.retain(|hv| {
                calc_diff = calc_diff || hv.closed;
                let delete: bool = { hv.closed };

                if let Some(id) = self.last_selected_hv {
                    if hv.id == id {
                        self.last_selected_hv = None;
                    }
                }

                !delete
            });

            // If we have no hex views left, don't keep track of any selection
            if self.hex_views.is_empty() {
                self.global_selection.clear();
            }
        });

        // File reloading
        for hv in self.hex_views.iter_mut() {
            if hv.file.modified.swap(false, Ordering::Relaxed) {
                match hv.reload_file() {
                    Ok(_) => {
                        log::info!("Reloaded file {}", hv.file.path.display());
                        calc_diff = true;
                    }
                    Err(e) => {
                        log::error!("Failed to reload file: {}", e);
                    }
                }
            }

            if hv.mt.map_file.is_some() {
                let map_file = hv.mt.map_file.as_mut().unwrap();
                if map_file.modified.swap(false, Ordering::Relaxed) {
                    match map_file.reload() {
                        Ok(_) => {
                            log::info!("Reloaded map file {}", map_file.path.display());
                        }
                        Err(e) => {
                            log::error!("Failed to reload map file: {}", e);
                        }
                    }
                }
            }
        }

        if calc_diff {
            self.diff_state.recalculate(&self.hex_views);
        }

        if self.settings_open {
            self.show_settings(ctx);
        }
    }
}

impl BdiffApp {
    fn overwrite_modal(&mut self, modal: &Modal) {
        modal.show(|ui| {
            modal.title(ui, "Overwrite previous config");
            ui.label(&format!(
                "By saving, you are going to overwrite existing configuration file at \"{}\".",
                "./bdiff.json"
            ));
            ui.label("Are you sure you want to proceed?");

            modal.buttons(ui, |ui| {
                if ui.button("Overwrite").clicked() {
                    write_json_config("bdiff.json", &self.config).unwrap();
                    self.config.changed = false;
                    self.overwrite_modal.open = false;
                }
                if ui.button("Cancel").clicked() {
                    modal.close();
                    self.overwrite_modal.open = false;
                }
            });
        });
    }

    fn show_goto_modal(&mut self, goto_modal: &Modal, ui: &mut egui::Ui, ctx: &egui::Context) {
        goto_modal.title(ui, "Go to address");
        ui.label("Enter a hex address to go to");

        ui.text_edit_singleline(&mut self.goto_modal.value)
            .request_focus();

        ui.label(egui::RichText::new(self.goto_modal.status.clone()).color(egui::Color32::RED));

        goto_modal.buttons(ui, |ui| {
            if ui.button("Go").clicked() || ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
                let pos: Option<usize> = parse_int::parse(&self.goto_modal.value).ok();

                match pos {
                    Some(pos) => {
                        for hv in self.hex_views.iter_mut() {
                            hv.set_cur_pos(pos);
                        }
                        goto_modal.close();
                    }
                    None => {
                        self.goto_modal.status = "Invalid address".to_owned();
                        self.goto_modal.value = "0x".to_owned();
                    }
                }
            }

            if goto_modal.button(ui, "Cancel").clicked() {
                self.goto_modal.status = "".to_owned();
                goto_modal.close();
            };

            if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                goto_modal.close();
            }
        });
    }
}
