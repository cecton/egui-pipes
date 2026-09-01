// The #[run_example] macro generates:
//   - wasm32: a #[wasm_bindgen(start)] that calls this function body
//   - native: a main with `dist` / `start` sub-commands that build the wasm
//             bundle and serve it via a local dev server
#[xtask_wasm::run_example(assets_dir = "assets")]
fn run() {
    use eframe::egui;
    use egui_pipes::{content_size, fit_cell_size, GameStatus, PipesGame, PipesWidget};
    use serde::{Deserialize, Serialize};
    use xtask_wasm::wasm_bindgen::JsCast as _;

    const SELECTED_PRESET_KEY: &str = "selected_preset";

    #[derive(Clone, Copy, Deserialize, PartialEq, Serialize)]
    enum Preset {
        Beginner,
        Intermediate,
        Expert,
    }

    impl Preset {
        const ALL: &'static [Preset] = &[Self::Beginner, Self::Intermediate, Self::Expert];

        fn label(self) -> &'static str {
            match self {
                Self::Beginner => "Beginner (6x5)",
                Self::Intermediate => "Intermediate (7x6)",
                Self::Expert => "Expert (9x7)",
            }
        }

        /// `(columns, rows, locked ratio)`. Difficulty comes from all three:
        /// more columns means a longer chain to line up, more rows means more
        /// scroll positions to consider per column, and *fewer* locked columns
        /// means more of them move, which is what really multiplies the search.
        /// 0.4 leaves 4, 4 and 5 columns scrollable respectively.
        fn dims(self) -> (usize, usize, f32) {
            match self {
                Self::Beginner => (6, 5, 0.4),
                Self::Intermediate => (7, 6, 0.4),
                Self::Expert => (9, 7, 0.4),
            }
        }
    }

    struct PipesApp {
        game: PipesGame,
        selected_preset: Preset,
        seed_counter: u64,
        scene_rect: Option<egui::Rect>,
        mobile_cell_size: Option<f32>,
        show_menu: bool,
        touch_device: bool,
    }

    impl eframe::App for PipesApp {
        fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
            let bg = ui.max_rect();
            ui.painter()
                .rect_filled(bg, egui::CornerRadius::ZERO, ui.visuals().panel_fill);

            let is_mobile = self.is_mobile(ui);
            self.show_top_bar(ui, is_mobile);

            if is_mobile {
                self.mobile_ui(ui);
                self.show_menu_modal(ui.ctx());
            } else {
                self.desktop_ui(ui);
                self.show_menu = false;
            }
        }

        fn save(&mut self, storage: &mut dyn eframe::Storage) {
            eframe::set_value(storage, SELECTED_PRESET_KEY, &self.selected_preset);
        }
    }

    impl PipesApp {
        const MOBILE_MIN_CELL_SIZE: f32 = 34.0;

        fn new(cc: &eframe::CreationContext<'_>) -> Self {
            let selected_preset = cc
                .storage
                .and_then(|storage| eframe::get_value(storage, SELECTED_PRESET_KEY))
                .unwrap_or(Preset::Beginner);
            let (columns, rows, locked) = selected_preset.dims();
            let initial_seed = fastrand::u64(..);

            let touch_device = web_sys::window()
                .and_then(|w| w.match_media("(pointer: coarse)").ok())
                .flatten()
                .is_some_and(|mql| mql.matches());

            Self {
                game: PipesGame::random(columns, rows, locked, initial_seed),
                selected_preset,
                seed_counter: initial_seed,
                scene_rect: None,
                mobile_cell_size: None,
                show_menu: false,
                touch_device,
            }
        }

        fn new_game(&mut self, preset: Preset) {
            self.selected_preset = preset;
            let (columns, rows, locked) = preset.dims();
            self.seed_counter += 1;
            self.game = PipesGame::random(columns, rows, locked, self.seed_counter);
            self.scene_rect = None;
        }

        fn start_new_game(&mut self) {
            self.new_game(self.selected_preset);
        }

        /// Narrow viewport or a coarse (touch) pointer: switches to the
        /// panning, menu-driven mobile layout. Pointer coarseness is queried
        /// once at startup and cached, since it can't realistically change
        /// mid-session.
        fn is_mobile(&self, ui: &egui::Ui) -> bool {
            ui.ctx().content_rect().width() < 900.0 || self.touch_device
        }

        fn show_top_bar(&mut self, ui: &mut egui::Ui, is_mobile: bool) {
            if is_mobile {
                return;
            }

            egui::Panel::top("top_bar")
                .frame(egui::Frame::new().inner_margin(4.0))
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.visuals_mut().button_frame = false;
                        ui.add_space(8.0);
                        egui::widgets::global_theme_preference_switch(ui);
                        ui.separator();
                        for &preset in Preset::ALL {
                            if ui
                                .selectable_label(self.selected_preset == preset, preset.label())
                                .clicked()
                            {
                                self.new_game(preset);
                            }
                        }
                        ui.separator();
                        ui.label(format!("Moves: {}", self.game.moves()));
                        if self.game.status() == GameStatus::Won {
                            ui.colored_label(egui::Color32::GREEN, "Connected!");
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("\u{1F504} New Game").clicked() {
                                self.start_new_game();
                            }
                        });
                    });
                });
        }

        fn desktop_ui(&mut self, ui: &mut egui::Ui) {
            ui.vertical_centered(|ui| {
                ui.add_space(8.0);
                ui.label(
                    "Click a column to scroll it down. The mouse wheel scrolls it either way. \
                     Locked columns are greyed out.",
                );
                ui.add_space(8.0);
                ui.add(PipesWidget::new(&mut self.game));
            });
        }

        /// Fit the board to the viewport, floored at a tap-friendly size.
        /// Deliberately uncapped on the high end: `Scene` rescales whatever
        /// footprint it is given to fill its rect, so handing it a smaller
        /// cell size doesn't make a smaller board, it makes the same board
        /// stretched back up and blurred.
        fn mobile_cell_size(&mut self, available: egui::Vec2) -> f32 {
            let fitted = fit_cell_size(&self.game, available).max(Self::MOBILE_MIN_CELL_SIZE);
            if self.mobile_cell_size != Some(fitted) {
                self.mobile_cell_size = Some(fitted);
                self.scene_rect = None;
            }
            fitted
        }

        fn mobile_ui(&mut self, ui: &mut egui::Ui) {
            ui.spacing_mut().interact_size.y = 48.0;
            self.show_action_bar(ui);

            let cell_size = self.mobile_cell_size(ui.available_size());
            let footprint = content_size(&self.game, cell_size);
            let mut scene_rect = self
                .scene_rect
                .unwrap_or_else(|| egui::Rect::from_min_size(egui::Pos2::ZERO, footprint));

            egui::Scene::new()
                .zoom_range(egui::Rangef::new(0.25, 4.0))
                .max_inner_size(footprint)
                .show(ui, &mut scene_rect, |ui| {
                    ui.add(PipesWidget::new(&mut self.game).cell_size(cell_size));
                });
            self.scene_rect = Some(scene_rect);
        }

        fn show_action_bar(&mut self, ui: &mut egui::Ui) {
            egui::Panel::bottom("action_bar")
                .frame(egui::Frame::new().inner_margin(6.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if ui
                            .add_sized([56.0, 48.0], egui::Button::new("\u{2630}"))
                            .clicked()
                        {
                            self.show_menu = true;
                        }
                        ui.separator();
                        ui.label(format!("Moves: {}", self.game.moves()));
                        if self.game.status() == GameStatus::Won {
                            ui.colored_label(egui::Color32::GREEN, "Connected!");
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .add_sized([56.0, 48.0], egui::Button::new("\u{1F504}"))
                                .clicked()
                            {
                                self.start_new_game();
                            }
                        });
                    });
                });
        }

        fn show_menu_modal(&mut self, ctx: &egui::Context) {
            if !self.show_menu {
                return;
            }
            let font_size = 20.0;
            let response = egui::Modal::new(egui::Id::new("menu")).show(ctx, |ui| {
                ui.set_width(280.0);
                ui.visuals_mut().button_frame = false;
                if ui
                    .button(egui::RichText::new("\u{1F504} New Game").size(font_size))
                    .clicked()
                {
                    self.start_new_game();
                    self.show_menu = false;
                }
                ui.separator();
                ui.label(egui::RichText::new("Difficulty").size(font_size));
                for &preset in Preset::ALL {
                    if ui
                        .selectable_label(
                            self.selected_preset == preset,
                            egui::RichText::new(preset.label()).size(font_size),
                        )
                        .clicked()
                    {
                        self.new_game(preset);
                        self.show_menu = false;
                    }
                }
                ui.separator();
                ui.label(egui::RichText::new("Theme").size(font_size));
                let mut preference = ui.options(|o| o.theme_preference);
                ui.selectable_value(
                    &mut preference,
                    egui::ThemePreference::System,
                    egui::RichText::new("\u{1F4BB} System").size(font_size),
                );
                ui.selectable_value(
                    &mut preference,
                    egui::ThemePreference::Light,
                    egui::RichText::new("\u{2600} Light").size(font_size),
                );
                ui.selectable_value(
                    &mut preference,
                    egui::ThemePreference::Dark,
                    egui::RichText::new("\u{1F319} Dark").size(font_size),
                );
                ui.ctx().set_theme(preference);
            });

            if response.should_close() {
                self.show_menu = false;
            }
        }
    }

    // Create a full-screen canvas and attach it to the page body.
    let document = web_sys::window()
        .expect("no window")
        .document()
        .expect("no document");

    let canvas = document
        .create_element("canvas")
        .expect("failed to create canvas")
        .dyn_into::<web_sys::HtmlCanvasElement>()
        .expect("not a HtmlCanvasElement");

    let style = canvas.style();
    style.set_property("position", "fixed").unwrap();
    style.set_property("top", "0").unwrap();
    style.set_property("left", "0").unwrap();
    style.set_property("width", "100%").unwrap();
    style.set_property("height", "100%").unwrap();

    let body = document.body().expect("no body");
    body.style().set_property("margin", "0").unwrap();
    body.append_child(&canvas).expect("failed to append canvas");
    canvas.style().set_property("touch-action", "none").unwrap();

    // Start the eframe web runner on that canvas element.
    wasm_bindgen_futures::spawn_local(async move {
        eframe::WebRunner::new()
            .start(
                canvas,
                eframe::WebOptions::default(),
                Box::new(|cc| Ok(Box::new(PipesApp::new(cc)))),
            )
            .await
            .expect("failed to start eframe");
    });
}
