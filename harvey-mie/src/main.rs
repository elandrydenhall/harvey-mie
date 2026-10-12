#![cfg_attr(not(test), windows_subsystem = "windows")]

mod edit;

use std::path::Path;

use eframe::egui;

const WINDOW_POS: [f32; 2] = [2000.0, 80.0];

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Harvey MIE")
            .with_inner_size([1280.0, 860.0])
            .with_position(WINDOW_POS),
        persist_window: false,
        ..Default::default()
    };
    eframe::run_native(
        "Harvey MIE",
        options,
        Box::new(|cc| Ok(Box::new(PinEditor::new(cc)))),
    )
}

struct PinEditor {
    dir_input: String,
    opened_dir: String,
    files: Vec<String>,
    selected: Option<String>,
    text: String,
    original: String,
    has_pin: bool,
    editable: bool,
    status: String,
    preview: Option<egui::TextureHandle>,
    pending: Option<Pending>,
}

enum Pending {
    Select(String),
    OpenDir,
}

impl PinEditor {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        install_fonts(&cc.egui_ctx);
        for theme in [egui::Theme::Dark, egui::Theme::Light] {
            cc.egui_ctx.style_mut_of(theme, |style| {
                if let Some(font) = style.text_styles.get_mut(&egui::TextStyle::Monospace) {
                    font.size = 16.0;
                }
            });
        }
        Self {
            dir_input: String::new(),
            opened_dir: String::new(),
            files: Vec::new(),
            selected: None,
            text: String::new(),
            original: String::new(),
            has_pin: false,
            editable: true,
            status: "Choose a folder of JPEG files.".to_owned(),
            preview: None,
            pending: None,
        }
    }

    fn dirty(&self) -> bool {
        self.editable && self.text != self.original
    }

    fn path_bar(&mut self, ui: &mut egui::Ui) {
        let mut open = false;
        ui.horizontal(|ui| {
            ui.label("Folder");
            ui.add(
                egui::TextEdit::singleline(&mut self.dir_input)
                    .desired_width(f32::INFINITY)
                    .hint_text("Folder of JPEG files"),
            );
            open = ui.button("Open").clicked();
        });
        ui.label("Save writes this text into the JPEG trailer and the sibling .jpg.json file.");
        if open {
            let ctx = ui.ctx().clone();
            self.request_open(&ctx);
        }
    }

    fn file_list(&mut self, ui: &mut egui::Ui) {
        let files = self.files.clone();
        let selected = self.selected.clone();
        let dirty = self.dirty();
        let mut clicked = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            for name in files {
                let mut label = name.clone();
                if dirty && selected.as_deref() == Some(name.as_str()) {
                    label.push_str(" *");
                }
                if ui
                    .selectable_label(selected.as_deref() == Some(name.as_str()), label)
                    .clicked()
                {
                    clicked = Some(name);
                }
            }
        });
        if let Some(name) = clicked {
            let ctx = ui.ctx().clone();
            self.request_select(&ctx, name);
        }
    }

    fn editor(&mut self, ui: &mut egui::Ui) {
        let preview = self
            .preview
            .as_ref()
            .map(|texture| (texture.id(), texture.size_vec2()));
        let status = self.status.clone();
        let can_save = self.can_save();
        let editable = self.editable;
        let mut save = false;
        ui.horizontal(|ui| {
            if let Some((id, size)) = preview {
                ui.image((id, size));
            } else {
                ui.label("No preview");
            }
            ui.vertical(|ui| {
                ui.label(status);
                ui.add_enabled_ui(can_save, |ui| {
                    if ui.button("Save").clicked() {
                        save = true;
                    }
                });
            });
        });
        ui.separator();
        let response = ui.add_sized(
            ui.available_size(),
            egui::TextEdit::multiline(&mut self.text)
                .font(egui::TextStyle::Monospace)
                .code_editor()
                .desired_width(f32::INFINITY)
                .interactive(editable)
                .lock_focus(true),
        );
        if !editable {
            response.on_hover_text("This Pin is not UTF-8 text, so it cannot be edited here.");
        }
        if save {
            self.save_current();
        }
    }

    fn confirm(&mut self, ui: &mut egui::Ui) {
        if self.pending.is_none() {
            return;
        }
        let ctx = ui.ctx().clone();
        let mut close = None;
        egui::Modal::new(egui::Id::new("discard-edits")).show(&ctx, |ui| {
            ui.label("This file has unsaved edits. Discard them?");
            ui.horizontal(|ui| {
                if ui.button("Discard").clicked() {
                    close = Some(true);
                }
                if ui.button("Keep editing").clicked() {
                    close = Some(false);
                }
            });
        });
        if close == Some(false) {
            self.pending = None;
        } else if close == Some(true) {
            if let Some(pending) = self.pending.take() {
                self.apply_pending(&ctx, pending);
            }
        }
    }

    fn can_save(&self) -> bool {
        self.editable && self.dirty() && edit::json_error(&self.text).is_none() && self.selected.is_some()
    }

    fn request_open(&mut self, ctx: &egui::Context) {
        if self.dirty() {
            self.pending = Some(Pending::OpenDir);
        } else {
            self.opened_dir = self.dir_input.trim().to_owned();
            self.reload(ctx);
        }
    }

    fn request_select(&mut self, ctx: &egui::Context, name: String) {
        if self.selected.as_deref() == Some(name.as_str()) {
            return;
        }
        if self.dirty() {
            self.pending = Some(Pending::Select(name));
        } else {
            self.load_file(ctx, &name);
        }
    }

    fn apply_pending(&mut self, ctx: &egui::Context, pending: Pending) {
        match pending {
            Pending::Select(name) => self.load_file(ctx, &name),
            Pending::OpenDir => {
                self.opened_dir = self.dir_input.trim().to_owned();
                self.reload(ctx);
            }
        }
    }

    fn reload(&mut self, ctx: &egui::Context) {
        if self.opened_dir.is_empty() {
            self.files.clear();
            self.clear_selection();
            self.status = "Choose a folder of JPEG files.".to_owned();
            return;
        }
        let dir = Path::new(&self.opened_dir);
        match edit::list_jpegs(dir) {
            Ok(files) => {
                self.files = files;
                let pick = self
                    .files
                    .iter()
                    .find(|name| file_has_pin(dir, name))
                    .cloned()
                    .or_else(|| self.files.first().cloned());
                if let Some(name) = pick {
                    self.load_file(ctx, &name);
                } else {
                    self.clear_selection();
                    self.status = "That folder has no JPEG files.".to_owned();
                }
            }
            Err(err) => {
                self.files.clear();
                self.clear_selection();
                self.status = err.to_string();
            }
        }
    }

    fn clear_selection(&mut self) {
        self.selected = None;
        self.text.clear();
        self.original.clear();
        self.has_pin = false;
        self.editable = true;
        self.preview = None;
    }

    fn load_file(&mut self, ctx: &egui::Context, name: &str) {
        let path = Path::new(&self.opened_dir).join(name);
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(err) => {
                self.status = err.to_string();
                return;
            }
        };
        self.preview = decode_preview(ctx, &bytes, name);
        self.selected = Some(name.to_owned());
        match edit::load_pin(&bytes) {
            Ok(loaded) => {
                self.editable = true;
                self.has_pin = loaded.has_pin;
                self.original = loaded.text.clone();
                self.text = loaded.text;
                self.status = if loaded.has_pin {
                    format!("{name}  ·  {} bytes", self.text.len())
                } else {
                    format!("{name}  ·  no Pin trailer. Valid JSON creates one.")
                };
            }
            Err(edit::SaveError::NotUtf8) => {
                self.editable = false;
                self.has_pin = true;
                self.text.clear();
                self.original.clear();
                self.status = format!("{name}  ·  Pin is not UTF-8");
            }
            Err(err) => self.status = err.to_string(),
        }
    }

    fn save_current(&mut self) {
        let Some(name) = self.selected.clone() else {
            return;
        };
        let path = Path::new(&self.opened_dir).join(&name);
        match edit::save_file(&path, &self.text) {
            Ok(()) => {
                self.has_pin = true;
                self.original = self.text.clone();
                self.status = format!("Saved {} bytes into {name} and {name}.json", self.text.len());
            }
            Err(err) => self.status = err.to_string(),
        }
    }
}

