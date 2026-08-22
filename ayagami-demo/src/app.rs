use std::{
    collections::{HashMap, HashSet},
    f64::consts::PI,
    io::{Cursor, Read, Seek},
    sync::{Arc, Mutex},
};

use anyhow::anyhow;

use ayagami::{core::ItemArray, meta, pose::Pose};
use ayagami::{
    core::{Model, Param},
    file, physics, pose,
};
use ayagami_render::*;
use glam::f32::{Affine2, Vec2, vec2};
use log::{error, info};
use std::{env, fs::File};

use eframe::{
    egui::{self, Color32},
    egui_wgpu,
};

use git_version::git_version;

const VERSION: &str = git_version!(cargo_prefix = "v", fallback = "unknown");

const FALLBACK_FONT: &[u8] = include_bytes!("../assets/DroidSansFallback.ttf");

const PARAM_BREATH: pose::Key<'static> = pose::Key::param("ParamBreath");

#[derive(Clone)]
struct ModelView {
    top_left_px: Vec2,
    dims_px: Vec2,
    transform: Affine2,
}

#[derive(Copy, Clone, PartialEq, Eq, Default)]
enum CaptureAlphaMode {
    #[default]
    StraightClamp,
    StraightExtend,
    Premultiplied,
}

#[derive(Copy, Clone, PartialEq, Eq, Default)]
enum CaptureFrameMode {
    #[default]
    Viewport1X,
    Viewport2X,
    WholeModel,
}

struct CaptureRequest {
    view: ModelView,
    bg_color: egui::Color32,
    alpha_mode: CaptureAlphaMode,
    clipboard_image: Option<Arc<Mutex<Option<egui::ColorImage>>>>,
}

#[derive(Default)]
pub struct AppState {
    transform: Affine2,
    physics: Option<physics::PhysicsEngine>,
    rigged_parameters: HashSet<pose::Key<'static>>,
    pose: pose::Pose,
    user_pose: pose::Pose,
    physics_pose: pose::Pose,
    needs_settle: bool,
    physics_enabled: bool,
    breath_enabled: bool,
    breath_time: f64,
    bg_color: egui::Color32,
    alpha_mode: CaptureAlphaMode,
    frame_mode: CaptureFrameMode,
    request_capture: Option<bool>,
    clipboard_image: Option<Arc<Mutex<Option<egui::ColorImage>>>>,
}

type ModelRenderer = ayagami_render::ModelRenderer<file::ParsedModel, Arc<file::ParsedModel>>;

pub struct AyagamiTestApp {
    model: Option<Arc<file::ParsedModel>>,
    renderer: Arc<Mutex<ModelRenderer>>,
    state: AppState,
    info: Option<meta::DisplayInfo>,
    info_param: HashMap<String, meta::Parameter>,
    kp_param: HashMap<String, Vec<f32>>,
}

struct RenderResources {
    renderer: Arc<Mutex<ModelRenderer>>,
    format: wgpu::TextureFormat,
}

impl AyagamiTestApp {
    fn load_startup_model(&mut self) -> anyhow::Result<()> {
        let args: Vec<String> = env::args().collect();
        if let Some(filename) = args.get(1) {
            let zipf = File::open(filename)?;
            let archive = zip::ZipArchive::new(zipf)?;
            self.load_model(archive)?;
        }
        Ok(())
    }

    fn load_model<R: Read + Seek>(
        &mut self,
        mut archive: zip::ZipArchive<R>,
    ) -> anyhow::Result<()> {
        use std::path::PathBuf;

        let model3 = 'out: {
            for i in 0..archive.len() {
                let member = archive.by_index(i)?;
                if member.name().ends_with(".model3.json") {
                    break 'out member;
                }
            }
            return Err(anyhow!("model3 file not found"));
        };

        let tmp = PathBuf::from(model3.name());
        let base = tmp.parent().unwrap();
        info!(
            "Loading model3 file: {} (base: {})",
            model3.name(),
            base.to_string_lossy()
        );

