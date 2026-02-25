// Dedicated render thread: handles GPU drawable acquisition, command
// encoding, submission, and presentation.  The main thread builds all
// vertex data (which is fast, ~1ms) and sends the renderer here.
// This thread may block on CAMetalLayer.nextDrawable() without
// stalling the event loop.

use std::sync::mpsc;
use std::sync::Arc;
use std::time::Instant;

use tide_platform::WakeCallback;
use tide_renderer::WgpuRenderer;

pub(crate) struct RenderJob {
    pub renderer: WgpuRenderer,
    /// If set, reconfigure the surface before rendering.
    pub config_update: Option<wgpu::SurfaceConfiguration>,
}

pub(crate) struct RenderResult {
    pub renderer: WgpuRenderer,
    /// Microseconds spent waiting for a drawable (nextDrawable).
    pub drawable_wait_us: u64,
    /// True if the surface was lost/outdated and needs reconfiguration.
    pub surface_lost: bool,
}

pub(crate) struct RenderThreadHandle {
    pub job_tx: mpsc::Sender<RenderJob>,
    pub result_rx: mpsc::Receiver<RenderResult>,
    _handle: std::thread::JoinHandle<()>,
}

impl RenderThreadHandle {
    pub fn spawn(
        surface: wgpu::Surface<'static>,
        device: Arc<wgpu::Device>,
        queue: Arc<wgpu::Queue>,
        initial_config: wgpu::SurfaceConfiguration,
        waker: WakeCallback,
    ) -> Self {
        let (job_tx, job_rx) = mpsc::channel::<RenderJob>();
        let (result_tx, result_rx) = mpsc::channel::<RenderResult>();

        let handle = std::thread::Builder::new()
            .name("render".to_string())
            .spawn(move || {
                run(surface, device, queue, initial_config, job_rx, result_tx, waker);
            })
            .expect("failed to spawn render thread");

        Self {
            job_tx,
            result_rx,
            _handle: handle,
        }
    }
}

fn run(
    surface: wgpu::Surface<'static>,
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    mut config: wgpu::SurfaceConfiguration,
    job_rx: mpsc::Receiver<RenderJob>,
    result_tx: mpsc::Sender<RenderResult>,
    waker: WakeCallback,
) {
    loop {
        let job = match job_rx.recv() {
            Ok(j) => j,
            Err(_) => break, // Main thread dropped the sender — exit
        };

        // Apply surface reconfiguration if requested
        if let Some(new_config) = job.config_update {
            config = new_config;
            surface.configure(&device, &config);
        }

        let t0 = Instant::now();

        let output = match surface.get_current_texture() {
            Ok(t) => t,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                // Reconfigure and skip this frame
                surface.configure(&device, &config);
                let _ = result_tx.send(RenderResult {
                    renderer: job.renderer,
                    drawable_wait_us: 0,
                    surface_lost: true,
                });
                waker();
                continue;
            }
            Err(e) => {
                log::error!("Surface error on render thread: {}", e);
                let _ = result_tx.send(RenderResult {
                    renderer: job.renderer,
                    drawable_wait_us: 0,
                    surface_lost: false,
                });
                waker();
                continue;
            }
        };

        let drawable_wait_us = t0.elapsed().as_micros() as u64;

        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("render_encoder"),
            });

        let mut renderer = job.renderer;
        renderer.render_frame(&mut encoder, &view);

        queue.submit(std::iter::once(encoder.finish()));
        output.present();

        // Reclaim completed GPU staging buffers to prevent memory accumulation.
        device.poll(wgpu::Maintain::Poll);

        let _ = result_tx.send(RenderResult {
            renderer,
            drawable_wait_us,
            surface_lost: false,
        });
        waker();
    }
}
