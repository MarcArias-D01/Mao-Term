//! GPU renderer for Mao-Term.
//!
//! Knows nothing about terminals: it draws a grid of already-resolved cells.
//! See `docs/specs/renderer.md`.

mod color;
mod grid;
mod quad;

use glyphon::{
    Cache, FontSystem, Resolution, SwashCache, TextAtlas, TextBounds, TextRenderer, Viewport,
};

pub use color::srgb_to_linear;
pub use grid::{CellMetrics, CellStyle, Cursor, CursorShape, FontLoader, GridCell, Rgb8};

use grid::{Font, Grid};
use quad::QuadPipeline;

/// Errors raised while creating or using the renderer.
#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("failed to create the GPU surface: {0}")]
    CreateSurface(#[from] wgpu::CreateSurfaceError),
    #[error("no compatible GPU adapter found: {0}")]
    RequestAdapter(#[from] wgpu::RequestAdapterError),
    #[error("failed to open the GPU device: {0}")]
    RequestDevice(#[from] wgpu::RequestDeviceError),
    #[error("the GPU surface is not supported by the selected adapter")]
    UnsupportedSurface,
    #[error("the GPU surface was lost")]
    SurfaceLost,
}

/// What happened when a frame was requested.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameOutcome {
    /// The frame was presented.
    Presented,
    /// The frame was skipped (window occluded, zero-sized, timeout, reconfigured);
    /// the caller should simply request another frame when something changes.
    Skipped,
}

/// Options used when creating the [`Renderer`].
#[derive(Debug, Clone, Copy)]
pub struct RendererOptions {
    /// Prefer the integrated/low-power GPU (default) or the high-performance one.
    pub high_performance: bool,
    /// Wait for vsync (`Fifo`). Disabling it uses `Mailbox` when available.
    pub vsync: bool,
}

impl Default for RendererOptions {
    fn default() -> Self {
        Self {
            high_performance: false,
            vsync: true,
        }
    }
}

/// Font to use for the grid.
#[derive(Debug, Clone)]
pub struct FontSpec {
    pub family: String,
    /// Size in points.
    pub size: f32,
    /// Display scale factor (1.0 = 96 DPI).
    pub scale: f32,
}

struct Text {
    font_system: FontSystem,
    swash: SwashCache,
    atlas: TextAtlas,
    viewport: Viewport,
    renderer: TextRenderer,
}

/// Owns the wgpu device, the window surface and all render state.
pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    srgb: bool,
    quads: QuadPipeline,
    text: Text,
    font: Font,
    grid: Grid,
    /// Quad instances for the current frame (reused).
    instances: Vec<f32>,
}