        let info: meta::Model3 = serde_json::from_reader(model3)?;

        let moc_path = base.join(info.file_references.moc);
        let mut moc = archive.by_path(&moc_path)?;

        info!("Loading model {}...", moc_path.to_string_lossy());
        let model = Arc::new(file::ParsedModel::load(&mut moc)?);
        drop(moc);

        self.kp_param.clear();
        for param in model.params().into_iter() {
            if let Some(kp) = param.keypoints() {
                self.kp_param.insert(param.id().to_owned(), kp.to_vec());
            }
        }

        info!("Loading texture files...");
        let mut texdata: Vec<Vec<u8>> = Vec::new();
        for name in info.file_references.textures {
            let path = base.join(name);
            info!("Loading {}...", path.to_string_lossy());
            let mut f = archive.by_path(path)?;
            let mut data = Vec::new();
            f.read_to_end(&mut data)?;
            texdata.push(data);
        }

        let texref: Vec<&[u8]> = texdata.iter().map(|v| &v[..]).collect();
        info!("Loading model into renderer...");
        self.model = Some(model.clone());
        let mut rigged_parameters = HashSet::new();
        for param in model.params() {
            if !param.param_maps().is_empty() || !param.blend_param_maps().is_empty() {
                rigged_parameters.insert(pose::Key::from_param(param.id().to_string()));
            }
        }
        let mut pose = Pose::new(&*model);
        self.renderer.lock().unwrap().load_model(model, &texref)?;
        self.renderer.lock().unwrap().driver().apply_pose(&pose);
        self.state.physics_pose = pose.clone();
        pose.update(&self.state.user_pose);
        self.state.user_pose = pose.clone();
        self.state.pose = pose;
        self.state.rigged_parameters = rigged_parameters;

        self.info = None;
        self.info_param.clear();

        if let Some(cdi_name) = info.file_references.display_info {
            info!("Loading display info...");
            let cdi_path = base.join(cdi_name);
            let cdi = archive.by_path(&cdi_path)?;
            let info: meta::DisplayInfo = serde_json::from_reader(cdi)?;

            for param in info.parameters.iter() {
                self.info_param.insert(param.id.clone(), param.clone());
            }

            self.info = Some(info);
        }

        self.state.physics = None;
        if let Some(physics_name) = info.file_references.physics {
            info!("Loading physics...");
            let physics_path = base.join(physics_name);
            let physics = archive.by_path(&physics_path)?;
            let setting: meta::Physics3 = serde_json::from_reader(physics)?;

            let config = physics::PhysicsOptions::compatible(None);
            self.state.physics = Some(physics::PhysicsEngine::new(setting, config));
            self.state.needs_settle = true;
        }

