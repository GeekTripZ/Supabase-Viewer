#![windows_subsystem = "windows"]

use eframe::egui;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::mpsc;

#[derive(Clone, Serialize, Deserialize)]
struct Bookmark {
    name: String,
    url: String,
    anon_key: String,
    table: String,
}

struct App {
    url: String,
    anon_key: String,
    table: String,
    status: String,
    columns: Vec<String>,
    rows: Vec<Vec<String>>,
    loading: bool,
    rx: Option<mpsc::Receiver<Result<String, String>>>,
    show_error: bool,
    error_msg: String,
    dark_mode: bool,
    sort_col: Option<usize>,
    sort_asc: bool,
    selected_row: Option<usize>,
    col_widths: Vec<f32>,
    bookmarks: Vec<Bookmark>,
    bookmarks_path: String,
    show_save_dialog: bool,
    bookmark_name: String,
    editing_bookmark: Option<usize>,
    show_edit_dialog: bool,
    edit_name_buf: String,
    edit_url_buf: String,
    edit_key_buf: String,
    edit_table_buf: String,
}

impl Default for App {
    fn default() -> Self {
        Self {
            url: String::new(),
            anon_key: String::new(),
            table: String::new(),
            status: String::new(),
            columns: Vec::new(),
            rows: Vec::new(),
            loading: false,
            rx: None,
            show_error: false,
            error_msg: String::new(),
            dark_mode: true,
            sort_col: None,
            sort_asc: true,
            selected_row: None,
            col_widths: Vec::new(),
            bookmarks: Vec::new(),
            bookmarks_path: String::new(),
            show_save_dialog: false,
            bookmark_name: String::new(),
            editing_bookmark: None,
            show_edit_dialog: false,
            edit_name_buf: String::new(),
            edit_url_buf: String::new(),
            edit_key_buf: String::new(),
            edit_table_buf: String::new(),
        }
    }
}

