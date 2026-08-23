use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    hash::Hash,
    num::NonZeroUsize,
};
use wgpu::util::DeviceExt;

use crate::texture::{Texture, TextureManager};

use ayagami::core::*;
use ayagami::driver::*;

use anyhow::Result;
#[cfg(not(target_arch = "wasm32"))]
use rayon::prelude::*;

use glam::f32::{Affine2, Mat4, Vec2, Vec3, Vec4, vec2, vec4};
use glam::u32::UVec2;
use log::{debug, info, trace};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum RendererError {
    #[error("Texture {0} is too large: {1}x{2}, max dimension is {3}")]
    TextureTooLarge(String, u32, u32, u32),
}

struct RenderTexture {
    _tex: Texture,
    bind_group: wgpu::BindGroup,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
struct PipelineMode {
    surface_format: wgpu::TextureFormat,
    blend_config: BlendConfig,
    cull: bool,
    mask: bool,
    blit: bool,
}

struct BufferTexture {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    bind_group: wgpu::BindGroup,
}

impl BufferTexture {
    fn new(
        device: &wgpu::Device,
        bind_group_layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        width: u32,
        height: u32,
        format: wgpu::TextureFormat,
        label: &str,
    ) -> Self {
        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
            label: Some(label),
        });

        BufferTexture {
            texture,
            view,
            bind_group,
        }
    }
}

struct ClipSet<T: Model> {
    targets: Vec<T::Uid>,
    dirty: Cell<bool>,
    update_queued: Cell<bool>,
    use_count: usize,
    cur_use_count: usize,
    texture: Option<BufferTexture>,
}

impl<T: Model> ClipSet<T> {
    fn create_texture(
        &mut self,
        device: &wgpu::Device,
        bind_group_layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        width: u32,
        height: u32,
    ) {
        let label = format!(
            "Clip texture: Targets={:?}, {} users",
            self.targets, self.use_count
        );

        self.texture = Some(BufferTexture::new(
            device,
            bind_group_layout,
            sampler,
            width,
            height,
            wgpu::TextureFormat::R8Unorm,
            &label,
        ));
    }
}

struct ArtMeshRenderData {
    clip_use_count: usize,
    dirty: bool,
    clip_set: Option<usize>,
    uniform_offset: usize,
}

struct PartRenderData {
    clip_set: Option<usize>,
    uniform_offset: usize,
}

struct LoadedModel<T: Model, R: AsRef<T>> {
    model: R,
    driver: Driver<T>,
    vertex_buffer: wgpu::Buffer,
    texcoord_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    artmesh_buffer: wgpu::Buffer,
    uniform_bind_group: wgpu::BindGroup,

    clip_sets: Vec<ClipSet<T>>,
    artmesh_data: HashMap<T::Uid, ArtMeshRenderData>,
    part_data: HashMap<T::Uid, PartRenderData>,
    needs_offscreen: bool,
    update_queued: Cell<bool>,
}

struct RendererStatic {
    device: wgpu::Device,
    queue: wgpu::Queue,
    texture_bind_group_layout: wgpu::BindGroupLayout,
    mask_bind_group_layout: wgpu::BindGroupLayout,
    uniform_bind_group_layout: wgpu::BindGroupLayout,
    shader: wgpu::ShaderModule,
    pipeline_layout: wgpu::PipelineLayout,
    mask_pipeline_layout: wgpu::PipelineLayout,
    adv_pipeline_layout: wgpu::PipelineLayout,
    adv_mask_pipeline_layout: wgpu::PipelineLayout,
    mask_sampler: wgpu::Sampler,
    mask_pipeline_nocull: wgpu::RenderPipeline,
    mask_pipeline_cull: wgpu::RenderPipeline,
}

#[derive(Default)]
struct RendererCache {
    render_pipelines: HashMap<PipelineMode, wgpu::RenderPipeline>,
}