impl Renderer {
    /// Creates a renderer for `window` (typically an `Arc<winit::window::Window>`,
    /// which the surface keeps alive).
    ///
    /// `display` is the platform display connection (e.g. winit's `OwnedDisplayHandle`);
    /// it is required for GL on Wayland and harmless elsewhere.
    pub fn new<W, D>(
        window: W,
        display: D,
        size: (u32, u32),
        fonts: FontLoader,
        font: &FontSpec,
        options: RendererOptions,
    ) -> Result<Self, RenderError>
    where
        W: Into<wgpu::SurfaceTarget<'static>> + Clone,
        D: wgpu::wgt::WgpuHasDisplayHandle + Clone,
    {
        let started = std::time::Instant::now();
        let power_preference = if options.high_performance {
            wgpu::PowerPreference::HighPerformance
        } else {
            wgpu::PowerPreference::LowPower
        };

        // Try the preferred backend first, then let wgpu pick among all of them.
        // `WGPU_BACKEND` (applied by `with_env`) overrides both attempts.
        let attempts = preferred_backends();
        let mut attempt = 0;
        let (surface, adapter) = loop {
            let backends = attempts[attempt];
            let mut desc =
                wgpu::InstanceDescriptor::new_with_display_handle(Box::new(display.clone()));
            desc.backends = backends;
            let instance = wgpu::Instance::new(desc.with_env());
            let surface = instance.create_surface(window.clone())?;
            let request = instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference,
                force_fallback_adapter: false,
                compatible_surface: Some(&surface),
                ..Default::default()
            });
            match pollster::block_on(request) {
                Ok(adapter) => break (surface, adapter),
                Err(e) if attempt + 1 < attempts.len() => {
                    tracing::warn!(
                        ?backends,
                        "no adapter for preferred backend ({e}), trying others"
                    );
                    attempt += 1;
                }
                Err(e) => return Err(e.into()),
            }
        };
        let info = adapter.get_info();
        tracing::info!(
            name = %info.name,
            backend = ?info.backend,
            device_type = ?info.device_type,
            "selected GPU adapter"
        );

        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("mao-device"),
                required_limits:
                    wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter.limits()),
                memory_hints: wgpu::MemoryHints::MemoryUsage,
                ..Default::default()
            }))?;

        let mut config = surface
            .get_default_config(&adapter, size.0.max(1), size.1.max(1))
            .ok_or(RenderError::UnsupportedSurface)?;
        let caps = surface.get_capabilities(&adapter);
        config.present_mode =
            if !options.vsync && caps.present_modes.contains(&wgpu::PresentMode::Mailbox) {
                wgpu::PresentMode::Mailbox
            } else {
                wgpu::PresentMode::Fifo
            };
        // Keep latency low: at most one frame queued ahead.
        config.desired_maximum_frame_latency = 1;
        tracing::debug!(format = ?config.format, present_mode = ?config.present_mode, "surface configured");
        surface.configure(&device, &config);
        tracing::debug!(
            ms = started.elapsed().as_millis(),
            "startup: GPU device ready"
        );

        let quads = QuadPipeline::new(&device, config.format);

        let t = std::time::Instant::now();
        let mut font_system = fonts.finish();
        tracing::debug!(
            waited_ms = t.elapsed().as_millis(),
            "startup: font system ready"
        );
        let cache = Cache::new(&device);
        let mut atlas = TextAtlas::new(&device, &queue, &cache, config.format);
        let viewport = Viewport::new(&device, &cache);
        let text_renderer =
            TextRenderer::new(&mut atlas, &device, wgpu::MultisampleState::default(), None);
        let font = Font::new(&mut font_system, &font.family, font.size, font.scale);
        tracing::debug!(ms = t.elapsed().as_millis(), "startup: text pipeline ready");

        Ok(Self {
            srgb: config.format.is_srgb(),
            surface,
            device,
            queue,
            config,
            quads,
            text: Text {
                font_system,
                swash: SwashCache::new(),
                atlas,
                viewport,
                renderer: text_renderer,
            },
            font,
            grid: Grid::new(),
            instances: Vec::new(),
        })
    }

    /// Current surface size in physical pixels.
    pub fn size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    /// Resizes the surface. Zero sizes (minimized window) are ignored.
    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 || (width, height) == self.size() {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
    }

    pub fn cell_metrics(&self) -> CellMetrics {
        self.font.cell
    }

    pub fn font_size(&self) -> f32 {
        self.font.size_pt()
    }

    /// Changes font size (points) and/or display scale. Returns the new cell metrics.
    pub fn set_font_size(&mut self, size_pt: f32, scale: f32) -> CellMetrics {
        self.font
            .set_size(&mut self.text.font_system, size_pt, scale);
        self.grid.set_font(&self.font);
        self.font.cell
    }

    /// Places the grid at `origin` (pixels) with the given size in cells.
    pub fn set_grid_layout(&mut self, origin: (f32, f32), cols: usize, rows: usize) {
        if origin != self.grid.origin() {
            self.grid.mark_all_dirty();
        }
        self.grid.set_layout(&self.font, origin, cols, rows);
    }

    /// Replaces the contents of one grid row.
    pub fn set_row(&mut self, row: usize, cells: impl IntoIterator<Item = GridCell>) {
        self.grid.set_row(row, cells);
    }

    pub fn set_cursor(&mut self, cursor: Option<Cursor>) {
        self.grid.set_cursor(cursor);
    }

    /// Renders one frame on top of `background` (sRGB).
    pub fn render(&mut self, background: Rgb8) -> Result<FrameOutcome, RenderError> {
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) => frame,
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
                // Present this one, reconfigure for the next.
                self.surface.configure(&self.device, &self.config);
                frame
            }
            wgpu::CurrentSurfaceTexture::Outdated => {
                self.surface.configure(&self.device, &self.config);
                return Ok(FrameOutcome::Skipped);
            }
            wgpu::CurrentSurfaceTexture::Timeout
            | wgpu::CurrentSurfaceTexture::Occluded
            | wgpu::CurrentSurfaceTexture::Validation => return Ok(FrameOutcome::Skipped),
            wgpu::CurrentSurfaceTexture::Lost => return Err(RenderError::SurfaceLost),
        };

        let (width, height) = self.size();
        let Text {
            font_system,
            swash,
            atlas,
            viewport,
            renderer,
        } = &mut self.text;

        // Grid: reshape dirty rows, gather quads and text.
        self.grid
            .update(font_system, &self.font, background, self.srgb);
        self.instances.clear();
        self.grid
            .collect_quads(&self.font, self.srgb, &mut self.instances);
        self.quads
            .prepare(&self.device, &self.queue, (width, height), &self.instances);

        viewport.update(&self.queue, Resolution { width, height });
        let bounds = TextBounds {
            left: 0,
            top: 0,
            right: width as i32,
            bottom: height as i32,
        };
        if let Err(e) = renderer.prepare(
            &self.device,
            &self.queue,
            font_system,
            atlas,
            viewport,
            self.grid.text_areas(&self.font, bounds),
            swash,
        ) {
            tracing::error!("text prepare failed: {e}");
        }

        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let [r, g, b, _] = color::to_target(background, self.srgb);
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("main"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: f64::from(r),
                            g: f64::from(g),
                            b: f64::from(b),
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            self.quads.draw(&mut pass);
            if let Err(e) = renderer.render(atlas, viewport, &mut pass) {
                tracing::error!("text render failed: {e}");
            }
        }
        self.queue.submit(std::iter::once(encoder.finish()));
        self.queue.present(frame);
        atlas.trim();
        Ok(FrameOutcome::Presented)
    }
}

/// Backends to try, in order. On Windows DX12 initializes noticeably faster
/// than Vulkan (~120 ms vs ~170 ms on an Intel iGPU), which dominates startup.
fn preferred_backends() -> &'static [wgpu::Backends] {
    const ALL: wgpu::Backends = wgpu::Backends::PRIMARY.union(wgpu::Backends::GL);
    if cfg!(windows) {
        &[wgpu::Backends::DX12, ALL]
    } else {
        &[ALL]
    }
}