        Ok(())
    }

    fn load_fonts(ctx: &egui::Context) {
        use egui::epaint::text;
        ctx.add_font(text::FontInsert::new(
            "droid_sans_fallback",
            egui::FontData::from_static(FALLBACK_FONT),
            vec![
                text::InsertFontFamily {
                    family: egui::FontFamily::Proportional,
                    priority: egui::epaint::text::FontPriority::Lowest,
                },
                text::InsertFontFamily {
                    family: egui::FontFamily::Monospace,
                    priority: egui::epaint::text::FontPriority::Lowest,
                },
            ],
        ));
    }

    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        Self::load_fonts(&cc.egui_ctx);

        let render_state = cc.wgpu_render_state.as_ref().expect("WGPU enabled");

        let device = render_state.device.clone();
        let queue = render_state.queue.clone();

        let renderer = ModelRenderer::new(device, queue).expect("ModelRenderer init");

        let renderer = Arc::new(Mutex::new(renderer));

        let format = render_state.target_format;

        render_state
            .renderer
            .write()
            .callback_resources
            .insert(RenderResources {
                renderer: renderer.clone(),
                format,
            });

        let mut app = Self {
            model: None,
            renderer,
            state: AppState {
                physics_enabled: true,
                breath_enabled: true,
                bg_color: egui::Color32::TRANSPARENT,
                ..Default::default()
            },
            info: Default::default(),
            info_param: Default::default(),
            kp_param: Default::default(),
        };

        if let Err(e) = app.load_startup_model() {
            error!("Failed to load startup model: {:?}", e);
        }

        app
    }

    fn model_view(&mut self, ui: &mut egui::Ui, rect: egui::Rect, frame: &mut eframe::Frame) {
        let response = ui.interact(rect, egui::Id::NULL, egui::Sense::drag());

        // Apply drag (2x factor because viewport is -1..1)
        let drag = response.drag_delta() / rect.size() * 2.0;
        let delta = Affine2::from_translation(vec2(drag.x, -drag.y));
        self.state.transform = delta * self.state.transform;

        // Apply zoom (scroll + pinch-to-zoom)
        if response.hovered() {
            response.ctx.input(|r| {
                let rel_cursor = r
                    .pointer
                    .interact_pos()
                    .map(|p| (p - rect.center()) / rect.size() * 2.)
                    .unwrap_or_default();
                let cpos = Affine2::from_translation(vec2(rel_cursor.x, -rel_cursor.y));
                let dy = r.smooth_scroll_delta().y / 200.0;
                if dy != 0. || r.zoom_delta() != 1. {
                    let cur = self.state.transform.to_scale_angle_translation().0.x;
                    let zoom = ((2f32).powf(dy) * r.zoom_delta()).clamp(0.05 / cur, 20. / cur);
                    let delta = Affine2::from_scale(Vec2::splat(zoom));
                    self.state.transform = cpos * delta * cpos.inverse() * self.state.transform;
                }
            });
        }
        self.draw_model(ui, rect, frame);
    }

    fn draw_model(&mut self, ui: &mut egui::Ui, rect: egui::Rect, frame: &mut eframe::Frame) {
        // Figure out our viewport in pixels, to get 1:1 mask rendering
        let pixels_per_point = ui.pixels_per_point();
        let left_px = (pixels_per_point * rect.min.x).round();
        let top_px = (pixels_per_point * rect.min.y).round();
        let right_px = (pixels_per_point * rect.max.x).round();
        let bottom_px = (pixels_per_point * rect.max.y).round();
        let top_left_px = vec2(left_px, top_px);
        let bot_right_px = vec2(right_px, bottom_px);
        let dims_px = bot_right_px - top_left_px;

        let scale = if dims_px.x > dims_px.y {
            vec2(dims_px.y / dims_px.x, 1.)
        } else {
            vec2(1., dims_px.x / dims_px.y)
        };

        let transform = self.state.transform * Affine2::from_scale(1.2 * scale);

        let view = ModelView {
            top_left_px,
            dims_px,
            transform,
        };

        if let Some(to_clipboard) = self.state.request_capture.take() {
            let view = match self.state.frame_mode {
                CaptureFrameMode::Viewport1X => ModelView {
                    top_left_px: Vec2::ZERO,
                    ..view
                },
                CaptureFrameMode::Viewport2X => ModelView {
                    top_left_px: Vec2::ZERO,
                    dims_px: dims_px * Vec2::splat(2.),
                    ..view
                },
                CaptureFrameMode::WholeModel => {
                    let m = self.model.as_ref().unwrap();
                    let dim = m.canvas_properties().dimensions;
                    let center = m.canvas_properties().center;
                    let scale = m.canvas_properties().scale;
                    let transform = Affine2::from_translation(-Vec2::ONE)
                        * Affine2::from_scale(Vec2::new(2., 2.) / dim)
                        * Affine2::from_translation(center)
                        * Affine2::from_scale(Vec2::splat(scale));
                    ModelView {
                        top_left_px: Vec2::ZERO,
                        dims_px: dim,
                        transform,
                    }
                }
            };

            if to_clipboard {
                self.state.clipboard_image = Some(Default::default());
            }

            let cap = CaptureRequest {
                view,
                bg_color: self.state.bg_color,
                alpha_mode: self.state.alpha_mode,
                clipboard_image: self.state.clipboard_image.clone(),
            };

            let rs = frame.wgpu_render_state().unwrap();
            cap.capture(&mut self.renderer.lock().unwrap(), &rs.device, &rs.queue);
        }

        let cb = egui_wgpu::Callback::new_paint_callback(rect, view);

        ui.painter().add(cb);
    }
    fn top_bar(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
        ui.add_space(8.0);

        egui::widgets::global_theme_preference_switch(ui);

        ui.separator();
        ui.label("Ayagami Model Poser");

        if let Some(render_state) = frame.wgpu_render_state() {
            let info = render_state.adapter.get_info();

            ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                ui.add_space(8.0);
                ui.label(format!("{:?}", info.backend));
                ui.label("Backend:");
                ui.separator();
                ui.label(VERSION);
                ui.label("Version:");
                ui.separator();
                ui.add(egui::Hyperlink::from_label_and_url(
                    egui::RichText::new("Source code"),
                    "https://github.com/AyagamiDev/ayagami",
                ));
            });
        }
    }

    fn parameter_group(
        state: &mut AppState,
        info_param: &HashMap<String, meta::Parameter>,
        kp_param: &HashMap<String, Vec<f32>>,
        renderer: &mut ModelRenderer,
        ui: &mut egui::Ui,
        id: &str,
    ) {
        for param in renderer.params() {
            let mut label = &param.id;
            if let Some(info) = info_param.get(&param.id) {
                if info.group_id != id {
                    continue;
                }
                label = &info.name;
            }
            let key = pose::Key::param(&param.id);
            let mut value = state.pose.get_flattened(&key).unwrap();
            let (physics_input, physics_output) = state
                .physics
                .as_ref()
                .map(|p| {
                    (
                        p.input_key_set().contains(&key),
                        p.output_key_set().contains(&key),
                    )
                })
                .unwrap_or((false, false));
            let (physics_type, mut physics_type_hint) = match (physics_input, physics_output) {
                (false, false) => ("", "Normal parameter (no physics)"),
                (true, false) => ("○", "Physics input"),
                (false, true) => ("⏺", "Physics output"),
                (true, true) => ("◑", "Physics input and output"),
            };

            ui.horizontal(|ui| {
                let phys = if physics_output && state.user_pose.has_value(&key) {
                    if physics_input {
                        physics_type_hint = "Physics input and output (overridden)";
                    } else {
                        physics_type_hint = "Physics output (overridden)";
                    }
                    egui::RichText::new(physics_type).color(egui::Color32::RED)
                } else if physics_type.is_empty() {
                    if !state.rigged_parameters.contains(&key) {
                        physics_type_hint = "Unused parameter";
                        egui::RichText::new("⛶")
                    } else {
                        egui::RichText::new("⏵")
                    }
                } else {
                    egui::RichText::new(physics_type)
                };
                ui.label(phys)
                    .on_hover_cursor(egui::CursorIcon::Default)
                    .on_hover_text(physics_type_hint);
                if ui
                    .add_enabled(state.user_pose.has_value(&key), egui::Button::new("🔄"))
                    .on_hover_text("Reset to default")
                    .clicked()
                {
                    state.user_pose.unset(&key);
                    if !physics_output {
                        state.physics_pose.unset(&key);
                    }
                }
                // Slider::max_decimals() force rounds the value even if the user doesn't touch
                // it. We don't want that for physics outputs/breath, so explicitly round
                // but only commit if the slider was touched.
                value = (value * 100.).round() / 100.;
                let res = ui
                    .add(egui::Slider::new(&mut value, param.min..=param.max).text(label))
                    .on_hover_text_at_pointer(&param.id);
                if res.changed() {
                    if key == PARAM_BREATH {
                        state.breath_enabled = false;
                    }
                    if res.ctx.input(|input| input.modifiers.shift)
                        && let Some(closest) = kp_param.get(&param.id).and_then(|v| {
                            v.iter().min_by(|a, b| {
                                (*a - value).abs().partial_cmp(&(*b - value).abs()).unwrap()
                            })
                        })
                    {
                        value = *closest;
                    }
                    state.user_pose.set(&key, value);
                }
            });
        }
    }

    fn left_panel(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let mut renderer = self.renderer.lock().unwrap();

        ui.heading("Model Info");

        let m = self.model.as_ref().unwrap();

        ui.horizontal(|ui| {
            match m.version() {
                Some(ver) => ui.label(format!("Version: {}", ver)),
                None => ui.label("Version: <unknown>"),
            };

            let dim = m.canvas_properties().dimensions;
            let center = m.canvas_properties().center;
            let scale = m.canvas_properties().scale;
            ui.label(format!(
                "Canvas: {}x{} ({}, {}) ×{}",
                dim.x, dim.y, center.x, center.y, scale
            ));
        });
        ui.horizontal(|ui| {
            ui.label(format!("ArtMeshes: {}", m.artmeshes().count()));
            ui.label(format!("Deformers: {}", m.deformers().count()));
            ui.label(format!("Parameters: {}", m.params().count()));
        });
        ui.horizontal(|ui| {
            ui.label(format!("Glues: {}", m.glues().count()));
            ui.label(format!("Draw groups: {}", m.draw_groups().count()));
            ui.label(format!(
                "Vtx: {}",
                m.texcoord_buffer().map(|i| i.len()).unwrap_or(0)
            ));
            ui.label(format!(
                "Tri: {}",
                m.index_buffer().map(|i| i.len() / 3).unwrap_or(0)
            ));
        });

        ui.heading("Parameters");

        if let Some(info) = self.info.as_ref() {
            for group in info.parameter_groups.iter() {
                ui.collapsing(group.name.clone(), |ui| {
                    Self::parameter_group(
                        &mut self.state,
                        &self.info_param,
                        &self.kp_param,
                        &mut renderer,
                        ui,
                        &group.id,
                    );
                });
            }
        }

        Self::parameter_group(
            &mut self.state,
            &self.info_param,
            &self.kp_param,
            &mut renderer,
            ui,
            "",
        );

        self.state.pose = self.state.physics_pose.clone();
        self.state.pose.update(&self.state.user_pose);
        renderer.driver().set_pose(&self.state.pose);
    }

    fn right_panel(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        ui.heading("Screenshot");
        ui.horizontal(|ui| {
            ui.label("Background color");
            ui.color_edit_button_srgba(&mut self.state.bg_color);
        });
        let a = &mut self.state.alpha_mode;
        ui.label("Emissive pixel mode:");
        egui::ComboBox::from_id_salt("ALPHA_MODE")
            .selected_text(match *a {
                CaptureAlphaMode::StraightClamp => "Transparent",
                CaptureAlphaMode::StraightExtend => "Opaque",
                CaptureAlphaMode::Premultiplied => "Premultiplied",
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(a, CaptureAlphaMode::StraightClamp, "Transparent");
                ui.selectable_value(a, CaptureAlphaMode::StraightExtend, "Opaque");
                ui.selectable_value(a, CaptureAlphaMode::Premultiplied, "Premultiplied");
            });
        ui.horizontal(|ui| {
            let m = &mut self.state.frame_mode;
            ui.label("Resolution:");
            ui.radio_value(m, CaptureFrameMode::Viewport1X, "1×")
                .on_hover_text("Visible area (display resolution)");
            ui.radio_value(m, CaptureFrameMode::Viewport2X, "2×")
                .on_hover_text("Visible area (double resolution)");
            ui.radio_value(m, CaptureFrameMode::WholeModel, "Full")
                .on_hover_text("Entire model (original resolution");
        });

        ui.horizontal(|ui| {
            if ui.button("📋 Copy").clicked() {
                self.state.request_capture = Some(true);
            }
            if ui.button("💾 Save").clicked() {
                self.state.request_capture = Some(false);
            }
        });

        if let Some(physics) = self.state.physics.as_ref() {
            ui.heading("Physics");
            ui.horizontal(|ui| {
                ui.toggle_value(&mut self.state.physics_enabled, "Enabled");
                if self.state.physics_enabled {
                    if ui.button("⏹").on_hover_text("Settle physics").clicked() {
                        self.state.needs_settle = true;
                    }
                } else {
                    if ui
                        .button("🔄")
                        .on_hover_text("Reset physics outputs")
                        .clicked()
                    {
                        self.state.needs_settle = true;
                        for k in physics.output_key_set() {
                            self.state.physics_pose.unset(k);
                        }
                    }
                }
            });
        }

        if self.state.pose.has_key(&PARAM_BREATH) {
            ui.heading("Breath");
            ui.horizontal(|ui| {
                if ui
                    .toggle_value(&mut self.state.breath_enabled, "Enabled")
                    .changed()
                {
                    // When explicitly enabled, remove user override on breath
                    if self.state.breath_enabled {
                        self.state.user_pose.unset(&PARAM_BREATH);
                    }
                }
                if ui
                    .add_enabled(!self.state.breath_enabled, egui::Button::new("🔄"))
                    .on_hover_text("Reset breath output")
                    .clicked()
                {
                    // When explicitly reset, remove user override on breath
                    self.state.user_pose.unset(&PARAM_BREATH);
                    self.state.physics_pose.unset(&PARAM_BREATH);
                    self.state.breath_time = 0.;
                }
            });
        }
    }
}

