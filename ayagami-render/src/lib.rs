// Based on learn-wgpu tutorial5
//
// https://github.com/sotrh/learn-wgpu/tree/master/code/beginner/tutorial5-textures
// License: MIT

#![allow(unused)]

use std::{
    io,
    io::Read,
    iter,
    sync::{Arc, Mutex},
};

use wgpu::util::DeviceExt;
use winit::{
    application::ApplicationHandler,
    event::*,
    event_loop::{ActiveEventLoop, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::Window,
};

use ayagami::file;
use glam::u32::uvec2;
use glam::{
    Affine2,
    f32::{Mat3, vec2},
};
use log::info;
use std::{env, fs::File};

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;
#[cfg(target_arch = "wasm32")]
use winit::platform::web::EventLoopExtWebSys;

mod renderer;
pub use renderer::*;
mod texture;

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 3],
    tex_coords: [f32; 2],
}

impl Vertex {
    fn desc() -> wgpu::VertexBufferLayout<'static> {
        use std::mem;
        wgpu::VertexBufferLayout {
            array_stride: mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: mem::size_of::<[f32; 3]>() as wgpu::BufferAddress,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x2,
                },
            ],
        }
    }
}

pub struct State {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    is_surface_configured: bool,
    window: Arc<Window>,

    renderer: renderer::ModelRenderer<file::ParsedModel, Arc<file::ParsedModel>>,
}

impl State {
    #[cfg(target_arch = "wasm32")]
    async fn get_model() -> anyhow::Result<(file::ParsedModel, Vec<Vec<u8>>)> {
        use gloo_utils::errors::JsError;
        use js_sys::{ArrayBuffer, futures::JsFuture};
        use web_sys::{Request, RequestInit, RequestMode, Response, js_sys};

        pub async fn fetch_as_vec_u8(resource_name: &str) -> anyhow::Result<Vec<u8>> {
            pub async fn fetch(resource_name: &str) -> Result<Vec<u8>, JsValue> {
                let opts = RequestInit::new();
                opts.set_method("GET");
                opts.set_mode(RequestMode::Cors);

                let request = Request::new_with_str_and_init(resource_name, &opts)?;

                let window = web_sys::window().unwrap();
                let resp_value = JsFuture::from(window.fetch_with_request(&request)).await?;

                assert!(resp_value.is_instance_of::<Response>());
                let resp: Response = resp_value.dyn_into().unwrap();

                let array_buf = JsFuture::from(resp.array_buffer()?).await?;
                assert!(array_buf.is_instance_of::<ArrayBuffer>());

                let typebuf: js_sys::Uint8Array = js_sys::Uint8Array::new(&array_buf);
                let mut body = vec![0; typebuf.length() as usize];
                typebuf.copy_to(&mut body[..]);

                Ok(body)
            }

            fetch(resource_name).await.map_err(|e| {
                let e: Result<JsError, _> = e.try_into();
                e.map_or_else(anyhow::Error::from, anyhow::Error::from)
            })
        }

        info!("Loading model...");
        let moc = fetch_as_vec_u8("2025_lina_chibi/2025_lina_chibi.moc3").await?;
        info!("Parsing model...");
        let model = file::ParsedModel::load(&mut &*moc).unwrap();

        let mut texdata: Vec<Vec<u8>> = Vec::new();

        let texnames = [
            "2025_lina_chibi/2025_lina_chibi.4096/texture_00.png",
            "2025_lina_chibi/2025_lina_chibi.4096/texture_01.png",
            "2025_lina_chibi/2025_lina_chibi.4096/texture_02.png",
        ];

        for name in texnames {
            let val = fetch_as_vec_u8(name).await?;
            texdata.push(val);
        }

        Ok((model, texdata))
    }

    #[cfg(not(target_arch = "wasm32"))]
    async fn get_model() -> anyhow::Result<(file::ParsedModel, Vec<Vec<u8>>)> {
        let args: Vec<String> = env::args().collect();

        let mut f = File::open(&args[1]).unwrap();

        info!("Loading model...");
        let model = file::ParsedModel::load(&mut f).unwrap();
        //println!("{:#?}", model);

        info!("Loading texture files...");
        let mut texdata: Vec<Vec<u8>> = Vec::new();
        for texf in &args[2..] {
            let mut f = File::open(texf).unwrap();
            let mut data = Vec::new();
            f.read_to_end(&mut data).unwrap();
            texdata.push(data);
        }

        Ok((model, texdata))
    }

