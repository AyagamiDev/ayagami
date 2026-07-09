#![allow(unused)]

use std::{
    collections::HashMap,
    default,
    io::{self, Cursor, Read, Seek},
    iter,
    sync::{Arc, Mutex},
};

use anyhow::anyhow;
use wgpu::util::DeviceExt;

use ayagami::meta;
use ayagami::{
    core::{Model, Param},
    file,
};
use ayagami_render::*;
use glam::f32::{Affine2, Mat3, Vec2, vec2};
use log::{error, info};
use std::{env, fs::File};

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;
#[cfg(target_arch = "wasm32")]
use winit::platform::web::EventLoopExtWebSys;

use eframe::{
    egui::{self, Color32, LayerId},
    egui_wgpu,
};

#[derive(Default)]
pub struct AppState {
    transform: Affine2,
    param_values: HashMap<<file::ParsedModel as ayagami::core::Model>::Uid, f32>,
}

type ModelRenderer = ayagami_render::ModelRenderer<file::ParsedModel, Box<file::ParsedModel>>;

pub struct AyagamiTestApp {
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
        use std::ops::Index;

        let args: Vec<String> = env::args().collect();
        if let Some(filename) = args.get(1) {
            let mut zipf = File::open(&args[1])?;
            let mut archive = zip::ZipArchive::new(zipf)?;
            self.load_model(archive)?;
        }
        Ok(())
    }

    fn load_model<R: Read + Seek>(
        &mut self,
        mut archive: zip::ZipArchive<R>,
    ) -> anyhow::Result<()> {
        use std::path::PathBuf;

        let mut model3 = 'out: loop {
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
        let model = Box::new(file::ParsedModel::load(&mut moc)?);
        drop(moc);

        self.kp_param.clear();
        for param in model.params().into_iter() {
            if let Some(kp) = param.keypoints() {
                let kps: Vec<f32> = kp.iter().cloned().collect();
                self.kp_param.insert(param.id().to_owned(), kps);
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
        self.renderer.lock().unwrap().load_model(model, &texref)?;

        self.info = None;
        self.info_param.clear();

        if let Some(cdi_name) = info.file_references.display_info {
            info!("Loading display info...");
            let cdi_path = base.join(cdi_name);
            let mut cdi = archive.by_path(&cdi_path)?;
            let info: meta::DisplayInfo = serde_json::from_reader(cdi)?;

            for param in info.parameters.iter() {
                self.info_param.insert(param.id.clone(), param.clone());
            }

            self.info = Some(info);
        }

        Ok(())
    }

    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let render_state = cc.wgpu_render_state.as_ref().expect("WGPU enabled");

        let device = render_state.device.clone();
        let queue = render_state.queue.clone();

        let mut renderer = ModelRenderer::new(device, queue).expect("ModelRenderer init");

        let renderer = Arc::new(Mutex::new(renderer));

        let format = render_state.target_format.into();

        render_state
            .renderer
            .write()
            .callback_resources
            .insert(RenderResources {
                renderer: renderer.clone(),
                format,
            });

        let mut app = Self {
            renderer,
            state: Default::default(),
            info: Default::default(),
            info_param: Default::default(),
            kp_param: Default::default(),
        };

        if let Err(e) = app.load_startup_model() {
            error!("Failed to load startup model: {:?}", e);
        }

        app
    }

    fn model_view(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        let response = ui.interact(rect, egui::Id::NULL, egui::Sense::drag());

        // Apply drag (2x factor because viewport is -1..1)
        let drag = response.drag_delta() / rect.size() * 2.0;
        let delta = Affine2::from_translation(vec2(drag.x, -drag.y));
        self.state.transform = delta * self.state.transform;

        // Apply zoom (scroll + pinch-to-zoom)
        if response.hovered() {
            let e = &response.ctx.input(|r| {
                let rel_cursor = r
                    .pointer
                    .interact_pos()
                    .map(|p| ((p - rect.center()) / rect.size() * 2.))
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
        self.draw_model(ui, rect);
    }

    fn draw_model(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        // Figure out our viewport in pixels, to get 1:1 mask rendering
        let pixels_per_point = ui.pixels_per_point();
        let left_px = (pixels_per_point * rect.min.x).round();
        let top_px = (pixels_per_point * rect.min.y).round();
        let right_px = (pixels_per_point * rect.max.x).round();
        let bottom_px = (pixels_per_point * rect.max.y).round();
        let top_left_px = vec2(left_px, top_px);
        let bot_right_px = vec2(right_px, bottom_px);
        let dims_px = bot_right_px - top_left_px;

        let mut scale = if dims_px.x > dims_px.y {
            vec2(dims_px.y / dims_px.x, 1.)
        } else {
            vec2(1., dims_px.x / dims_px.y)
        };

        let transform = self.state.transform * Affine2::from_scale(1.8 * scale);

        let cb = egui_wgpu::Callback::new_paint_callback(
            rect,
            ModelView {
                top_left_px,
                dims_px,
                rect,
                transform,
            },
        );

        ui.painter().add(cb);
    }
    fn top_bar(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
        ui.add_space(8.0);

        egui::widgets::global_theme_preference_switch(ui);

        ui.separator();
        ui.label("Ayagami Demo App");

        if let Some(render_state) = frame.wgpu_render_state() {
            let info = render_state.adapter.get_info();

            ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                ui.add_space(8.0);
                ui.label(format!("{:?}", info.backend));
                ui.label("Backend:");
                ui.separator();
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
            let value = state.param_values.entry(param.uid).or_insert(param.default);
            ui.horizontal(|ui| {
                if ui.button("🔄").clicked() {
                    *value = param.default;
                    renderer.set_param(param.uid, param.default);
                }
                let res = ui.add(egui::Slider::new(value, param.min..=param.max).text(label));
                if res.changed() {
                    if res.ctx.input(|input| input.modifiers.shift) {
                        if let Some(closest) = kp_param
                            .get(&param.id)
                            .map(|v| {
                                v.iter().min_by(|a, b| {
                                    (*a - *value)
                                        .abs()
                                        .partial_cmp(&(*b - *value).abs())
                                        .unwrap()
                                })
                            })
                            .flatten()
                        {
                            *value = *closest;
                        }
                    }
                    renderer.set_param(param.uid, *value);
                }
            });
        }
    }

    fn left_panel(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let mut renderer = self.renderer.lock().unwrap();

        if let Some(info) = self.info.as_ref() {
            for group in info.parameter_groups.iter() {
                ui.collapsing(group.name.clone(), |ui| {
                    Self::parameter_group(
                        &mut self.state,
                        &self.info_param,
                        &self.kp_param,
                        &mut *renderer,
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
            &mut *renderer,
            ui,
            &"",
        );
    }
}

impl eframe::App for AyagamiTestApp {
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

        egui::Panel::left("left panel")
            .frame(egui::Frame::side_top_panel(ui.style()).inner_margin(6))
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    self.left_panel(ui, frame);
                });
            });

        ui.input_mut(|inp| {
            if let Some(f) = inp.raw.dropped_files.pop() {
                if let Some(p) = f.path {
                    info!("File dropped (path: {})", p.to_string_lossy());
                    let Ok(mut zipf) = File::open(&p) else {
                        error!("Failed to open file {}", p.to_string_lossy());
                        return;
                    };
                    let Ok(mut archive) = zip::ZipArchive::new(zipf) else {
                        error!("Failed to parse zip file");
                        return;
                    };
                    if let Err(e) = self.load_model(archive) {
                        error!("Failed to load model: {:?}", e);
                    }
                } else if let Some(b) = f.bytes {
                    info!("File dropped ({} bytes)", b.len());
                    let c = Cursor::new(b);
                    let Ok(mut archive) = zip::ZipArchive::new(c) else {
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

        let mut frame = egui::Frame::canvas(ui.style());

        if ui.input(|inp| !inp.raw.hovered_files.is_empty()) {
            frame = frame.fill(Color32::LIGHT_BLUE);
        }

        let panel = egui::CentralPanel::default().frame(frame);

        panel.show(ui, |ui| {
            let rect = ui.available_rect_before_wrap();

            if self.renderer.lock().unwrap().is_loaded() {
                self.model_view(ui, rect);
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
    }
}

struct ModelView {
    rect: egui::Rect,
    top_left_px: Vec2,
    dims_px: Vec2,
    transform: Affine2,
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
        device: &wgpu::Device,
        _queue: &wgpu::Queue,
        screen_descriptor: &egui_wgpu::ScreenDescriptor,
        egui_encoder: &mut wgpu::CommandEncoder,
        callback_resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let dims = self.rect.size();
        //println!("dims: {:?} {:?}", dims, self.dims_px);

        let scale = screen_descriptor.pixels_per_point;

        let res: &mut RenderResources = callback_resources.get_mut().unwrap();

        let opts = RenderOptions {
            transform: self.transform.clone(),
            mask_dimensions: self.dims_px.as_uvec2(),
            colorspace: RenderColorspace::SRgb,
        };

        res.renderer.lock().unwrap().prepare(egui_encoder, &opts);

        vec![]
    }

    fn paint(
        &self,
        info: egui::PaintCallbackInfo,
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
