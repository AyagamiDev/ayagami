// Based on learn-wgpu tutorial5
//
// https://github.com/sotrh/learn-wgpu/tree/master/code/beginner/tutorial5-textures
// License: MIT

use crate::renderer::RendererError;
use anyhow::*;
use image::{GenericImageView, ImageReader, Pixel};
use log::{error, info};
use std::{io::Cursor, iter};

pub struct Texture {
    #[allow(unused)]
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub sampler: wgpu::Sampler,
}

impl Texture {
    pub fn from_bytes(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bytes: &[u8],
        label: &str,
    ) -> Result<Self> {
        info!("Decoding image {}", label);

        // Use bigger than default limits to support 16K textures
        let mut limits = image::Limits::no_limits();
        limits.max_image_width = Some(16384);
        limits.max_image_height = Some(16384);
        limits.max_alloc = Some(16384 * 16384 * 12);

        let mut reader = ImageReader::new(Cursor::new(bytes));
        reader.limits(limits);
        let mut reader = reader.with_guessed_format()?;
        let img = reader.decode()?;

        Self::from_image(device, queue, &img, Some(label)).with_context(|| {
            format!(
                "Failed to load texture {} ({}x{})",
                label,
                img.width(),
                img.height(),
            )
        })
    }

    pub fn from_image(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        img: &image::DynamicImage,
        label: Option<&str>,
    ) -> Result<Self> {
        info!(
            "Loading texture {:?} ({}x{})",
            label,
            img.width(),
            img.height()
        );

        let max_dim = device.limits().max_texture_dimension_2d;
        if img.width().max(img.height()) > max_dim {
            Err(RendererError::TextureTooLarge(
                label.unwrap_or("<unnamed>").to_string(),
                img.width(),
                img.height(),
                max_dim,
            ))?;
        }

        info!("{:?}: Converting to RGBA8", label);
        let mut rgba = img.to_rgba8();

        info!("{:?}: Loading into GPU", label);
        let dimensions = img.dimensions();

        let mip_level_count = img.width().min(img.height()).ilog2() + 1;

        let size = wgpu::Extent3d {
            width: dimensions.0,
            height: dimensions.1,
            depth_or_array_layers: 1,
        };
        let format = wgpu::TextureFormat::Rgba8UnormSrgb;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label,
            size,
            mip_level_count,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });

        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                aspect: wgpu::TextureAspect::All,
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
            },
            &rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * dimensions.0),
                rows_per_image: Some(dimensions.1),
            },
            size,
        );

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });

        info!("{:?}: Loaded", label);

        Ok(Self {
            texture,
            view,
            sampler,
        })
    }
}

pub struct TextureManager {
    shader: wgpu::ShaderModule,
}

impl TextureManager {
    pub fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(include_str!("flat.wgsl").into()),
        });

        Self { shader }
    }

    pub fn premultiply(&self, device: &wgpu::Device, queue: &wgpu::Queue, texture: &Texture) {
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("blit"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &self.shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &self.shader,
                entry_point: Some("fs_flat"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: texture.texture.format(),
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::Zero,
                            dst_factor: wgpu::BlendFactor::DstAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::Zero,
                            dst_factor: wgpu::BlendFactor::One,
                            operation: wgpu::BlendOperation::Add,
                        },
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        let view = texture.texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some("premultiply"),
            format: None,
            dimension: None,
            usage: None,
            aspect: wgpu::TextureAspect::All,
            base_mip_level: 0,
            mip_level_count: Some(1),
            base_array_layer: 0,
            array_layer_count: None,
        });

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Premultiply Encoder"),
        });

        {
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
                multiview_mask: None,
            });
            rpass.set_pipeline(&pipeline);
            rpass.draw(0..4, 0..1);
        }
        queue.submit(iter::once(encoder.finish()));
    }

    pub fn gen_mips(&self, device: &wgpu::Device, queue: &wgpu::Queue, texture: &Texture) {
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Generate mips Encoder"),
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("mip"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        let views = (0..texture.texture.mip_level_count())
            .map(|mip| {
                texture.texture.create_view(&wgpu::TextureViewDescriptor {
                    label: Some("mip"),
                    format: None,
                    dimension: None,
                    usage: None,
                    aspect: wgpu::TextureAspect::All,
                    base_mip_level: mip,
                    mip_level_count: Some(1),
                    base_array_layer: 0,
                    array_layer_count: None,
                })
            })
            .collect::<Vec<_>>();

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("mip"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &self.shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &self.shader,
                entry_point: Some("fs_blit"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: texture.texture.format(),
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        let bind_group_layout = pipeline.get_bind_group_layout(0);

        for target_mip in 1..texture.texture.mip_level_count() as usize {
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                layout: &bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&views[target_mip - 1]),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&sampler),
                    },
                ],
                label: None,
            });

            {
                let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: None,
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &views[target_mip],
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(Default::default()),
                            store: wgpu::StoreOp::Store,
                        },
                        depth_slice: None,
                    })],
                    depth_stencil_attachment: None,
                    occlusion_query_set: None,
                    timestamp_writes: None,
                    multiview_mask: None,
                });
                rpass.set_pipeline(&pipeline);
                rpass.set_bind_group(0, &bind_group, &[]);
                rpass.draw(0..4, 0..1);
            }
        }
        queue.submit(iter::once(encoder.finish()));
    }
}