    async fn new(window: Arc<Window>) -> anyhow::Result<State> {
        let (model, texdata) = Self::get_model().await.unwrap();

        info!("Initializing wgpu...");
        let size = window.inner_size();

        // The instance is a handle to our GPU
        // BackendBit::PRIMARY => Vulkan + Metal + DX12 + Browser WebGPU
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            #[cfg(not(target_arch = "wasm32"))]
            backends: wgpu::Backends::PRIMARY,
            #[cfg(target_arch = "wasm32")]
            backends: wgpu::Backends::GL,
            flags: Default::default(),
            memory_budget_thresholds: Default::default(),
            backend_options: Default::default(),
            display: None,
        });

        let surface = instance.create_surface(window.clone()).unwrap();

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .unwrap();
        let (device, mut queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: None,
                required_features: wgpu::Features::empty(),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                // WebGL doesn't support all of wgpu's features, so if
                // we're building for the web we'll have to disable some.
                required_limits: if cfg!(target_arch = "wasm32") {
                    wgpu::Limits {
                        max_texture_dimension_2d: 8192,
                        ..wgpu::Limits::downlevel_webgl2_defaults()
                    }
                } else {
                    wgpu::Limits::default()
                },
                memory_hints: Default::default(),
                trace: wgpu::Trace::Off, // Trace path
            })
            .await
            .unwrap();

        info!("Creating renderer...");
        let mut renderer = renderer::ModelRenderer::new(device.clone(), queue.clone())?;

        let model = Arc::new(model);
        let textures: Vec<&[u8]> = texdata.iter().map(|v| &v[..]).collect();
        info!("Loading model into renderer...");
        renderer.load_model(model, &textures)?;

        let surface_caps = surface.get_capabilities(&adapter);
        // Shader code in this tutorial assumes an Srgb surface texture. Using a different
        // one will result all the colors comming out darker. If you want to support non
        // Srgb surfaces, you'll need to account for that when drawing to the frame.
        let surface_format = surface_caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(surface_caps.formats[0]);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width,
            height: size.height,
            present_mode: surface_caps.present_modes[0],
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };

        info!("Initialized.");
        Ok(Self {
            surface,
            device,
            queue,
            config,
            renderer,
            is_surface_configured: false,
            window,
        })
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.is_surface_configured = true;
            self.config.width = width;
            self.config.height = height;
            self.surface.configure(&self.device, &self.config);
        }
    }

    fn handle_key(&mut self, event_loop: &ActiveEventLoop, key: KeyCode, pressed: bool) {
        #[allow(clippy::single_match)]
        match (key, pressed) {
            (KeyCode::Escape, true) => event_loop.exit(),
            _ => {}
        }
    }

    #[cfg(not(unix))]
    fn update(&mut self) {}

    #[cfg(unix)]
    fn update(&mut self) {
        use nix::poll::*;

        loop {
            use std::os::fd::AsFd;

            let stdin = io::stdin();

            let mut fds = [PollFd::new(stdin.as_fd(), PollFlags::POLLIN)];
            let r = poll(&mut fds, PollTimeout::ZERO).unwrap();

            if r <= 0 {
                break;
            }

            let mut buffer = String::new();
            let l = stdin.read_line(&mut buffer);

            if let Ok(json::JsonValue::Object(param)) = json::parse(&buffer) {
                for (k, v) in param.iter() {
                    if let json::JsonValue::Number(v) = v {
                        self.renderer.set_param_by_id(k, (*v).into()).unwrap()
                    }
                }
            }
        }
    }

    fn render(&mut self) -> anyhow::Result<()> {
        self.window.request_redraw();

        // We can't render unless the surface is configured
        if !self.is_surface_configured {
            return Ok(());
        }

        let output = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(surface_texture) => surface_texture,
            wgpu::CurrentSurfaceTexture::Suboptimal(surface_texture) => {
                self.surface.configure(&self.device, &self.config);
                surface_texture
            }
            wgpu::CurrentSurfaceTexture::Timeout
            | wgpu::CurrentSurfaceTexture::Occluded
            | wgpu::CurrentSurfaceTexture::Validation => {
                // Skip this frame
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Outdated => {
                self.surface.configure(&self.device, &self.config);
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Lost => {
                // You could recreate the devices and all resources
                // created with it here, but we'll just bail
                anyhow::bail!("Lost device");
            }
        };

        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let attachment = wgpu::RenderPassColorAttachment {
            view: &view,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: 0.0,
                }),
                store: wgpu::StoreOp::Store,
            },
            depth_slice: None,
        };

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Render Encoder"),
            });

        let dims = glam::u32::uvec2(view.texture().width(), view.texture().height()).as_vec2();

        let mut scale = if dims.x > dims.y {
            vec2(dims.y / dims.x, 1.)
        } else {
            vec2(1., dims.x / dims.y)
        };

        scale *= vec2(1.75, 1.75);

        let transform = Affine2::from_scale(scale);

        let dims = uvec2(view.texture().width(), view.texture().height());

        let opts = RenderOptions {
            transform,
            mask_dimensions: uvec2(view.texture().width(), view.texture().height()),
            colorspace: RenderColorspace::Linear,
        };

        self.renderer.prepare(&mut encoder, &opts);

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Model Render Pass"),
                color_attachments: &[Some(attachment.clone())],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
                multiview_mask: None,
            });

            self.renderer
                .render(&mut render_pass, view.texture().format());
        }

        self.queue.submit(iter::once(encoder.finish()));
        output.present();

        Ok(())
    }
}

pub fn run() {}
