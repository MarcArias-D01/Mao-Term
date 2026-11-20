//! Textured rectangles: images shown by the document viewer.

use std::collections::HashMap;

/// Identifies an uploaded image.
pub type ImageId = u64;

/// One level of an image's mipmap chain (RGBA8, straight alpha, sRGB).
pub struct ImageData<'a> {
    pub width: u32,
    pub height: u32,
    pub rgba: &'a [u8],
}

/// Draws image `id` stretched to `dest` (`[x, y, w, h]` in pixels), clipped to `clip`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ImageDraw {
    pub id: ImageId,
    pub dest: [f32; 4],
    pub clip: [f32; 4],
}

/// Floats per instance: dest rect.
const FLOATS: usize = 4;

struct Texture {
    bind_group: wgpu::BindGroup,
}

pub(crate) struct ImagePipeline {
    pipeline: wgpu::RenderPipeline,
    globals: wgpu::Buffer,
    globals_group: wgpu::BindGroup,
    texture_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    format: wgpu::TextureFormat,
    instances: wgpu::Buffer,
    capacity: usize,
    textures: HashMap<ImageId, Texture>,
}

impl ImagePipeline {
    pub(crate) fn new(device: &wgpu::Device, target: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("image shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("image.wgsl").into()),
        });
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("image globals layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("image texture layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
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
        });
        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("image globals"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let globals_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("image globals"),
            layout: &globals_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals.as_entire_binding(),
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("image pipeline layout"),
            bind_group_layouts: &[Some(&globals_layout), Some(&texture_layout)],
            immediate_size: 0,
        });
        let attributes = wgpu::vertex_attr_array![0 => Float32x4];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("image pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: (FLOATS * 4) as wgpu::BufferAddress,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &attributes,
                })],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("image sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        // Sampling an sRGB texture yields linear values, which an sRGB target
        // encodes back; a non-sRGB target gets the bytes unchanged.
        let format = if target.is_srgb() {
            wgpu::TextureFormat::Rgba8UnormSrgb
        } else {
            wgpu::TextureFormat::Rgba8Unorm
        };
        let capacity = 4;
        Self {
            pipeline,
            globals,
            globals_group,
            texture_layout,
            sampler,
            format,
            instances: instance_buffer(device, capacity),
            capacity,
            textures: HashMap::new(),
        }
    }

    pub(crate) fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        id: ImageId,
        levels: &[ImageData<'_>],
    ) {
        let Some(base) = levels.first() else {
            return;
        };
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("image"),
            size: wgpu::Extent3d {
                width: base.width,
                height: base.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: levels.len() as u32,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        for (mip, level) in levels.iter().enumerate() {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: mip as u32,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                level.rgba,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(4 * level.width),
                    rows_per_image: Some(level.height),
                },
                wgpu::Extent3d {
                    width: level.width,
                    height: level.height,
                    depth_or_array_layers: 1,
                },
            );
        }
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("image texture"),
            layout: &self.texture_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        self.textures.insert(id, Texture { bind_group });
    }

    pub(crate) fn remove(&mut self, id: ImageId) {
        self.textures.remove(&id);
    }

    /// Uploads this frame's draw rectangles.
    pub(crate) fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        screen: (u32, u32),
        draws: &[ImageDraw],
    ) {
        if draws.is_empty() {
            return;
        }
        if draws.len() > self.capacity {
            self.capacity = draws.len().next_power_of_two();
            self.instances = instance_buffer(device, self.capacity);
        }
        let data: Vec<f32> = draws.iter().flat_map(|d| d.dest).collect();
        queue.write_buffer(&self.instances, 0, bytemuck::cast_slice(&data));
        let globals = [screen.0 as f32, screen.1 as f32, 0.0, 0.0];
        queue.write_buffer(&self.globals, 0, bytemuck::cast_slice(&globals));
    }

    pub(crate) fn draw(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        screen: (u32, u32),
        draws: &[ImageDraw],
    ) {
        if draws.is_empty() {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.globals_group, &[]);
        pass.set_vertex_buffer(0, self.instances.slice(..));
        for (i, d) in draws.iter().enumerate() {
            let Some(texture) = self.textures.get(&d.id) else {
                continue;
            };
            let Some([x, y, w, h]) = scissor(d.clip, screen) else {
                continue;
            };
            pass.set_scissor_rect(x, y, w, h);
            pass.set_bind_group(1, &texture.bind_group, &[]);
            pass.draw(0..4, i as u32..i as u32 + 1);
        }
        pass.set_scissor_rect(0, 0, screen.0, screen.1);
    }
}

/// Clip rectangle in whole pixels inside the screen, or `None` if empty.
fn scissor(clip: [f32; 4], screen: (u32, u32)) -> Option<[u32; 4]> {
    let x0 = clip[0].max(0.0).round() as u32;
    let y0 = clip[1].max(0.0).round() as u32;
    let x1 = ((clip[0] + clip[2]).round().max(0.0) as u32).min(screen.0);
    let y1 = ((clip[1] + clip[3]).round().max(0.0) as u32).min(screen.1);
    (x1 > x0 && y1 > y0).then(|| [x0, y0, x1 - x0, y1 - y0])
}

fn instance_buffer(device: &wgpu::Device, count: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("image instances"),
        size: (count * FLOATS * 4) as wgpu::BufferAddress,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scissor_is_clamped_to_screen() {
        assert_eq!(
            scissor([-5.0, 10.0, 50.0, 20.0], (30, 100)),
            Some([0, 10, 30, 20])
        );
        assert_eq!(scissor([40.0, 0.0, 10.0, 10.0], (30, 100)), None);
    }
}