impl App {
    fn bookmarks_path() -> String {
        let exe = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("."));
        let dir = exe.parent().unwrap_or_else(|| std::path::Path::new("."));
        dir.join("bookmarks.json")
            .to_string_lossy()
            .to_string()
    }

    fn load_bookmarks(path: &str) -> Vec<Bookmark> {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    fn save_bookmarks(&self) {
        if let Ok(json) = serde_json::to_string_pretty(&self.bookmarks) {
            let _ = std::fs::write(&self.bookmarks_path, json);
        }
    }

    fn fetch_table(
        url: String,
        key: String,
        table: String,
    ) -> mpsc::Receiver<Result<String, String>> {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let api_url = format!(
                "{}/rest/v1/{}?select=*",
                url.trim_end_matches('/'),
                table
            );
            let client = reqwest::blocking::Client::new();
            let resp = client
                .get(&api_url)
                .header("apikey", &key)
                .header("Authorization", format!("Bearer {}", &key))
                .header("Content-Type", "application/json")
                .header("Prefer", "return=representation")
                .send();

            match resp {
                Ok(r) => {
                    let status = r.status();
                    match r.text() {
                        Ok(body) => {
                            if status.is_success() {
                                let _ = tx.send(Ok(body));
                            } else {
                                let _ = tx.send(Err(format!("HTTP {}: {}", status, body)));
                            }
                        }
                        Err(e) => {
                            let _ = tx.send(Err(format!("Error: {}", e)));
                        }
                    }
                }
                Err(e) => {
                    let _ = tx.send(Err(format!("Connection error: {}", e)));
                }
            }
        });
        rx
    }

    fn parse_response(&mut self, json_str: &str) {
        match serde_json::from_str::<Value>(json_str) {
            Ok(Value::Array(arr)) => {
                self.columns.clear();
                self.rows.clear();
                self.sort_col = None;
                self.selected_row = None;
                self.col_widths.clear();

                if arr.is_empty() {
                    self.status = "Table is empty".to_string();
                    return;
                }
                if let Some(Value::Object(map)) = arr.first() {
                    self.columns = map.keys().cloned().collect();
                    self.columns.sort();
                }
                // Default 200px per column
                for _ in &self.columns {
                    self.col_widths.push(200.0);
                }
                for item in &arr {
                    let mut row = Vec::new();
                    for col in &self.columns {
                        let val = item
                            .get(col.as_str())
                            .map(|v| match v {
                                Value::String(s) => s.clone(),
                                Value::Null => "NULL".to_string(),
                                Value::Bool(b) => b.to_string(),
                                Value::Number(n) => n.to_string(),
                                other => other.to_string(),
                            })
                            .unwrap_or_default();
                        row.push(val);
                    }
                    self.rows.push(row);
                }
                self.status = format!("{} rows", arr.len());
            }
            Ok(_) => {
                self.status = "Response is not an array".to_string();
            }
            Err(e) => {
                self.status = format!("JSON error: {}", e);
                self.error_msg = json_str.to_string();
                self.show_error = true;
            }
        }
    }

    fn sort_rows(&mut self) {
        if let Some(col) = self.sort_col {
            let asc = self.sort_asc;
            self.rows.sort_by(|a, b| {
                let va = a.get(col).map(|s| s.as_str()).unwrap_or("");
                let vb = b.get(col).map(|s| s.as_str()).unwrap_or("");
                let na: Option<f64> = va.parse().ok();
                let nb: Option<f64> = vb.parse().ok();
                let ord = match (na, nb) {
                    (Some(a), Some(b)) => {
                        a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal)
                    }
                    _ => va.cmp(vb),
                };
                if asc { ord } else { ord.reverse() }
            });
        }
    }

    fn cell_bg(&self, idx: usize, is_sel: bool) -> egui::Color32 {
        if is_sel {
            egui::Color32::from_rgb(50, 100, 180)
        } else if idx % 2 == 1 {
            if self.dark_mode {
                egui::Color32::from_rgb(40, 40, 45)
            } else {
                egui::Color32::from_rgb(245, 245, 250)
            }
        } else if self.dark_mode {
            egui::Color32::from_rgb(30, 30, 30)
        } else {
            egui::Color32::from_rgb(255, 255, 255)
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.loading {
            ctx.request_repaint();
        }

        let mut visuals = if self.dark_mode {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };
        visuals.override_text_color = Some(if self.dark_mode {
            egui::Color32::from_rgb(220, 220, 220)
        } else {
            egui::Color32::from_rgb(30, 30, 30)
        });
        ctx.set_visuals(visuals);

        // ── Header ──
        egui::TopBottomPanel::top("header").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Supabase Viewer");
                ui.with_layout(
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        let (icon, tip) = if self.dark_mode {
                            ("\u{2600}", "Light mode")
                        } else {
                            ("\u{263E}", "Dark mode")
                        };
                        if ui
                            .button(egui::RichText::new(icon).size(18.0))
                            .on_hover_text(tip)
                            .clicked()
                        {
                            self.dark_mode = !self.dark_mode;
                        }
                    },
                );
            });
        });

        // ── Footer ──
        egui::TopBottomPanel::bottom("footer").show(ctx, |ui| {
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                let color = egui::Color32::from_rgb(140, 140, 140);
                if !self.status.is_empty() {
                    ui.label(egui::RichText::new(&self.status).color(color));
                }

                // Export CSV button
                if !self.rows.is_empty() {
                    if ui
                        .button(egui::RichText::new("Export CSV").strong())
                        .on_hover_text("Save table to CSV file")
                        .clicked()
                    {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("CSV", &["csv"])
                            .set_file_name(&format!("{}.csv", self.table))
                            .save_file()
                        {
                            let mut csv = String::new();
                            // Header
                            csv.push_str(&self.columns.join(","));
                            csv.push('\n');
                            // Rows
                            for row in &self.rows {
                                let escaped: Vec<String> = row
                                    .iter()
                                    .map(|c| {
                                        if c.contains(',') || c.contains('"') || c.contains('\n')
                                        {
                                            format!("\"{}\"", c.replace('"', "\"\""))
                                        } else {
                                            c.clone()
                                        }
                                    })
                                    .collect();
                                csv.push_str(&escaped.join(","));
                                csv.push('\n');
                            }
                            let _ = std::fs::write(&path, csv);
                            self.status = format!("Exported to {}", path.display());
                        }
                    }
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(row) = self.selected_row {
                        ui.label(
                            egui::RichText::new(format!("Row {} / {}", row + 1, self.rows.len()))
                                .color(color),
                        );
                    }
                });
            });
            ui.add_space(2.0);
        });

        // ── Inputs + Bookmarks (stable height) ──
        egui::TopBottomPanel::top("inputs").show(ctx, |ui| {
            ui.add_space(6.0);
            egui::Grid::new("inputs_grid").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                ui.label("URL:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.url)
                        .desired_width(ui.available_width())
                        .hint_text("https://xxx.supabase.co"),
                );
                ui.end_row();

                ui.label("Anon Key:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.anon_key)
                        .desired_width(ui.available_width())
                        .password(true)
                        .hint_text("eyJhbGci..."),
                );
                ui.end_row();

                ui.label("Table:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.table)
                        .desired_width(ui.available_width())
                        .hint_text("users"),
                );
                ui.end_row();
            });

            ui.add_space(4.0);

            ui.horizontal(|ui| {
                let can_connect = !self.loading
                    && !self.url.is_empty()
                    && !self.anon_key.is_empty()
                    && !self.table.is_empty();
                if ui
                    .add_enabled(
                        can_connect,
                        egui::Button::new(egui::RichText::new("Connect").strong()),
                    )
                    .clicked()
                {
                    self.loading = true;
                    self.status = "Connecting...".to_string();
                    self.sort_col = None;
                    self.selected_row = None;
                    self.rx = Some(Self::fetch_table(
                        self.url.clone(),
                        self.anon_key.clone(),
                        self.table.clone(),
                    ));
                }
                if self.loading {
                    ui.spinner();
                }

                let can_save =
                    !self.url.is_empty() && !self.anon_key.is_empty() && !self.table.is_empty();
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add_enabled(
                            can_save,
                            egui::Button::new(egui::RichText::new("+ Bookmark").strong()),
                        )
                        .on_hover_text("Save current connection as bookmark")
                        .clicked()
                    {
                        self.show_save_dialog = true;
                        self.bookmark_name = self.table.clone();
                    }
                });
            });

            ui.add_space(2.0);

            // Bookmarks row — always rendered for stable panel height
            ui.add_space(2.0);
            if self.bookmarks.is_empty() {
                ui.label(
                    egui::RichText::new("No bookmarks yet")
                        .small()
                        .color(egui::Color32::from_rgb(120, 120, 120)),
                );
            } else {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("\u{2606}")
                            .color(egui::Color32::from_rgb(180, 180, 80))
                            .size(18.0),
                    );

                    let mut delete_idx = None;
                    let mut load_idx = None;
                    let mut edit_idx = None;

                    for (i, bm) in self.bookmarks.iter().enumerate() {
                        let label = format!("{} / {}", bm.table, bm.name);

                        let resp = ui.add(
                            egui::Button::new(egui::RichText::new(&label).small())
                                .fill(if self.dark_mode {
                                    egui::Color32::from_rgb(50, 50, 60)
                                } else {
                                    egui::Color32::from_rgb(230, 230, 240)
                                })
                                .corner_radius(6.0),
                        );

                        resp.context_menu(|ui| {
                            if ui.button("\u{270E} Edit").clicked() {
                                edit_idx = Some(i);
                                ui.close_menu();
                            }
                            ui.separator();
                            if ui.button("\u{2716} Delete").clicked() {
                                delete_idx = Some(i);
                                ui.close_menu();
                            }
                        });

                        let resp = resp.on_hover_text(format!(
                            "URL: {}\nTable: {}\nClick to load",
                            bm.url, bm.table
                        ));

                        if resp.clicked() {
                            load_idx = Some(i);
                        }
                    }

                    if let Some(i) = delete_idx {
                        self.bookmarks.remove(i);
                        self.save_bookmarks();
                    }
                    if let Some(i) = load_idx {
                        self.url = self.bookmarks[i].url.clone();
                        self.anon_key = self.bookmarks[i].anon_key.clone();
                        self.table = self.bookmarks[i].table.clone();
                    }
                    if let Some(i) = edit_idx {
                        self.editing_bookmark = Some(i);
                        self.show_edit_dialog = true;
                        self.edit_name_buf = self.bookmarks[i].name.clone();
                        self.edit_url_buf = self.bookmarks[i].url.clone();
                        self.edit_key_buf = self.bookmarks[i].anon_key.clone();
                        self.edit_table_buf = self.bookmarks[i].table.clone();
                    }
                });
            }
            ui.add_space(4.0);
        });

        // ── Receive data ──
        if let Some(rx) = &self.rx {
            if let Ok(result) = rx.try_recv() {
                self.loading = false;
                self.rx = None;
                match result {
                    Ok(json) => self.parse_response(&json),
                    Err(e) => {
                        self.status = e.clone();
                        self.error_msg = e;
                        self.show_error = true;
                    }
                }
            }
        }

        // ── Central panel — table ──
        egui::CentralPanel::default().show(ctx, |ui| {
            if self.loading {
                ui.vertical_centered(|ui| {
                    ui.add_space(60.0);
                    ui.spinner();
                    ui.label("Loading...");
                });
                return;
            }
            if self.columns.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space(60.0);
                    ui.label(
                        egui::RichText::new("Enter credentials and press Connect")
                            .color(egui::Color32::from_rgb(120, 120, 120)),
                    );
                });
                return;
            }

            let num_cols = self.columns.len();
            let row_num_w = 50.0;
            let header_h = 28.0;
            let handle_w = 6.0;

            // Header
            ui.horizontal(|ui| {
                ui.allocate_ui(egui::vec2(row_num_w, header_h), |ui| {
                    ui.label(
                        egui::RichText::new("#")
                            .strong()
                            .color(egui::Color32::from_rgb(140, 140, 140)),
                    );
                });
                for ci in 0..num_cols {
                    let col = self.columns[ci].clone();
                    let w = self.col_widths[ci];
                    let is_sorted = self.sort_col == Some(ci);
                    let asc = self.sort_asc;
                    let dark = self.dark_mode;

                    let arrow = if is_sorted {
                        if asc { " \u{25B2}" } else { " \u{25BC}" }
                    } else {
                        ""
                    };
                    let text = format!("{}{}", col, arrow);
                    let text_color = if is_sorted {
                        egui::Color32::from_rgb(100, 180, 255)
                    } else if dark {
                        egui::Color32::from_rgb(220, 220, 220)
                    } else {
                        egui::Color32::from_rgb(30, 30, 30)
                    };

                    let btn = ui.push_id(ci, |ui| {
                        ui.add(
                            egui::Button::new(
                                egui::RichText::new(&text)
                                    .color(text_color)
                                    .strong(),
                            )
                            .min_size(egui::vec2(w, header_h))
                            .frame(false),
                        )
                    }).inner;

                    // Double click = auto-fit to content
                    if btn.double_clicked() {
                        let max_w = self.rows.iter().map(|r| {
                            let t = &r[ci];
                            let chars: f32 = t.chars().count() as f32;
                            chars * 10.0 + 16.0
                        }).fold(60.0f32, f32::max);
                        self.col_widths[ci] = max_w.min(600.0);
                    } else if btn.clicked() {
                        // Single click = sort
                        let asc = if is_sorted { !asc } else { true };
                        self.sort_col = Some(ci);
                        self.sort_asc = asc;
                        self.sort_rows();
                    }

                    // Resize handle (overlay)
                    let col_rect = btn.rect;
                    let handle_rect = egui::Rect::from_min_size(
                        egui::pos2(col_rect.max.x - handle_w, col_rect.min.y),
                        egui::vec2(handle_w + 2.0, header_h),
                    );
                    let handle_resp = ui.interact(
                        handle_rect,
                        egui::Id::new(("col_resize", ci)),
                        egui::Sense::click_and_drag(),
                    );
                    if handle_resp.hovered() || handle_resp.dragged() {
                        ui.painter().rect_filled(
                            handle_rect,
                            0.0,
                            egui::Color32::from_rgb(100, 100, 100),
                        );
                    }
                    ui.painter().line_segment(
                        [
                            egui::pos2(handle_rect.center().x, handle_rect.min.y + 4.0),
                            egui::pos2(handle_rect.center().x, handle_rect.max.y - 4.0),
                        ],
                        egui::Stroke::new(1.0, egui::Color32::from_rgb(70, 70, 70)),
                    );
                    if handle_resp.dragged() {
                        let delta = handle_resp.drag_delta().x;
                        self.col_widths[ci] = (w + delta).max(40.0);
                    }
                }
            });

            ui.separator();

            // Scrollable body
            let available = ui.available_size();
            let row_h = 28.0;
            let total_rows = self.rows.len();
            let body_h = total_rows as f32 * row_h;

            egui::ScrollArea::both()
                .max_height(available.y)
                .show(ui, |ui| {
                    ui.allocate_ui(egui::vec2(available.x, body_h), |ui| {
                        for idx in 0..total_rows {
                            let is_sel = Some(idx) == self.selected_row;
                            let bg = self.cell_bg(idx, is_sel);

                            let _response = ui.horizontal(|ui| {
                                let (rect, _) = ui.allocate_exact_size(
                                    egui::vec2(row_num_w, row_h),
                                    egui::Sense::hover(),
                                );
                                ui.painter().rect_filled(rect, 0.0, bg);
                                ui.painter().text(
                                    rect.center(),
                                    egui::Align2::CENTER_CENTER,
                                    format!("{}", idx + 1),
                                    egui::FontId::proportional(16.0),
                                    egui::Color32::from_rgb(120, 120, 120),
                                );

                                for ci in 0..num_cols {
                                    let w = self.col_widths[ci];
                                    let cell_text = &self.rows[idx][ci];
                                    let truncated = if cell_text.len() > 100 {
                                        format!("{}...", &cell_text[..100])
                                    } else {
                                        cell_text.clone()
                                    };

                                    let (rect, resp) = ui.allocate_exact_size(
                                        egui::vec2(w, row_h),
                                        egui::Sense::click(),
                                    );
                                    ui.painter().rect_filled(rect, 0.0, bg);
                                    ui.painter().text(
                                        rect.left_center() + egui::vec2(4.0, 0.0),
                                        egui::Align2::LEFT_CENTER,
                                        &truncated,
                                        egui::FontId::proportional(16.0),
                                        if is_sel {
                                            egui::Color32::WHITE
                                        } else if self.dark_mode {
                                            egui::Color32::from_rgb(220, 220, 220)
                                        } else {
                                            egui::Color32::from_rgb(30, 30, 30)
                                        },
                                    );

                                    let resp =
                                        resp.on_hover_text(cell_text.as_str());
                                    if resp.clicked() {
                                        self.selected_row = Some(idx);
                                        ui.ctx().copy_text(cell_text.clone());
                                    }
                                }
                            });
                        }
                    });
                });
        });

        // ── Save bookmark dialog ──
        if self.show_save_dialog {
            egui::Window::new("Save Bookmark")
                .collapsible(false)
                .resizable(false)
                .default_width(380.0)
                .show(ctx, |ui| {
                    ui.label("Name:");
                    ui.text_edit_singleline(&mut self.bookmark_name);
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new(format!(
                            "{} | {} | {}",
                            self.url, self.anon_key, self.table
                        ))
                        .small()
                        .color(egui::Color32::from_rgb(140, 140, 140)),
                    );
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("Cancel").clicked() {
                            self.show_save_dialog = false;
                        }
                        if ui.button("Save").clicked() {
                            let bm = Bookmark {
                                name: self.bookmark_name.clone(),
                                url: self.url.clone(),
                                anon_key: self.anon_key.clone(),
                                table: self.table.clone(),
                            };
                            self.bookmarks.push(bm);
                            self.save_bookmarks();
                            self.show_save_dialog = false;
                            self.status =
                                format!("Bookmark '{}' saved", self.bookmark_name);
                        }
                    });
                });
        }

        // ── Edit bookmark dialog ──
        if self.show_edit_dialog {
            egui::Window::new("Edit Bookmark")
                .collapsible(false)
                .resizable(false)
                .default_width(420.0)
                .show(ctx, |ui| {
                    ui.label("Name:");
                    ui.text_edit_singleline(&mut self.edit_name_buf);
                    ui.add_space(4.0);
                    ui.label("URL:");
                    ui.text_edit_singleline(&mut self.edit_url_buf);
                    ui.add_space(4.0);
                    ui.label("Anon Key:");
                    ui.text_edit_singleline(&mut self.edit_key_buf);
                    ui.add_space(4.0);
                    ui.label("Table:");
                    ui.text_edit_singleline(&mut self.edit_table_buf);
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("Cancel").clicked() {
                            self.show_edit_dialog = false;
                            self.editing_bookmark = None;
                        }
                        if ui.button("Save").clicked() {
                            if let Some(i) = self.editing_bookmark {
                                self.bookmarks[i].name = self.edit_name_buf.clone();
                                self.bookmarks[i].url = self.edit_url_buf.clone();
                                self.bookmarks[i].anon_key = self.edit_key_buf.clone();
                                self.bookmarks[i].table = self.edit_table_buf.clone();
                                self.save_bookmarks();
                                self.status =
                                    format!("Bookmark '{}' updated", self.edit_name_buf);
                            }
                            self.show_edit_dialog = false;
                            self.editing_bookmark = None;
                        }
                    });
                });
        }

        // ── Error window ──
        if self.show_error {
            egui::Window::new("Error")
                .collapsible(false)
                .resizable(true)
                .default_width(520.0)
                .default_height(200.0)
                .show(ctx, |ui| {
                    egui::ScrollArea::both().show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut self.error_msg.as_str())
                                .desired_width(f32::INFINITY)
                                .font(egui::TextStyle::Monospace),
                        );
                    });
                    ui.add_space(8.0);
                    if ui.button("Close").clicked() {
                        self.show_error = false;
                    }
                });
        }
    }
}

fn main() -> eframe::Result<()> {
    let path = App::bookmarks_path();
    let bookmarks = App::load_bookmarks(&path);

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 700.0])
            .with_min_inner_size([700.0, 500.0])
            .with_title("Supabase Viewer"),
        ..Default::default()
    };

    eframe::run_native(
        "Supabase Viewer",
        options,
        Box::new(|cc| {
            let mut style = (*cc.egui_ctx.style()).clone();
            style.spacing.item_spacing = egui::vec2(8.0, 4.0);
            style.text_styles
                .insert(egui::TextStyle::Body, egui::FontId::proportional(20.0));
            style.text_styles
                .insert(egui::TextStyle::Button, egui::FontId::proportional(20.0));
            style.text_styles
                .insert(egui::TextStyle::Heading, egui::FontId::proportional(24.0));
            style.text_styles
                .insert(egui::TextStyle::Small, egui::FontId::proportional(16.0));
            style.text_styles
                .insert(egui::TextStyle::Monospace, egui::FontId::monospace(16.0));
            cc.egui_ctx.set_style(style);

            let mut app = App::default();
            app.bookmarks_path = path;
            app.bookmarks = bookmarks;
            Ok(Box::new(app))
        }),
    )
}