impl RendererCache {
    fn render_pipeline(
        &mut self,
        stat: &RendererStatic,
        mode: PipelineMode,
    ) -> &wgpu::RenderPipeline {
        self.render_pipelines.entry(mode).or_insert_with(|| {
            // Verified against VTube Studio behavior
            let blend = match (mode.blend_config.color, mode.blend_config.alpha) {
                (ColorBlendMode::Normal, AlphaBlendMode::Over) => {
                    wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING
                }
                (ColorBlendMode::PremultAdd, _) => wgpu::BlendState {
                    color: wgpu::BlendComponent {
                        src_factor: wgpu::BlendFactor::One,
                        dst_factor: wgpu::BlendFactor::One,
                        operation: wgpu::BlendOperation::Add,
                    },
                    alpha: wgpu::BlendComponent {
                        src_factor: wgpu::BlendFactor::Zero,
                        dst_factor: wgpu::BlendFactor::One,
                        operation: wgpu::BlendOperation::Add,
                    },
                },
                (ColorBlendMode::PremultMultiply, _) => wgpu::BlendState {
                    color: wgpu::BlendComponent {
                        src_factor: wgpu::BlendFactor::Dst,
                        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                        operation: wgpu::BlendOperation::Add,
                    },
                    alpha: wgpu::BlendComponent {
                        src_factor: wgpu::BlendFactor::Zero,
                        dst_factor: wgpu::BlendFactor::One,
                        operation: wgpu::BlendOperation::Add,
                    },
                },
                _ => wgpu::BlendState::REPLACE,
            };

            let write_mask = match mode.blend_config.color {
                ColorBlendMode::PremultAdd | ColorBlendMode::PremultMultiply => {
                    wgpu::ColorWrites::COLOR
                }
                _ => wgpu::ColorWrites::ALL,
            };

            let fs_entry = match (mode.blend_config.is_advanced(), mode.mask) {
                (false, false) => "fs_normal",
                (false, true) => "fs_normal_mask",
                (true, false) => "fs_advanced",
                (true, true) => "fs_advanced_mask",
            };

            let pipeline_layout = match (mode.blend_config.is_advanced(), mode.mask) {
                (false, false) => &stat.pipeline_layout,
                (false, true) => &stat.mask_pipeline_layout,
                (true, false) => &stat.adv_pipeline_layout,
                (true, true) => &stat.adv_mask_pipeline_layout,
            };

            let vertex = if mode.blit {
                wgpu::VertexState {
                    module: &stat.shader,
                    entry_point: Some("vs_blit"),
                    buffers: &[],
                    compilation_options: Default::default(),
                }
            } else {
                wgpu::VertexState {
                    module: &stat.shader,
                    entry_point: Some("vs_main"),
                    buffers: &[Vertex::desc(), TexCoord::desc()],
                    compilation_options: Default::default(),
                }
            };

            let label = format!("Ayagami: {:?}", mode);

            stat.device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some(&label),
                    layout: Some(pipeline_layout),
                    vertex,
                    fragment: Some(wgpu::FragmentState {
                        module: &stat.shader,
                        entry_point: Some(fs_entry),
                        targets: &[Some(wgpu::ColorTargetState {
                            format: mode.surface_format,
                            blend: Some(blend),
                            write_mask,
                        })],
                        compilation_options: Default::default(),
                    }),
                    primitive: wgpu::PrimitiveState {
                        topology: wgpu::PrimitiveTopology::TriangleList,
                        strip_index_format: None,
                        front_face: wgpu::FrontFace::Ccw,
                        cull_mode: if mode.cull {
                            Some(wgpu::Face::Front)
                        } else {
                            None
                        },
                        // Setting this to anything other than Fill requires Features::POLYGON_MODE_LINE
                        // or Features::POLYGON_MODE_POINT
                        polygon_mode: wgpu::PolygonMode::Fill,
                        // Requires Features::DEPTH_CLIP_CONTROL
                        unclipped_depth: false,
                        // Requires Features::CONSERVATIVE_RASTERIZATION
                        conservative: false,
                    },
                    depth_stencil: None,
                    multisample: wgpu::MultisampleState {
                        count: 1,
                        mask: !0,
                        alpha_to_coverage_enabled: false,
                    },
                    // If the pipeline will be used with a multiview render pass, this
                    // tells wgpu to render to just specific texture layers.
                    multiview_mask: None,
                    // Useful for optimizing shader compilation on Android
                    cache: None,
                })
        })
    }
}

pub struct ModelRenderer<T: Model, R: AsRef<T>> {
    stat: RendererStatic,
    global_buffer: wgpu::Buffer,

    model: Option<LoadedModel<T, R>>,
    textures: Vec<RenderTexture>,
    cache: RefCell<RendererCache>,
    offscreen_top: Vec<BufferTexture>,
    buffer_pool: Vec<BufferTexture>,
    shadow_fb: Option<BufferTexture>,
    mask_dimensions: UVec2,
    transform: Affine2,
}

#[repr(C)]
#[derive(Copy, Clone, Default, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: Coord,
}

#[repr(C)]
#[derive(Copy, Clone, Default, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct TexCoord {
    tex_coords: Coord,
}

#[repr(C)]
#[derive(Debug, Default, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct GlobalUniforms {
    view_mtx: [[f32; 4]; 4],
}

impl GlobalUniforms {
    fn new(transform: &Affine2) -> Self {
        let x = transform.matrix2.x_axis;
        let y = transform.matrix2.y_axis;
        let t = transform.translation;
        let mat = Mat4::from_cols(
            vec4(x.x, x.y, 0., 0.),
            vec4(y.x, y.y, 0., 0.),
            Vec4::ZERO,
            vec4(t.x, t.y, 0., 1.),
        );
        Self {
            view_mtx: mat.to_cols_array_2d(),
        }
    }
}

#[repr(C)]
#[derive(Debug, Default, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct ArtMeshUniform {
    // Note: Ordered to optimize packing & avoid padding
    multiply_color: Vec3,
    opacity: f32,
    screen_color: Vec3,
    mask_invert: u32,
    linear_to_srgb: u32,
    color_blend: u32,
    alpha_blend: u32,
    _pad: u32,
}

const ARTMESH_UNIFORM_STRIDE: NonZeroUsize =
    NonZeroUsize::new(core::mem::size_of::<ArtMeshUniform>().next_multiple_of(256)).unwrap();

impl Vertex {
    fn desc() -> wgpu::VertexBufferLayout<'static> {
        use std::mem;
        wgpu::VertexBufferLayout {
            array_stride: mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[wgpu::VertexAttribute {
                offset: 0,
                shader_location: 0,
                format: wgpu::VertexFormat::Float32x2,
            }],
        }
    }
}

impl TexCoord {
    fn desc() -> wgpu::VertexBufferLayout<'static> {
        use std::mem;
        wgpu::VertexBufferLayout {
            array_stride: mem::size_of::<TexCoord>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[wgpu::VertexAttribute {
                offset: 0,
                shader_location: 1,
                format: wgpu::VertexFormat::Float32x2,
            }],
        }
    }
}