impl eframe::App for AyagamiTestApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let (_, stable_dt) = ctx.input(|i| (i.time, i.stable_dt));

        if self.state.breath_enabled {
            let time = self.state.breath_time;
            self.state.breath_time += stable_dt as f64;
            if let Some((_, desc)) = self.state.pose.map().get(&PARAM_BREATH) {
                let v =
                    ((time / 2. * PI).cos() / -2. + 0.5) as f32 * (desc.max - desc.min) + desc.min;
                self.state.physics_pose.set(&PARAM_BREATH, v);
                ctx.request_repaint();
            }
        }

        if self.state.physics_enabled
            && let Some(physics) = &mut self.state.physics
        {
            self.state.physics_pose.update(&self.state.user_pose);
            if self.state.needs_settle {
                physics.settle(&self.state.physics_pose);
                self.state.needs_settle = false;
            }
            physics.update(&mut self.state.physics_pose, stable_dt);
            ctx.request_repaint();
        }

        if let Some(slot) = self.state.clipboard_image.as_ref() {
            let inner = slot.lock().unwrap().take();
            if let Some(img) = inner {
                ctx.copy_image(img);
                self.state.clipboard_image = None;
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        egui::Panel::top("top bar")
            .frame(egui::Frame::side_top_panel(ui.style()).inner_margin(4))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.visuals_mut().button_frame = false;
                    self.top_bar(ui, frame);
                });
            });

        egui::Panel::bottom("log").resizable(true).show(ui, |ui| {
            egui_logger::logger_ui().max_log_length(10000).show(ui);
            ui.take_available_space();
        });

        if self.renderer.lock().unwrap().is_loaded() {
            egui::Panel::left("left panel")
                .resizable(false)
                .frame(egui::Frame::side_top_panel(ui.style()).inner_margin(6))
                .show(ui, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        self.left_panel(ui, frame);
                    });
                });

            egui::Panel::right("right panel")
                .resizable(false)
                .frame(egui::Frame::side_top_panel(ui.style()).inner_margin(6))
                .show(ui, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        self.right_panel(ui, frame);
                    });
                });
        }

        ui.input_mut(|inp| {
            if let Some(f) = inp.raw.dropped_files.pop() {
                if let Some(p) = f.path {
                    info!("File dropped (path: {})", p.to_string_lossy());
                    let Ok(zipf) = File::open(&p) else {
                        error!("Failed to open file {}", p.to_string_lossy());
                        return;
                    };
                    let Ok(archive) = zip::ZipArchive::new(zipf) else {
                        error!("Failed to parse zip file");
                        return;
                    };
                    if let Err(e) = self.load_model(archive) {
                        error!("Failed to load model: {:?}", e);
                    }
                } else if let Some(b) = f.bytes {
                    info!("File dropped ({} bytes)", b.len());
                    let c = Cursor::new(b);
                    let Ok(archive) = zip::ZipArchive::new(c) else {
                        error!("Failed to parse zip file");
                        return;
                    };
                    if let Err(e) = self.load_model(archive) {
                        error!("Failed to load model: {:?}", e);
                    }
                }
                inp.raw.dropped_files.clear();
            }
        });

        let mut center_frame = egui::Frame::canvas(ui.style());
        center_frame = center_frame.fill(center_frame.fill.blend(self.state.bg_color));

        if ui.input(|inp| !inp.raw.hovered_files.is_empty()) {
            center_frame = center_frame.fill(Color32::LIGHT_BLUE);
        }

        let panel = egui::CentralPanel::default().frame(center_frame);

        panel.show(ui, |ui| {
            let rect = ui.available_rect_before_wrap();

            if self.renderer.lock().unwrap().is_loaded() {
                self.model_view(ui, rect, frame);
            } else {
                let style = egui::Style::default();
                let mut job = egui::text::LayoutJob::default();
                egui::RichText::new("Drag and drop a .zip file to load a model")
                    .text_style(egui::TextStyle::Heading)
                    .size(25.0)
                    .append_to(
                        &mut job,
                        &style,
                        egui::FontSelection::Default,
                        egui::Align::Center,
                    );
                #[cfg(target_arch = "wasm32")]
                egui::RichText::new(
                    "\nThis app works entirely within your browser. \
                     \nNo part of your model or any other data is sent to a remote server.",
                )
                .append_to(
                    &mut job,
                    &style,
                    egui::FontSelection::Default,
                    egui::Align::Center,
                );
                ui.place(rect.shrink(10.0), egui::Label::new(job));
            }

            ui.take_available_space();
        });

        if self.state.request_capture.is_some() {
            ui.ctx().request_repaint();
        }
    }
}

