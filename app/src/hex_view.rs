use anyhow::Error;
use eframe::{
    egui::{self, Id, Sense, Separator},
    epaint::Color32,
};

use crate::{
    app::CursorState,
    bin_file::BinFile,
    bin_file::{read_file_bytes, Endianness},
    config::Config,
    data_viewer::DataViewer,
    diff_state::DiffState,
    map_tool::MapTool,
    settings::{Settings, ThemeSettings},
    string_viewer::StringViewer,
    widget::spacer::Spacer,
};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct HexViewSelectionRange {
    pub first: usize,
    pub second: usize,
}

#[derive(Clone, Default, Debug, PartialEq)]
pub enum HexViewSelectionState {
    #[default]
    None,
    Selecting,
    Selected,
}

#[derive(Clone, Default, Debug, PartialEq)]
pub enum HexViewSelectionSide {
    #[default]
    Hex,
    Ascii,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct HexViewSelection {
    pub range: HexViewSelectionRange,
    pub state: HexViewSelectionState,
    pub side: HexViewSelectionSide,
}

impl HexViewSelection {
    pub fn start(&self) -> usize {
        self.range.first.min(self.range.second)
    }

    pub fn end(&self) -> usize {
        self.range.second.max(self.range.first)
    }

    fn contains(&self, grid_pos: usize) -> bool {
        self.state != HexViewSelectionState::None
            && grid_pos >= self.start()
            && grid_pos <= self.end()
    }

    pub fn begin(&mut self, grid_pos: usize, side: HexViewSelectionSide) {
        self.range.first = grid_pos;
        self.range.second = grid_pos;
        self.state = HexViewSelectionState::Selecting;
        self.side = side;
    }

    pub fn update(&mut self, grid_pos: usize) {
        self.range.second = grid_pos;
    }

    pub fn finalize(&mut self, grid_pos: usize) {
        self.range.second = grid_pos;
        self.state = HexViewSelectionState::Selected;
    }

    pub fn clear(&mut self) {
        self.range.first = 0;
        self.range.second = 0;
        self.state = HexViewSelectionState::None;
        self.side = HexViewSelectionSide::default();
    }

    pub fn adjust_cur_pos(&mut self, delta: isize) {
        self.range.first = (self.range.first as isize + delta).max(0) as usize;
        self.range.second = (self.range.second as isize + delta).max(0) as usize;
    }
}

pub struct HexView {
    pub id: usize,
    pub file: BinFile,
    pub num_rows: u32,
    pub bytes_per_row: usize,
    pub cur_pos: usize,
    pub pos_locked: bool,
    pub selection: HexViewSelection,
    pub cursor_pos: Option<usize>,
    pub show_selection_info: bool,
    pub show_cursor_info: bool,
    sv: StringViewer,
    dv: DataViewer,
    pub mt: MapTool,
    pub closed: bool,
}

impl Default for HexView {
    fn default() -> Self {
        Self {
            id: 0,
            file: BinFile::default(),
            num_rows: 0,
            bytes_per_row: 0,
            cur_pos: 0,
            pos_locked: false,
            selection: HexViewSelection::default(),
            cursor_pos: None,
            show_selection_info: true,
            show_cursor_info: true,
            sv: StringViewer::default(),
            dv: DataViewer::default(),
            mt: MapTool::default(),
            closed: false,
        }
    }
}

impl HexView {
    pub fn new(file: BinFile, id: usize) -> Self {
        let min_rows = 10;
        let max_rows = 25;
        let default_bytes_per_row = 0x10;
        let num_rows = (file.data.len() / default_bytes_per_row).clamp(min_rows, max_rows) as u32;

        Self {
            id,
            file,
            num_rows,
            bytes_per_row: default_bytes_per_row,
            ..Default::default()
        }
    }

    pub fn set_cur_pos(&mut self, val: usize) {
        if self.pos_locked {
            return;
        }
        let last_line_start_address =
            (self.file.data.len() / self.bytes_per_row) * self.bytes_per_row;
        self.cur_pos = val.clamp(0, last_line_start_address);
    }

    pub fn adjust_cur_pos(&mut self, delta: isize) {
        if self.pos_locked {
            return;
        }
        let last_line_start_address =
            (self.file.data.len() / self.bytes_per_row) * self.bytes_per_row;
        self.cur_pos =
            (self.cur_pos as isize + delta).clamp(0, last_line_start_address as isize) as usize;
    }

    pub fn bytes_per_screen(&self) -> usize {
        self.bytes_per_row * self.num_rows as usize
    }