pub struct ParamInfo<T: Model> {
    pub uid: T::Uid,
    pub id: String,
    pub min: f32,
    pub max: f32,
    pub default: f32,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum RenderColorspace {
    Linear,
    SRgb,
}

pub struct RenderOptions {
    pub mask_dimensions: UVec2,
    pub colorspace: RenderColorspace,
    pub transform: Affine2,
}

impl<T: Model, R: AsRef<T>> ModelRenderer<T, R> {
    pub fn new(device: wgpu::Device, queue: wgpu::Queue) -> Result<Self> {
        info!("Device limits:");
        info!(
            "  Maximum texture dimension: {}",
            device.limits().max_texture_dimension_2d
        );
        info!("Creating WGPU objects...");
        let texture_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            multisampled: false,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
                label: Some("texture_bind_group_layout"),
            });

        let mask_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            multisampled: false,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
                label: Some("mask_bind_group_layout"),
            });

        let fb_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            multisampled: false,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
                label: Some("fb_bind_group_layout"),
            });

        let uniform_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: true,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
                label: Some("uniform_bind_group_layout"),
            });

        let global_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ArtMesh Buffer"),
            size: core::mem::size_of::<GlobalUniforms>().next_multiple_of(16) as u64,
            mapped_at_creation: false,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Render Pipeline Layout"),
            bind_group_layouts: &[
                Some(&uniform_bind_group_layout),
                Some(&texture_bind_group_layout),
            ],
            immediate_size: 0,
        });
        let mask_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Render Pipeline Layout (Masked)"),
            bind_group_layouts: &[
                Some(&uniform_bind_group_layout),
                Some(&texture_bind_group_layout),
                Some(&mask_bind_group_layout),
            ],
            immediate_size: 0,
        });
        let adv_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Render Pipeline Layout"),
            bind_group_layouts: &[
                Some(&uniform_bind_group_layout),
                Some(&texture_bind_group_layout),
                None,
                Some(&fb_bind_group_layout),
            ],
            immediate_size: 0,
        });
        let adv_mask_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Render Pipeline Layout (Masked)"),
                bind_group_layouts: &[
                    Some(&uniform_bind_group_layout),
                    Some(&texture_bind_group_layout),
                    Some(&mask_bind_group_layout),
                    Some(&fb_bind_group_layout),
                ],
                immediate_size: 0,
            });

        let mask_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            address_mode_w: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        info!("Loading shaders...");
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });

        let mask_pipeline_nocull =
            Self::make_mask_pipeline(&device, &pipeline_layout, &shader, false);
        let mask_pipeline_cull = Self::make_mask_pipeline(&device, &pipeline_layout, &shader, true);

        info!("Done initializing renderer.");
        Ok(Self {
            stat: RendererStatic {
                device,
                queue,
                shader,
                texture_bind_group_layout,
                mask_bind_group_layout,
                uniform_bind_group_layout,
                pipeline_layout,
                mask_pipeline_layout,
                adv_pipeline_layout,
                adv_mask_pipeline_layout,
                mask_sampler,
                mask_pipeline_nocull,
                mask_pipeline_cull,
            },

            global_buffer,
            model: None,
            textures: Vec::new(),
            cache: Default::default(),
            offscreen_top: Default::default(),
            buffer_pool: Default::default(),
            shadow_fb: None,
            mask_dimensions: Default::default(),
            transform: Affine2::IDENTITY,
        })
    }

    fn make_mask_pipeline(
        device: &wgpu::Device,
        pipeline_layout: &wgpu::PipelineLayout,
        shader: &wgpu::ShaderModule,
        cull: bool,
    ) -> wgpu::RenderPipeline {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Mask Render Pipeline"),
            layout: Some(pipeline_layout),
            vertex: wgpu::VertexState {
                module: shader,
                entry_point: Some("vs_main"),
                buffers: &[Vertex::desc(), TexCoord::desc()],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: shader,
                entry_point: Some("fs_render_mask"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::R8Unorm,
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::OneMinusSrc,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent::OVER,
                    }),
                    write_mask: wgpu::ColorWrites::RED,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: if cull { Some(wgpu::Face::Front) } else { None },
                // Setting this to anything other than Fill requires Features::POLYGON_MODE_LINE
                // or Features::POLYGON_MODE_POINT
                polygon_mode: wgpu::PolygonMode::Fill,
                // Requires Features::DEPTH_CLIP_CONTROL
                unclipped_depth: false,
                // Requires Features::CONSERVATIVE_RASTERIZATION
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            // If the pipeline will be used with a multiview render pass, this
            // tells wgpu to render to just specific texture layers.
            multiview_mask: None,
            // Useful for optimizing shader compilation on Android
            cache: None,
        })
    }

    pub fn is_loaded(&self) -> bool {
        self.model.is_some()
    }

    pub fn load_model(&mut self, model: R, textures: &[&[u8]]) -> Result<()> {
        info!("Initializing texture manager...");
        let manager = TextureManager::new(&self.stat.device);

        info!("Loading textures...");

        // wasm build of wgpu stuff is not thread-safe, fails bounds despite rayon
        // being single-threaded in that build.
        #[cfg(target_arch = "wasm32")]
        let it = textures.iter();
        #[cfg(not(target_arch = "wasm32"))]
        let it = textures.par_iter();

        self.textures = it
            .enumerate()
            .map(|(i, bytes)| -> Result<_> {
                let name = format!("model-texture-{0}", i);
                let tex = Texture::from_bytes(&self.stat.device, &self.stat.queue, bytes, &name)?;
                manager.premultiply(&self.stat.device, &self.stat.queue, &tex);
                manager.gen_mips(&self.stat.device, &self.stat.queue, &tex);
                let bind_group = self
                    .stat
                    .device
                    .create_bind_group(&wgpu::BindGroupDescriptor {
                        layout: &self.stat.texture_bind_group_layout,
                        entries: &[
                            wgpu::BindGroupEntry {
                                binding: 0,
                                resource: wgpu::BindingResource::TextureView(&tex.view),
                            },
                            wgpu::BindGroupEntry {
                                binding: 1,
                                resource: wgpu::BindingResource::Sampler(&tex.sampler),
                            },
                        ],
                        label: Some("model_bind_group"),
                    });
                Ok(RenderTexture {
                    _tex: tex,
                    bind_group,
                })
            })
            .collect::<Result<Vec<_>>>()?;

        self.reload_model(model)?;

        Ok(())
    }

    pub fn reload_model(&mut self, model: R) -> Result<()> {
        let m = model.as_ref();

        let mut driver = Driver::new(m);
        driver.drive(m);

        let mut needs_offscreen = false;
        for node in driver.draw_nodes(None).unwrap() {
            if match node {
                DrawNode::ArtMesh(uid) => {
                    let artmesh = m.artmeshes().get(*uid).unwrap();
                    artmesh.blend_config().is_advanced()
                }
                DrawNode::OffscreenPart(uid) => {
                    let part = m.parts().get(*uid).unwrap();
                    part.blend_config().unwrap().is_advanced()
                }
            } {
                needs_offscreen = true;
                break;
            }
        }

        let vertex_buffer = self.stat.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Vertex Buffer"),
            size: std::mem::size_of_val(m.texcoord_buffer().unwrap()) as u64,
            mapped_at_creation: false,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        });

        let texcoord_buffer =
            self.stat
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Texcoord Buffer"),
                    contents: bytemuck::cast_slice(m.texcoord_buffer().unwrap()),
                    usage: wgpu::BufferUsages::VERTEX,
                });

        let index_buffer = self
            .stat
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Index Buffer"),
                contents: bytemuck::cast_slice(m.index_buffer().unwrap()),
                usage: wgpu::BufferUsages::INDEX,
            });

        let mut artmesh_data = HashMap::new();
        let mut clip_map = HashMap::new();
        let mut clip_sets = Vec::new();
        let mut uniform_offset: usize = ARTMESH_UNIFORM_STRIDE.get();
        for artmesh in m.artmeshes() {
            let clips: Vec<_> = artmesh.clips().into_iter().map(|c| c.uid()).collect();
            let clip_set = if clips.is_empty() {
                None
            } else {
                let idx = if !clip_map.contains_key(&clips) {
                    let i = clip_sets.len();
                    clip_map.insert(clips.clone(), i);
                    clip_sets.push(ClipSet {
                        targets: clips,
                        use_count: 0,
                        cur_use_count: 0,
                        texture: None,
                        dirty: false.into(),
                        update_queued: false.into(),
                    });
                    i
                } else {
                    *clip_map.get(&clips).unwrap()
                };
                clip_sets[idx].use_count += 1;

                Some(idx)
            };

            artmesh_data.insert(
                artmesh.uid(),
                ArtMeshRenderData {
                    clip_use_count: 0,
                    dirty: true,
                    clip_set,
                    uniform_offset,
                },
            );
            uniform_offset += ARTMESH_UNIFORM_STRIDE.get();
        }

        let mut part_data = HashMap::new();
        for part in m.parts() {
            if !part.offscreen() {
                continue;
            }

            let clips: Vec<_> = part.clips().unwrap().into_iter().map(|c| c.uid()).collect();
            let clip_set = if clips.is_empty() {
                None
            } else {
                let idx = if !clip_map.contains_key(&clips) {
                    let i = clip_sets.len();
                    clip_map.insert(clips.clone(), i);
                    clip_sets.push(ClipSet {
                        targets: clips,
                        use_count: 0,
                        cur_use_count: 0,
                        texture: None,
                        dirty: false.into(),
                        update_queued: false.into(),
                    });
                    i
                } else {
                    *clip_map.get(&clips).unwrap()
                };
                clip_sets[idx].use_count += 1;

                Some(idx)
            };

            part_data.insert(
                part.uid(),
                PartRenderData {
                    clip_set,
                    uniform_offset,
                },
            );
            uniform_offset += ARTMESH_UNIFORM_STRIDE.get();
        }

        let artmesh_buffer = self.stat.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ArtMesh Uniform Buffer"),
            size: uniform_offset as u64,
            mapped_at_creation: false,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let uniform_bind_group = self
            .stat
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                layout: &self.stat.uniform_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: self.global_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: &artmesh_buffer,
                            offset: 0,
                            size: Some(
                                (core::mem::size_of::<ArtMeshUniform>() as u64)
                                    .try_into()
                                    .unwrap(),
                            ),
                        }),
                    },
                ],
                label: Some("uniform_bind_group"),
            });

        info!("Number of distinct clipping masks: {}", clip_sets.len());

        self.mask_dimensions = UVec2::ZERO;

        self.model = Some(LoadedModel {
            model,
            driver,
            vertex_buffer,
            index_buffer,
            texcoord_buffer,
            artmesh_buffer,
            uniform_bind_group,
            clip_sets,
            artmesh_data,
            part_data,
            update_queued: Default::default(),
            needs_offscreen,
        });
        Ok(())
    }

    pub fn params(&self) -> Vec<ParamInfo<T>> {
        if self.model.is_none() {
            return Vec::new();
        }
        let mref = self.model.as_ref().unwrap().model.as_ref();
        let mut params = Vec::new();
        for p in mref.params() {
            params.push(ParamInfo {
                uid: p.uid(),
                id: p.id().to_string(),
                min: p.min(),
                max: p.max(),
                default: p.default(),
            });
        }

        params
    }

    pub fn driver(&mut self) -> &mut Driver<T> {
        let md = self.model.as_mut().unwrap();
        &mut md.driver
    }

    pub fn prepare(&mut self, encoder: &mut wgpu::CommandEncoder, options: &RenderOptions) -> bool {
        let mut any_changes = false;
        let mut redraw_clips = false;

        let md = self.model.as_mut().unwrap();
        let m = md.model.as_ref();
        md.driver.drive(m);

        // ==== Upload global uniforms (camera)

        let srgb = options.colorspace == RenderColorspace::SRgb;
        let camera = GlobalUniforms::new(&(options.transform * Affine2::from_scale(vec2(1., -1.))));
        self.stat
            .queue
            .write_buffer(&self.global_buffer, 0, bytemuck::cast_slice(&[camera]));

        // ==== Have surface dimensions changed? Re-create mask textures

        if self.mask_dimensions != options.mask_dimensions {
            self.offscreen_top.clear();
            self.buffer_pool.clear();
            self.shadow_fb = None;
            for (i, cs) in md.clip_sets.iter_mut().enumerate() {
                debug!(
                    "Create clip set {} texture: {}x{}",
                    i, options.mask_dimensions.x, options.mask_dimensions.y
                );
                cs.create_texture(
                    &self.stat.device,
                    &self.stat.mask_bind_group_layout,
                    &self.stat.mask_sampler,
                    options.mask_dimensions.x,
                    options.mask_dimensions.y,
                );
                cs.dirty.set(true);
            }
            self.mask_dimensions = options.mask_dimensions;
            redraw_clips = true;
            any_changes = true;
        }

        // ==== Record which ArtMeshes have changed & are visible

        for artmesh in m.artmeshes() {
            let state = md.driver.artmesh_state(artmesh.uid()).unwrap();
            if state.updated {
                let am_data = md.artmesh_data.get_mut(&artmesh.uid()).unwrap();
                debug!("ArtMesh #{} {} changed", artmesh.uid(), artmesh.id());
                am_data.dirty = true;
                any_changes = any_changes || state.visual.visible;
            }
        }

        // Check if the transform has changed, if so we need to redraw clips

        if options.transform != self.transform {
            redraw_clips = true;
            any_changes = true;
            self.transform = options.transform;
        }

        // Check if any clip sets have an update queued, if so render() was
        // not called which means the update might have been dropped, so
        // do not early exit.
        for clip_set in md.clip_sets.iter_mut() {
            if clip_set.update_queued.get() {
                any_changes = true;
            }
        }

        // Check if there are offscreen renders pending
        if md.update_queued.get() {
            any_changes = true;
        }

        // If truly nothing changed and clip masks are up to date, early exit
        if !any_changes {
            return false;
        }

        // ==== Upload global uniforms (ArtMesh)

        let mut am_buf_view = self
            .stat
            .queue
            .write_buffer_with(
                &md.artmesh_buffer,
                0,
                md.artmesh_buffer.size().try_into().unwrap(),
            )
            .unwrap();

        // Uniform set for plain blit (top-level)
        am_buf_view
            .slice(0..core::mem::size_of::<ArtMeshUniform>())
            .copy_from_slice(bytemuck::cast_slice(&[ArtMeshUniform {
                multiply_color: Vec3::ONE,
                screen_color: Vec3::ZERO,
                opacity: 1.0,
                mask_invert: 0,
                linear_to_srgb: 0,
                color_blend: ColorBlendMode::Normal as u32,
                alpha_blend: AlphaBlendMode::Over as u32,
                ..Default::default()
            }]));

        // ==== Record which masks have changed

        for clip_set in md.clip_sets.iter_mut() {
            clip_set.update_queued.set(false);
            if redraw_clips {
                clip_set.dirty.set(true);
            }
            for uid in clip_set.targets.iter() {
                let am_data = md.artmesh_data.get(uid).unwrap();
                if am_data.dirty {
                    clip_set.dirty.set(true);
                }
            }
        }

        // ==== Enumerate masks in use

        for clip_set in md.clip_sets.iter_mut() {
            clip_set.cur_use_count = 0;
        }

        for am_data in md.artmesh_data.values_mut() {
            am_data.clip_use_count = 0;
        }

        let mut part_queue = vec![None];
        while !part_queue.is_empty() {
            let mut more_parts = Vec::new();

            for part_uid in part_queue {
                for node in md.driver.draw_nodes(part_uid).unwrap() {
                    match node {
                        DrawNode::OffscreenPart(uid) => {
                            let state = md.driver.part_state(*uid).unwrap();
                            let visual = state.visual.unwrap();
                            if visual.opacity != 0. && visual.visible {
                                more_parts.push(Some(*uid));
                                let part_data = md.part_data.get(uid).unwrap();
                                if let Some(idx) = part_data.clip_set {
                                    md.clip_sets[idx].cur_use_count += 1;
                                }
                            }
                            continue;
                        }
                        DrawNode::ArtMesh(uid) => {
                            let state = md.driver.artmesh_state(*uid).unwrap();
                            if state.visual.opacity != 0. && state.visual.visible {
                                let am_data = md.artmesh_data.get(uid).unwrap();
                                if let Some(idx) = am_data.clip_set {
                                    md.clip_sets[idx].cur_use_count += 1;
                                }
                            }
                        }
                    };
                }
            }
            part_queue = more_parts;
        }

        for clip_set in md.clip_sets.iter_mut() {
            if clip_set.cur_use_count != 0 {
                for uid in clip_set.targets.iter() {
                    let am_data = md.artmesh_data.get_mut(uid).unwrap();
                    am_data.clip_use_count += 1;
                }
            }
        }

        // ==== Upload ArtMesh vertex data & uniforms

        for artmesh in m.artmeshes() {
            let state = md.driver.artmesh_state(artmesh.uid()).unwrap();
            // Disabled ArtMeshes are entirely ignored
            if !state.visual.visible {
                continue;
            }

            let am_data = md.artmesh_data.get_mut(&artmesh.uid()).unwrap();
            // Opacity 0 ArtMeshes are ignored if they are not used in clips
            if state.visual.opacity == 0. && am_data.clip_use_count == 0 {
                continue;
            }

            // Upload uniforms, if opacity > 0
            // Uniforms are not used for clip mask generation, so can be skipped in that case
            // Uniforms are uploaded in one operation, so build the whole buffer including
            // unchanged ArtMeshes
            if state.visual.opacity != 0. {
                let artmesh_uniforms = ArtMeshUniform {
                    opacity: state.visual.opacity,
                    multiply_color: state.visual.multiply_color,
                    screen_color: state.visual.screen_color,
                    mask_invert: if artmesh.invert_mask() { 1 } else { 0 },
                    linear_to_srgb: if srgb { 1 } else { 0 },
                    color_blend: artmesh.blend_config().color as u32,
                    alpha_blend: artmesh.blend_config().alpha as u32,
                    ..Default::default()
                };

                let off = am_data.uniform_offset;
                am_buf_view
                    .slice(off..off + core::mem::size_of::<ArtMeshUniform>())
                    .copy_from_slice(bytemuck::cast_slice(&[artmesh_uniforms]));
            }

            // If nothing changed, no need to upload vertex data
            if !am_data.dirty {
                continue;
            }

            // Upload vertex data
            let start = 8 * artmesh.texcoord_offset() as u64;
            let size = 8 * artmesh.vertex_count() as u64;
            let mut vtx_buf_view = self
                .stat
                .queue
                .write_buffer_with(&md.vertex_buffer, start, size.try_into().unwrap())
                .unwrap();

            vtx_buf_view.copy_from_slice(bytemuck::cast_slice(state.vertices));

            am_data.dirty = false;
        }

        // Upload offscreen part uniforms
        for (uid, part_data) in md.part_data.iter() {
            let part = m.parts().get(*uid).unwrap();
            let blend_config = part.blend_config().unwrap();
            let state = md.driver.part_state(*uid).unwrap();
            let visual = state.visual.unwrap();

            // Upload uniforms, if opacity > 0
            // Uniforms are uploaded in one operation, so build the whole buffer including
            // unchanged Parts
            if visual.opacity != 0. {
                let artmesh_uniforms = ArtMeshUniform {
                    opacity: visual.opacity,
                    multiply_color: visual.multiply_color,
                    screen_color: visual.screen_color,
                    mask_invert: if part.invert_mask().unwrap() { 1 } else { 0 },
                    linear_to_srgb: 0,
                    color_blend: blend_config.color as u32,
                    alpha_blend: blend_config.alpha as u32,
                    ..Default::default()
                };

                let off = part_data.uniform_offset;
                am_buf_view
                    .slice(off..off + core::mem::size_of::<ArtMeshUniform>())
                    .copy_from_slice(bytemuck::cast_slice(&[artmesh_uniforms]));
            }
        }

        // ==== Render clip masks
        let mut clips_updated = false;
        for (clip_idx, clip) in md.clip_sets.iter_mut().enumerate() {
            // Skip if no users or it's not dirty
            if clip.cur_use_count == 0 || !clip.dirty.get() {
                continue;
            }

            let clip_attachment = wgpu::RenderPassColorAttachment {
                view: &clip.texture.as_ref().unwrap().view,
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

            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Mask Render Pass"),
                color_attachments: &[Some(clip_attachment)],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
                multiview_mask: None,
            });

            render_pass.set_index_buffer(md.index_buffer.slice(..), wgpu::IndexFormat::Uint16);

            for uid in clip.targets.iter() {
                let artmesh = m.artmeshes().get(*uid).unwrap();
                let state = md.driver.artmesh_state(artmesh.uid()).unwrap();
                // Masks ignore opacity, but honor visible flag
                if !state.visual.visible {
                    continue;
                }

                let am_data = md.artmesh_data.get(&artmesh.uid()).unwrap();
                let tex = artmesh.texture();
                if artmesh.culling() {
                    render_pass.set_pipeline(&self.stat.mask_pipeline_cull);
                } else {
                    render_pass.set_pipeline(&self.stat.mask_pipeline_nocull);
                }

                render_pass.set_bind_group(
                    0,
                    &md.uniform_bind_group,
                    &[am_data.uniform_offset as u32],
                );
                render_pass.set_bind_group(1, &self.textures[tex as usize].bind_group, &[]);

                let texcoord_off = artmesh.texcoord_offset() as u64;
                render_pass.set_vertex_buffer(0, md.vertex_buffer.slice(8 * texcoord_off..));
                render_pass.set_vertex_buffer(1, md.texcoord_buffer.slice(8 * texcoord_off..));

                trace!(
                    "Render ArtMesh #{}: {} for clip set {} ({}/{} users): {:?}",
                    artmesh.uid(),
                    artmesh.id(),
                    clip_idx,
                    clip.cur_use_count,
                    clip.use_count,
                    state.visual,
                );

                // Note: Mask rendering does not use ArtMesh uniforms, so no need to ensure they are uploaded
                render_pass.draw_indexed(artmesh.index_range(), 0, 0..1);
            }

            clips_updated = true;
            clip.update_queued.set(true);
        }

        self.buffer_pool.append(&mut self.offscreen_top);

        if md.needs_offscreen {
            let top_buffer = self.get_offscreen_buffer();
            self.render_offscreen(encoder, &top_buffer, None);
            self.offscreen_top = vec![top_buffer];
            let md = self.model.as_mut().unwrap();
            md.update_queued.set(true);
            true
        } else if !md.part_data.is_empty() {
            self.offscreen_top = self.render_offscreen_children(encoder, None);
            assert!(!self.offscreen_top.is_empty());
            let md = self.model.as_mut().unwrap();
            md.update_queued.set(true);
            true
        } else {
            clips_updated
        }
    }

    fn get_offscreen_buffer(&mut self) -> BufferTexture {
        self.buffer_pool.pop().unwrap_or_else(|| {
            BufferTexture::new(
                &self.stat.device,
                &self.stat.mask_bind_group_layout,
                &self.stat.mask_sampler,
                self.mask_dimensions.x,
                self.mask_dimensions.y,
                wgpu::TextureFormat::Bgra8Unorm,
                "part_buf",
            )
        })
    }

    fn render_offscreen_children(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        part: Option<T::Uid>,
    ) -> Vec<BufferTexture> {
        let mut off_parts = Vec::new();

        let md = self.model.as_ref().unwrap();
        let nodes = md.driver.draw_nodes(part).unwrap().to_vec();

        for node in nodes.iter() {
            let DrawNode::OffscreenPart(uid) = node else {
                continue;
            };

            let md = self.model.as_ref().unwrap();
            let state = md.driver.part_state(*uid).unwrap();
            let visual = state.visual.unwrap();

            if visual.opacity == 0. || !visual.visible {
                continue;
            }

            let part_buf = self.buffer_pool.pop().unwrap_or_else(|| {
                BufferTexture::new(
                    &self.stat.device,
                    &self.stat.mask_bind_group_layout,
                    &self.stat.mask_sampler,
                    self.mask_dimensions.x,
                    self.mask_dimensions.y,
                    wgpu::TextureFormat::Bgra8Unorm,
                    "part_buf",
                )
            });

            self.render_offscreen(encoder, &part_buf, Some(*uid));
            off_parts.push(part_buf);
        }

        off_parts
    }

    fn render_offscreen(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        buffer: &BufferTexture,
        part: Option<T::Uid>,
    ) {
        let mut offscreen_children = self.render_offscreen_children(encoder, part);

        let attachment = wgpu::RenderPassColorAttachment {
            view: &buffer.view,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                store: wgpu::StoreOp::Store,
            },
            depth_slice: None,
        };

        let attachment_next = wgpu::RenderPassColorAttachment {
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Load,
                store: wgpu::StoreOp::Store,
            },
            ..attachment
        };

        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Off-screen Render Pass"),
            color_attachments: &[Some(attachment)],
            depth_stencil_attachment: None,
            occlusion_query_set: None,
            timestamp_writes: None,
            multiview_mask: None,
        });

        let md = self.model.as_ref().unwrap();
        let mut nodes = md.driver.draw_nodes(part).unwrap().to_vec();

        let mut first = true;

        loop {
            let (pop_nodes, pop_offscreen) = self.draw_nodes(
                &mut render_pass,
                buffer.texture.format(),
                &nodes[..],
                &offscreen_children,
                first,
            );
            first = false;
            nodes.drain(..pop_nodes);
            self.buffer_pool
                .extend(offscreen_children.drain(..pop_offscreen));

            if nodes.is_empty() {
                break;
            }

            core::mem::drop(render_pass);

            if self.shadow_fb.is_none() {
                self.shadow_fb = Some(BufferTexture::new(
                    &self.stat.device,
                    &self.stat.mask_bind_group_layout,
                    &self.stat.mask_sampler,
                    self.mask_dimensions.x,
                    self.mask_dimensions.y,
                    wgpu::TextureFormat::Bgra8Unorm,
                    "shadow_fb",
                ));
            }
            let shadow_fb = self.shadow_fb.as_ref().unwrap();
            // TODO: Use encoder.clear_texture if available and first && pop_nodes == 0
            encoder.copy_texture_to_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &buffer.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyTextureInfo {
                    texture: &shadow_fb.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                buffer.texture.size(),
            );

            render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Off-screen Render Pass (continuation)"),
                color_attachments: &[Some(attachment_next.clone())],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
                multiview_mask: None,
            });
            render_pass.set_bind_group(3, &shadow_fb.bind_group, &[]);
        }
    }

    fn draw_nodes(
        &self,
        render_pass: &mut wgpu::RenderPass<'_>,
        surface_format: wgpu::TextureFormat,
        nodes: &[DrawNode<T::Uid>],
        offscreen_children: &[BufferTexture],
        first: bool,
    ) -> (usize, usize) {
        let md = self.model.as_ref().unwrap();
        let m = md.model.as_ref();
        render_pass.set_index_buffer(md.index_buffer.slice(..), wgpu::IndexFormat::Uint16);

        let mut child_idx = 0;
        for (i, node) in nodes.iter().enumerate() {
            let (visual, blend_config) = match node {
                DrawNode::ArtMesh(uid) => {
                    let artmesh = m.artmeshes().get(*uid).unwrap();
                    let blend_config = artmesh.blend_config();
                    let state = md.driver.artmesh_state(*uid).unwrap();
                    (state.visual.clone(), blend_config)
                }
                DrawNode::OffscreenPart(uid) => {
                    let part = m.parts().get(*uid).unwrap();
                    let blend_config = part.blend_config().unwrap();
                    let state = md.driver.part_state(*uid).unwrap();
                    (state.visual.unwrap().clone(), blend_config)
                }
            };

            if visual.opacity == 0. || !visual.visible {
                continue;
            }

            if blend_config.is_advanced() && (i > 0 || first) {
                return (i, child_idx);
            }

            match node {
                DrawNode::ArtMesh(uid) => {
                    self.draw_artmesh(render_pass, surface_format, *uid);
                }
                DrawNode::OffscreenPart(uid) => {
                    self.draw_part(
                        render_pass,
                        surface_format,
                        Some(*uid),
                        &offscreen_children[child_idx],
                    );
                    child_idx += 1;
                }
            }
        }

        assert!(child_idx == offscreen_children.len());
        (nodes.len(), child_idx)
    }

    fn draw_part(
        &self,
        render_pass: &mut wgpu::RenderPass<'_>,
        surface_format: wgpu::TextureFormat,
        uid: Option<T::Uid>,
        buffer: &BufferTexture,
    ) {
        let md = self.model.as_ref().unwrap();

        let mut clip_set = None;
        let mut uniform_offset = 0;
        let mut mode = PipelineMode {
            surface_format,
            blend_config: Default::default(),
            cull: false,
            mask: false,
            blit: true,
        };

        if let Some(uid) = uid {
            let m = md.model.as_ref();
            let part = m.parts().get(uid).unwrap();
            let part_data = md.part_data.get(&uid).unwrap();
            mode.blend_config = part.blend_config().unwrap();
            mode.mask = part_data.clip_set.is_some();
            uniform_offset = part_data.uniform_offset;
            clip_set = part_data.clip_set;
        }

        let mut cache = self.cache.borrow_mut();
        let pipeline = cache.render_pipeline(&self.stat, mode);
        render_pass.set_pipeline(pipeline);
        render_pass.set_bind_group(0, &md.uniform_bind_group, &[uniform_offset as u32]);
        render_pass.set_bind_group(1, &buffer.bind_group, &[]);

        if let Some(idx) = clip_set {
            let clip = &md.clip_sets[idx];
            let tex = clip.texture.as_ref().unwrap();
            render_pass.set_bind_group(2, &tex.bind_group, &[]);
        }
        render_pass.draw(0..4, 0..1);
    }

    fn draw_artmesh(
        &self,
        render_pass: &mut wgpu::RenderPass<'_>,
        surface_format: wgpu::TextureFormat,
        uid: T::Uid,
    ) {
        let md = self.model.as_ref().unwrap();
        let m = md.model.as_ref();

        let artmesh = m.artmeshes().get(uid).unwrap();
        let state = md.driver.artmesh_state(uid).unwrap();
        if state.visual.opacity == 0. || !state.visual.visible {
            return;
        }
        let am_data = md.artmesh_data.get(&uid).unwrap();

        let mode = PipelineMode {
            surface_format,
            blend_config: artmesh.blend_config(),
            cull: artmesh.culling(),
            mask: am_data.clip_set.is_some(),
            blit: false,
        };
        let mut cache = self.cache.borrow_mut();
        let pipeline = cache.render_pipeline(&self.stat, mode);
        render_pass.set_pipeline(pipeline);

        let tex = artmesh.texture();

        render_pass.set_bind_group(0, &md.uniform_bind_group, &[am_data.uniform_offset as u32]);
        render_pass.set_bind_group(1, &self.textures[tex as usize].bind_group, &[]);

        if let Some(idx) = am_data.clip_set {
            let clip = &md.clip_sets[idx];
            let tex = clip.texture.as_ref().unwrap();
            render_pass.set_bind_group(2, &tex.bind_group, &[]);
        }

        let texcoord_off = artmesh.texcoord_offset() as u64;
        render_pass.set_vertex_buffer(0, md.vertex_buffer.slice(8 * texcoord_off..));
        render_pass.set_vertex_buffer(1, md.texcoord_buffer.slice(8 * texcoord_off..));

        let mut vmin = Vec2::INFINITY;
        let mut vmax = Vec2::NEG_INFINITY;
        for v in state.vertices {
            vmin = vmin.min(*v);
            vmax = vmax.max(*v);
        }

        trace!(
            "Draw ArtMesh {}: {:?} -> {:?} .. {:?} {:?}",
            artmesh.id(),
            artmesh.index_range(),
            vmin,
            vmax,
            mode
        );

        render_pass.draw_indexed(artmesh.index_range(), 0, 0..1);
    }

    pub fn render(
        &self,
        render_pass: &mut wgpu::RenderPass<'_>,
        surface_format: wgpu::TextureFormat,
    ) {
        let md = self.model.as_ref().unwrap();

        if md.needs_offscreen {
            assert!(self.offscreen_top.len() == 1);
            self.draw_part(render_pass, surface_format, None, &self.offscreen_top[0]);
        } else {
            let nodes = md.driver.draw_nodes(None).unwrap().to_vec();
            let (pop_nodes, _) = self.draw_nodes(
                render_pass,
                surface_format,
                &nodes,
                &self.offscreen_top,
                true,
            );
            assert!(pop_nodes == nodes.len());
        }

        if md.update_queued.get() {
            md.update_queued.set(false);
        }

        for clip in md.clip_sets.iter() {
            if clip.update_queued.get() {
                clip.dirty.set(false);
                clip.update_queued.set(false);
            }
        }
    }
}