impl eframe::App for PinEditor {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Panel::top("path").show(ui, |ui| self.path_bar(ui));
        egui::Panel::left("files")
            .resizable(true)
            .default_size(280.0)
            .show(ui, |ui| self.file_list(ui));
        egui::CentralPanel::default().show(ui, |ui| self.editor(ui));
        self.confirm(ui);
    }
}

fn file_has_pin(dir: &Path, name: &str) -> bool {
    std::fs::read(dir.join(name))
        .ok()
        .and_then(|bytes| mie::pin(&bytes).map(|_| ()))
        .is_some()
}

fn decode_preview(ctx: &egui::Context, jpeg: &[u8], name: &str) -> Option<egui::TextureHandle> {
    let host = mie::strip(jpeg);
    let image = image::load_from_memory(&host).ok()?;
    let image = image.resize(240, 360, image::imageops::FilterType::Triangle);
    let rgba = image.to_rgba8();
    let color = egui::ColorImage::from_rgba_unmultiplied(
        [rgba.width() as usize, rgba.height() as usize],
        rgba.as_raw(),
    );
    Some(ctx.load_texture(name, color, egui::TextureOptions::LINEAR))
}

fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let mut loaded = Vec::new();
    for (name, path) in [
        ("segoe", r"C:\Windows\Fonts\segoeui.ttf"),
        ("seguisym", r"C:\Windows\Fonts\seguisym.ttf"),
    ] {
        if let Ok(bytes) = std::fs::read(path) {
            fonts.font_data.insert(
                name.to_owned(),
                std::sync::Arc::new(egui::FontData::from_owned(bytes)),
            );
            loaded.push(name.to_owned());
        }
    }
    if loaded.is_empty() {
        return;
    }
    // Proportional UI text uses Segoe UI. The JSON editor stays on the monospace
    // face and falls back to Segoe UI Symbol for characters such as "→".
    if loaded.iter().any(|name| name == "segoe") {
        if let Some(list) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
            list.insert(0, "segoe".to_owned());
        }
    }
    if let Some(list) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
        for name in ["seguisym", "segoe"] {
            if loaded.iter().any(|loaded| loaded == name) {
                list.push(name.to_owned());
            }
        }
    }
    ctx.set_fonts(fonts);
}