    pub fn get_cur_bytes(&self) -> Vec<u8> {
        let max_end = self.cur_pos + self.bytes_per_screen();
        let end = max_end.min(self.file.data.len());

        self.file.data[self.cur_pos..end].to_vec()
    }

    pub fn get_selected_bytes(&self) -> Vec<u8> {
        match self.selection.state {
            HexViewSelectionState::None => vec![],
            HexViewSelectionState::Selecting | HexViewSelectionState::Selected => {
                self.file.data[self.selection.start()..self.selection.end() + 1].to_vec()
            }
        }
    }

    pub fn reload_file(&mut self) -> Result<(), Error> {
        self.file.data = read_file_bytes(self.file.path.clone())?;

        if self.selection.range.first >= self.file.data.len()
            && self.selection.range.second >= self.file.data.len()
        {
            self.selection.clear();
        } else {
            self.selection.range.first = self.selection.range.first.min(self.file.data.len() - 1);
            self.selection.range.second = self.selection.range.second.min(self.file.data.len() - 1);
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn show_hex_grid(
        &mut self,
        diff_state: &DiffState,
        ctx: &egui::Context,
        ui: &mut egui::Ui,
        cursor_state: CursorState,
        can_selection_change: bool,
        font_size: f32,
        byte_grouping: usize,
        theme_settings: ThemeSettings,
    ) {
        let grid_rect = ui
            .group(|ui| {
                egui::Grid::new(format!("hex_grid{}", self.id))
                    .striped(true)
                    .spacing([0.0, 0.0])
                    .min_col_width(0.0)
                    .num_columns(40)
                    .show(ui, |ui| {
                        let screen_bytes = self.get_cur_bytes();
                        let mut current_pos = self.cur_pos;

                        let mut row_chunks = screen_bytes.chunks(self.bytes_per_row);

                        let mut r = 0;
                        while r < self.num_rows {
                            let row: &[u8] = row_chunks.next().unwrap_or_default();

                            let num_digits = match self.file.data.len() {
                                //0..=0xFFFF => 4,
                                0x10000..=0xFFFFFFFF => 8,
                                0x100000000..=0xFFFFFFFFFFFF => 12,
                                _ => 8,
                            };
                            let mut i = num_digits;
                            let mut offset_leading_zeros = true;

                            while i > 0 {
                                let digit = current_pos >> ((i - 1) * 4) & 0xF;

                                if offset_leading_zeros && digit > 0 {
                                    offset_leading_zeros = false;
                                }

                                let offset_digit = egui::Label::new(
                                    egui::RichText::new(format!("{:X}", digit))
                                        .monospace()
                                        .size(font_size)
                                        .color({
                                            if offset_leading_zeros {
                                                Color32::from(
                                                    theme_settings
                                                        .offset_leading_zero_color
                                                        .clone(),
                                                )
                                            } else {
                                                Color32::from(
                                                    theme_settings.offset_text_color.clone(),
                                                )
                                            }
                                        }),
                                );

                                if i < num_digits && (i % 4) == 0 {
                                    ui.add(Spacer::default().spacing_x(4.0));
                                }
                                ui.add(offset_digit);
                                i -= 1;
                            }

                            ui.add(Spacer::default().spacing_x(8.0));
                            ui.add(Separator::default().vertical().spacing(0.0));
                            ui.add(Spacer::default().spacing_x(8.0));

                            // hex view
                            let mut i = 0;
                            while i < self.bytes_per_row {
                                if i > 0 && (i % byte_grouping) == 0 {
                                    ui.add(Spacer::default().spacing_x(4.0));
                                }
                                let row_current_pos = current_pos + i;

                                let byte: Option<u8> = row.get(i).copied();

                                let byte_text = match byte {
                                    Some(byte) => format!("{:02X}", byte),
                                    None => "  ".to_string(),
                                };

                                let hex_label = egui::Label::new(
                                    egui::RichText::new(byte_text)
                                        .monospace()
                                        .size(font_size)
                                        .color(
                                            if diff_state.enabled
                                                && diff_state.is_diff_at(row_current_pos)
                                            {
                                                Color32::from(theme_settings.diff_color.clone())
                                            } else {
                                                match byte {
                                                    Some(0) => Color32::from(
                                                        theme_settings.hex_null_color.clone(),
                                                    ),
                                                    _ => Color32::from(
                                                        theme_settings.other_hex_color.clone(),
                                                    ),
                                                }
                                            },
                                        )
                                        .background_color({
                                            if self.selection.contains(row_current_pos) {
                                                theme_settings.selection_color.clone().into()
                                            } else {
                                                Color32::TRANSPARENT
                                            }
                                        }),
                                )
                                .sense(Sense::click_and_drag());

                                let res = ui.add(hex_label);

                                if byte.is_some() {
                                    if res.hovered() {
                                        self.cursor_pos = Some(row_current_pos);
                                    }
                                    if can_selection_change {
                                        self.handle_selection(
                                            res,
                                            cursor_state,
                                            row_current_pos,
                                            ctx,
                                            HexViewSelectionSide::Hex,
                                        );
                                    }
                                }
                                i += 1;

                                if i < self.bytes_per_row {
                                    ui.add(Spacer::default().spacing_x(4.0));
                                }
                            }

                            ui.add(Spacer::default().spacing_x(8.0));
                            ui.add(Separator::default().vertical().spacing(0.0));
                            ui.add(Spacer::default().spacing_x(8.0));

                            // ascii view
                            let mut i = 0;
                            while i < self.bytes_per_row {
                                let byte: Option<u8> = row.get(i).copied();

                                let row_current_pos = current_pos + i;

                                let ascii_char = match byte {
                                    Some(32..=126) => byte.unwrap() as char,
                                    Some(_) => '·',
                                    None => ' ',
                                };

                                let hex_label = egui::Label::new(
                                    egui::RichText::new(ascii_char)
                                        .monospace()
                                        .size(font_size)
                                        .color(match byte {
                                            Some(0) => Color32::from(
                                                theme_settings.ascii_null_color.clone(),
                                            ),
                                            Some(32..=126) => {
                                                Color32::from(theme_settings.ascii_color.clone())
                                            }
                                            _ => Color32::from(
                                                theme_settings.other_ascii_color.clone(),
                                            ),
                                        })
                                        .background_color({
                                            if self.selection.contains(row_current_pos) {
                                                theme_settings.selection_color.clone().into()
                                            } else {
                                                Color32::TRANSPARENT
                                            }
                                        }),
                                )
                                .sense(Sense::click_and_drag());

                                let res = ui.add(hex_label);
                                ui.add(Spacer::default().spacing_x(1.0));

                                if byte.is_some() {
                                    if res.hovered() {
                                        self.cursor_pos = Some(row_current_pos);
                                    }
                                    if can_selection_change {
                                        self.handle_selection(
                                            res,
                                            cursor_state,
                                            row_current_pos,
                                            ctx,
                                            HexViewSelectionSide::Ascii,
                                        );
                                    }
                                }
                                i += 1;
                            }

                            current_pos += self.bytes_per_row;
                            r += 1;
                            ui.end_row();
                        }
                    });
            })
            .response
            .rect;

        if let Some(cursor_pos) = ctx.input(|i| i.pointer.hover_pos()) {
            if !grid_rect.contains(cursor_pos) {
                self.cursor_pos = None;
            }
        }
    }

    fn handle_selection(
        &mut self,
        res: egui::Response,
        cursor_state: CursorState,
        row_current_pos: usize,
        ctx: &egui::Context,
        side: HexViewSelectionSide,
    ) {
        if res.hovered() {
            if cursor_state == CursorState::Pressed {
                self.selection.begin(row_current_pos, side);
            }

            self.cursor_pos = Some(row_current_pos);
        }

        if let Some(cursor_pos) = ctx.input(|i| i.pointer.hover_pos()) {
            if res.rect.contains(cursor_pos) {
                match cursor_state {
                    CursorState::StillDown => {
                        if self.selection.state == HexViewSelectionState::Selecting {
                            self.selection.update(row_current_pos);
                        }
                    }
                    CursorState::Released => {
                        if self.selection.state == HexViewSelectionState::Selecting {
                            self.selection.finalize(row_current_pos);
                        }
                    }
                    _ => {}
                }
            }
        }

        if res.middle_clicked() {
            self.selection.clear();
        }
    }

    pub fn show(
        &mut self,
        config: &mut Config,
        settings: &Settings,
        diff_state: &DiffState,
        ctx: &egui::Context,
        cursor_state: CursorState,
        can_selection_change: bool,
    ) {
        let font_size = 14.0;

        egui::Window::new(self.file.path.to_str().unwrap())
            .id(Id::new(format!("hex_view_window_{}", self.id)))
            .title_bar(false)
            .show(ctx, |ui| {
                let file_name = self.file.path.as_path().to_str().unwrap();

                ui.with_layout(
                    egui::Layout::left_to_right(eframe::emath::Align::Min),
                    |ui| {
                        ui.label(
                            egui::RichText::new(file_name)
                                .monospace()
                                .size(font_size)
                                .color(Color32::LIGHT_GRAY),
                        );

                        let (lock_text, hover_text) = match self.pos_locked {
                            true => (
                                egui::RichText::new(egui_phosphor::regular::LOCK_SIMPLE)
                                    .color(Color32::RED),
                                "Unlock scroll position",
                            ),
                            false => (
                                egui::RichText::new(egui_phosphor::regular::LOCK_SIMPLE_OPEN)
                                    .color(Color32::GREEN),
                                "Lock scroll position",
                            ),
                        };
                        if ui.button(lock_text).on_hover_text(hover_text).clicked() {
                            self.pos_locked = !self.pos_locked;
                        }

                        match self.file.endianness {
                            Endianness::Little => {
                                if ui
                                    .button("LE")
                                    .on_hover_text("Switch to big-endian")
                                    .clicked()
                                {
                                    self.file.endianness = Endianness::Big;
                                }
                            }
                            Endianness::Big => {
                                if ui
                                    .button("BE")
                                    .on_hover_text("Switch to little-endian")
                                    .clicked()
                                {
                                    self.file.endianness = Endianness::Little;
                                }
                            }
                        }

                        ui.menu_button("...", |ui| {
                            ui.checkbox(&mut self.show_selection_info, "Selection info");
                            ui.checkbox(&mut self.show_cursor_info, "Cursor info");
                            ui.checkbox(&mut self.dv.show, "Data viewer");
                            ui.checkbox(&mut self.sv.show, "String viewer");
                            ui.checkbox(&mut self.mt.show, "Map tool");
                        });

                        if ui.button("X").on_hover_text("Close").clicked() {
                            self.closed = true;

                            // Remove file from the config if it's closed.
                            if let Some(pos) =
                                config.files.iter().position(|a| a.path == self.file.path)
                            {
                                config.files.remove(pos);
                                config.changed = true;
                            }
                        }
                    },
                );

                ui.with_layout(
                    egui::Layout::left_to_right(eframe::emath::Align::Min),
                    |ui: &mut egui::Ui| {
                        ui.vertical(|ui| {
                            self.show_hex_grid(
                                diff_state,
                                ctx,
                                ui,
                                cursor_state,
                                can_selection_change,
                                font_size,
                                settings.byte_grouping.into(),
                                settings.theme_settings.clone(),
                            );

                            if self.show_selection_info {
                                let selection_text = match self.selection.state {
                                    HexViewSelectionState::None => "No selection".to_owned(),
                                    _ => {
                                        let start = self.selection.start();
                                        let end = self.selection.end();
                                        let length = end - start + 1;

                                        let map_entry = match self.mt.map_file {
                                            Some(ref map_file) => {
                                                map_file.get_entry(start, end + 1)
                                            }
                                            None => None,
                                        };

                                        let beginning = match length {
                                            1 => {
                                                format!("Selection: 0x{:X}", start)
                                            }
                                            _ => {
                                                format!(
                                                    "Selection: 0x{:X} - 0x{:X} (len 0x{:X})",
                                                    start, end, length
                                                )
                                            }
                                        };

                                        match map_entry {
                                            Some(entry) => {
                                                format!(
                                                    "{} ({} + 0x{})",
                                                    beginning,
                                                    entry.symbol_name,
                                                    start - entry.symbol_vrom
                                                )
                                            }
                                            None => beginning,
                                        }
                                    }
                                };
                                ui.label(egui::RichText::new(selection_text).monospace());
                            }

                            if self.show_cursor_info {
                                let hover_text = match self.cursor_pos {
                                    Some(pos) => {
                                        let map_entry = match self.mt.map_file {
                                            Some(ref map_file) => map_file.get_entry(pos, pos + 1),
                                            None => None,
                                        };

                                        match map_entry {
                                            Some(entry) => {
                                                format!(
                                                    "Cursor: 0x{:X} ({} + 0x{})",
                                                    pos,
                                                    entry.symbol_name,
                                                    pos - entry.symbol_vrom
                                                )
                                            }
                                            None => format!("Cursor: 0x{:X}", pos),
                                        }
                                    }
                                    None => "Not hovering".to_owned(),
                                };
                                ui.label(egui::RichText::new(hover_text).monospace());
                            }
                        });

                        ui.with_layout(egui::Layout::top_down(eframe::emath::Align::Min), |ui| {
                            self.dv.display(
                                ui,
                                self.id,
                                self.get_selected_bytes(),
                                self.file.endianness,
                            );
                            self.sv.display(
                                ui,
                                self.id,
                                self.get_selected_bytes(),
                                self.file.endianness,
                            );
                            self.mt.display(ui);
                        });
                    },
                );
            });
    }
}