impl CaptureRequest {
    #[cfg(target_arch = "wasm32")]
    fn save_png(buf: Vec<u8>, filename: &str) {
        let filename = filename.to_string();
        wasm_bindgen_futures::spawn_local(async move {
            use eframe::wasm_bindgen::JsCast as _;

            let document = web_sys::window()
                .expect("No window")
                .document()
                .expect("No document");

            let a = document
                .create_element("a")
                .unwrap()
                .dyn_into::<web_sys::HtmlAnchorElement>()
                .unwrap();

            let bytes = web_sys::js_sys::Array::new();
            bytes.push(&web_sys::js_sys::Uint8Array::from(&buf as &[u8]));
            let blob = web_sys::Blob::new_with_u8_array_sequence(&bytes).unwrap();
            let url = web_sys::Url::create_object_url_with_blob(&blob).unwrap();

            let body = document.body().unwrap();

            a.set_href(&url);
            a.set_download(&filename);
            body.append_with_node_1(&a).unwrap();
            a.click();
            a.remove();
            web_sys::Url::revoke_object_url(&url).unwrap();
        });
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn save_png(buf: Vec<u8>, filename: &str) {
        let file = rfd::FileDialog::new()
            .set_file_name(filename)
            .add_filter("PNG Image", &["png"])
            .save_file();
        if let Some(path) = file
            && let Ok(mut fd) = File::create(path)
        {
            use std::io::Write;
            fd.write_all(&buf).unwrap();
        }
    }

    fn capture(&self, renderer: &mut ModelRenderer, device: &wgpu::Device, queue: &wgpu::Queue) {
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Model screenshot encoder"),
        });

