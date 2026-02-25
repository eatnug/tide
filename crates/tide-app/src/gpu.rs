use std::sync::Arc;

use tide_platform::PlatformWindow;
use tide_renderer::WgpuRenderer;

use crate::App;

impl App {
    pub(crate) fn init_gpu(&mut self, window: &dyn PlatformWindow) {
        self.scale_factor = window.scale_factor() as f32;
        self.window_size = window.inner_size();

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

        // Create surface using raw window handle (unsafe: we know the window outlives the surface)
        let surface = unsafe {
            let raw_handle = window.window_handle().expect("window handle");
            let raw_display = window.display_handle().expect("display handle");
            let target = wgpu::SurfaceTargetUnsafe::RawHandle {
                raw_display_handle: raw_display.into(),
                raw_window_handle: raw_handle.into(),
            };
            instance.create_surface_unsafe(target).expect("create surface")
        };

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .expect("no suitable GPU adapter found");

        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("tide_device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                memory_hints: Default::default(),
            },
            None,
        ))
        .expect("failed to create device");

        let device = Arc::new(device);
        let queue = Arc::new(queue);

        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .find(|f| !f.is_srgb())
            .copied()
            .unwrap_or(caps.formats[0]);

        // Prefer Mailbox (low latency, no tearing) > Fifo (vsync fallback)
        let present_mode = if caps.present_modes.contains(&wgpu::PresentMode::Mailbox) {
            wgpu::PresentMode::Mailbox
        } else {
            wgpu::PresentMode::Fifo
        };

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: self.window_size.0,
            height: self.window_size.1,
            present_mode,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 1,
        };
        surface.configure(&device, &config);

        let mut renderer = WgpuRenderer::new(
            Arc::clone(&device),
            Arc::clone(&queue),
            format,
            self.scale_factor,
        );

        // Set initial clear color from theme palette
        renderer.clear_color = self.palette().border_color;

        // Pre-warm ASCII + Korean Jamo glyphs before first frame to avoid input latency
        renderer.warmup_ascii();
        renderer.warmup_common_unicode();

        self.surface = Some(surface);
        self.device = Some(device);
        self.queue = Some(queue);
        self.surface_config = Some(config);
        self.renderer = Some(renderer);
    }

    pub(crate) fn reconfigure_surface(&mut self) {
        if let (Some(surface), Some(device), Some(config)) = (
            self.surface.as_ref(),
            self.device.as_ref(),
            self.surface_config.as_mut(),
        ) {
            config.width = self.window_size.0.max(1);
            config.height = self.window_size.1.max(1);
            surface.configure(device, config);
        }
    }
}
