//! GPU renderer for Mao-Term.
//!
//! Knows nothing about terminals or widgets. See `docs/specs/renderer.md`.

mod color;

pub use color::srgb_to_linear;

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

/// Owns the wgpu device and the window surface.
pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
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
        width: u32,
        height: u32,
        options: RendererOptions,
    ) -> Result<Self, RenderError>
    where
        W: Into<wgpu::SurfaceTarget<'static>>,
        D: wgpu::wgt::WgpuHasDisplayHandle,
    {
        let mut instance_desc =
            wgpu::InstanceDescriptor::new_with_display_handle(Box::new(display));
        instance_desc.backends = wgpu::Backends::PRIMARY | wgpu::Backends::GL;
        let instance = wgpu::Instance::new(instance_desc.with_env());

        let surface = instance.create_surface(window)?;

        let power_preference = if options.high_performance {
            wgpu::PowerPreference::HighPerformance
        } else {
            wgpu::PowerPreference::LowPower
        };
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference,
            force_fallback_adapter: false,
            compatible_surface: Some(&surface),
            ..Default::default()
        }))?;
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
            .get_default_config(&adapter, width.max(1), height.max(1))
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

        Ok(Self {
            surface,
            device,
            queue,
            config,
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

    /// Renders one frame cleared to `background` (sRGB).
    pub fn render(&mut self, background: [u8; 3]) -> Result<FrameOutcome, RenderError> {
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

        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let clear = self.clear_color(background);

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame"),
            });
        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("clear"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
        }
        self.queue.submit(std::iter::once(encoder.finish()));
        self.queue.present(frame);
        Ok(FrameOutcome::Presented)
    }

    fn clear_color(&self, [r, g, b]: [u8; 3]) -> wgpu::Color {
        // Clear values are written as-is into the target. sRGB targets expect
        // linear values (the hardware encodes them), others expect sRGB values.
        let convert = |c: u8| {
            let v = f64::from(c) / 255.0;
            if self.config.format.is_srgb() {
                srgb_to_linear(v)
            } else {
                v
            }
        };
        wgpu::Color {
            r: convert(r),
            g: convert(g),
            b: convert(b),
            a: 1.0,
        }
    }
}