        let mut texture = ayagami_render::texture::Texture::new(
            device,
            self.view.dims_px.as_uvec2(),
            wgpu::TextureFormat::Rgba8Unorm,
            Some(1),
            Some("screenshot texture"),
        );

        let opts = RenderOptions {
            transform: self.view.transform,
            mask_dimensions: self.view.dims_px.as_uvec2(),
            // XXX Probably want linear, but only once the UI rendering is linear too.
            colorspace: RenderColorspace::SRgb,
        };

        renderer.prepare(&mut encoder, &opts);
        let bg = self.bg_color.to_normalized_gamma_f32();

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("screenshot renderpass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &texture.view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: bg[0] as f64,
                            g: bg[1] as f64,
                            b: bg[2] as f64,
                            a: bg[3] as f64,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
                multiview_mask: None,
            });

            renderer.render(&mut render_pass, texture.texture.format());
        }

        queue.submit([encoder.finish()]);

        if self.alpha_mode != CaptureAlphaMode::Premultiplied {
            let manager = ayagami_render::texture::TextureManager::new(device);
            texture = manager.unpremultiply(
                device,
                queue,
                &texture,
                self.alpha_mode == CaptureAlphaMode::StraightClamp,
            );
        }

        if let Some(slot) = self.clipboard_image.clone() {
            let width = texture.texture.width();
            let height = texture.texture.height();
            texture.download(device, queue, move |buf, bpr| {
                // This is a LIE: egui stores images premultiplied, but erroneously sends them to the
                // clipboard as-is, where they are (usually) interpreted as straight alpha.
                let img = egui::ColorImage::from_rgba_premultiplied(
                    [bpr as usize / 4, height as usize],
                    &buf,
                )
                .region_by_pixels([0, 0], [width as usize, height as usize]);
                *slot.lock().unwrap() = Some(img);
            });
        } else {
            texture.download_to_image(device, queue, move |img| {
                let mut buf = Vec::new();
                let writer = std::io::Cursor::new(&mut buf);
                let encoder = image::codecs::png::PngEncoder::new(writer);
                if img.write_with_encoder(encoder).is_ok() {
                    let dt = chrono::Local::now().format("%Y%m%d_%H%M%S");
                    let filename = format!("ayagami_screenshot_{}.png", dt);
                    Self::save_png(buf, &filename);
                }
            });
        }
    }
}

impl egui_wgpu::CallbackTrait for ModelView {
    // The callback function for WGPU is in two stages: prepare, and paint.
    //
    // The prepare callback is called every frame before paint and is given access to the wgpu
    // Device and Queue, which can be used, for instance, to update buffers and uniforms before
    // rendering.
    //
    // The paint callback is called after prepare and is given access to the render pass, which
    // can be used to issue draw commands.
    fn prepare(
        &self,
        _device: &wgpu::Device,
        _queue: &wgpu::Queue,
        _screen_descriptor: &egui_wgpu::ScreenDescriptor,
        egui_encoder: &mut wgpu::CommandEncoder,
        callback_resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let res: &mut RenderResources = callback_resources.get_mut().unwrap();

        let opts = RenderOptions {
            transform: self.transform,
            mask_dimensions: self.dims_px.as_uvec2(),
            colorspace: RenderColorspace::SRgb,
        };

        res.renderer.lock().unwrap().prepare(egui_encoder, &opts);

        vec![]
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        render_pass: &mut eframe::wgpu::RenderPass<'static>,
        callback_resources: &egui_wgpu::CallbackResources,
    ) {
        let res: &RenderResources = callback_resources.get().unwrap();

        render_pass.set_viewport(
            self.top_left_px.x,
            self.top_left_px.y,
            self.dims_px.x,
            self.dims_px.y,
            0.0,
            1.0,
        );

        res.renderer.lock().unwrap().render(render_pass, res.format);
    }
}
