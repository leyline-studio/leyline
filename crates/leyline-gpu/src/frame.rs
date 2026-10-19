//! A frame resident on the GPU, and the operations a preview runs on it
//! (ADR 0156 §4–§5).
//!
//! The buffer the CPU hands over is the stage cache's resume point: at 4K it
//! is ~134 MB of `f32`, so it is uploaded once and kept, and each frame of a
//! drag starts from a GPU-side copy of it. The result comes back as 8-bit
//! RGB, the form every preview already has.

use std::sync::{Arc, OnceLock};

use wgpu::util::DeviceExt;

use crate::Gpu;

mod dehaze;
use dehaze::{DehazeParams, DehazePipelines};

/// Invocations per workgroup, along x.
const WORKGROUP: u32 = 64;
/// The most workgroups one dispatch dimension may hold (WebGPU's floor).
const MAX_GROUPS: u32 = 65_535;

/// One step of a preview on the GPU, with everything it needs already
/// computed on the CPU — the engine builds these from a stage version and
/// its settings, so the GPU never decides a value the CPU would not.
#[derive(Debug, Clone)]
pub enum Op {
    /// Every sample times its channel's gain, floored at 0: white balance
    /// and exposure (`gains` v1).
    Gains([f32; 3]),
    /// A tone operator on the display axis.
    Display(DisplayOp, Transfer),
    /// `crop` v1: the frame from here on is this rectangle, given as
    /// fractions of the frame and rounded exactly as the CPU rounds them.
    /// Applied when the output is read, so it may only be followed by
    /// [`Op::Vignette`] and [`Op::OutputRendering`].
    Crop {
        /// Left edge, as a fraction of the width.
        x: f64,
        /// Top edge, as a fraction of the height.
        y: f64,
        /// Width, as a fraction of the width.
        width: f64,
        /// Height, as a fraction of the height.
        height: f64,
    },
    /// `sharpen` v1 and v2: the luma of the display-axis frame, minus its
    /// Gaussian blur, times `k` — and, with `edge`, times v2's blurred
    /// Sobel mask — added to all three channels on the display axis. A
    /// neighbourhood operator, so it runs on the whole frame like any
    /// kernel before a crop.
    Sharpen {
        /// Amount / 100.
        k: f32,
        /// The CPU's half kernel at the stage's σ, centre first.
        weights: Arc<Vec<f32>>,
        /// v2's mask threshold and softness, when masking is above 0.
        edge: Option<[f32; 2]>,
        /// The CPU's transfer tables.
        transfer: Transfer,
    },
    /// `clarity` v1 and `texture` v1, the CPU's `local_contrast`: the luma
    /// shift of [`Op::Sharpen`] without a mask, around the blur of the
    /// CPU's `approx_blur` — a `factor`×`factor` box reduction, a Gaussian
    /// at the reduced scale and a bilinear return — or a literal Gaussian
    /// when `factor` is 1.
    LocalContrast {
        /// Amount / 100.
        k: f32,
        /// How much smaller a side of the blurred plane is; 1 blurs it at
        /// full size.
        factor: u32,
        /// The CPU's half kernel at the blur's σ (the reduced one when
        /// `factor` is above 1), centre first.
        weights: Arc<Vec<f32>>,
        /// The CPU's transfer tables.
        transfer: Transfer,
    },
    /// `dehaze` v1, the CPU's dark channel prior on the display axis: two
    /// minimum filters, the atmospheric light between them, and the
    /// recovery (or, below 0, the added haze). The atmospheric light is
    /// the brightest of the `take` darkest-channel maxima, chosen on the
    /// GPU; where the CPU's `select_nth_unstable` keeps only some of the
    /// pixels tied at the cut, the GPU keeps them all — the one place the
    /// two may pick a different light (ADR 0156 §3 holds it to a view).
    Dehaze {
        /// Amount / 100.
        k: f32,
        /// The minimum filters' radius, already scaled.
        patch: u32,
        /// The share of the frame, as dark-channel maxima, the atmospheric
        /// light is chosen among (`DEHAZE_TOP_FRACTION`).
        top_fraction: f32,
        /// The share of the haze removed, `DEHAZE_OMEGA`.
        omega: f32,
        /// The transmission's floor, `DEHAZE_MIN_TRANSMISSION`.
        min_transmission: f32,
        /// The CPU's transfer tables.
        transfer: Transfer,
    },
    /// `vignette` v1, in the coordinates of the (cropped) frame: applied as
    /// the output reads each pixel, so it may only be followed by
    /// [`Op::OutputRendering`]. Values as the stage derives them.
    Vignette {
        /// Amount / 100.
        amount: f32,
        /// Midpoint / 100, clamped to [0, 1].
        midpoint: f32,
        /// The superellipse exponent.
        exponent: f32,
        /// `2^(1/exponent)`.
        corner: f32,
        /// The feather's width, at least 1e-4.
        width: f32,
    },
    /// The output rendering (`output_rendering` v1): the highlight rolloff
    /// at `knee`, the working space to linear sRGB through `matrix`, the
    /// clamp, and the display curve through `display_table` — the very
    /// table the CPU looks up, `LUT_SIZE + 1` entries over `[0, 1]`.
    OutputRendering {
        /// Where the highlight rolloff starts, in linear light.
        knee: f32,
        /// Working space (Rec. 2020) to linear sRGB, row by row.
        matrix: [[f32; 3]; 3],
        /// The CPU's display-curve table, entry `i` at `i / (len − 1)`.
        display_table: Arc<Vec<f32>>,
    },
}

/// A tone operator on the display axis: the CPU's `in_display` around a
/// per-sample function, with the CPU's own transfer tables (ADR 0156 §5).
#[derive(Debug, Clone)]
pub enum DisplayOp {
    /// `contrast` v1, at `k` = amount / 100.
    Contrast {
        /// The amount, in [-1, 1].
        k: f32,
    },
    /// `whites_blacks` v1: `(x − black) · scale`, floored at 0.
    WhitesBlacks {
        /// The display value that becomes black.
        black: f32,
        /// `1 / (white − black)`.
        scale: f32,
    },
    /// `highlights_shadows` v1, each at amount / 100.
    HighlightsShadows {
        /// The highlights amount, in [-1, 1].
        highlights: f32,
        /// The shadows amount, in [-1, 1].
        shadows: f32,
    },
    /// `levels` v1, the four limits already divided by 255.
    Levels {
        /// Input black.
        in_black: f32,
        /// Input white minus input black.
        in_range: f32,
        /// Output black.
        out_black: f32,
        /// Output white minus output black.
        out_range: f32,
        /// `1 / gamma`.
        exponent: f32,
    },
    /// A curve the CPU already tabulated (`tone_curve` v1, the parametric
    /// curve): looked up exactly as the CPU looks it up.
    Curve(Arc<Vec<f32>>),
    /// `saturation` v1 (`vibrance` false) and `vibrance` v1 (true), at
    /// `k` = amount / 100, headroom preserved as the CPU's
    /// `preserving_headroom` does.
    Saturate {
        /// The amount, in [-1, 1].
        k: f32,
        /// Weigh by how unsaturated a pixel already is.
        vibrance: bool,
    },
    /// `hsl` v1: the eight bands' hue, saturation and luminance amounts,
    /// as the settings hold them (in [-100, 100]), band order of
    /// `HSL_BAND_CENTERS_DEG`.
    Hsl {
        /// Per band: hue, saturation, luminance.
        bands: [[f32; 3]; 8],
    },
    /// `monochrome` v1: every channel set to the Rec. 2020 luma, on the
    /// **linear** axis — the one mode that does not cross to display.
    Monochrome,
    /// `color_grading` v1, its values computed by the CPU: the three zone
    /// tints (shadows, midtones, highlights), their luminance shifts, and
    /// the balance and blending, each as the stage scales them.
    ColorGrading {
        /// Per zone, the tint already multiplied by its saturation.
        tints: [[f32; 3]; 3],
        /// Per zone, the luminance shift in [-1, 1].
        luminance: [f32; 3],
        /// The balance, in [-1, 1].
        balance: f32,
        /// The blending, in [0, 1].
        blending: f32,
    },
}

/// The CPU's transfer tables between the linear and display axes, shared
/// by every display-axis operation.
#[derive(Debug, Clone)]
pub struct Transfer {
    /// sRGB → linear over [0, 1].
    pub to_linear: Arc<Vec<f32>>,
    /// Linear → sRGB over [0, 1].
    pub to_display: Arc<Vec<f32>>,
}

/// A frame uploaded once, rendered many times.
#[derive(Debug)]
pub struct Frame {
    width: u32,
    height: u32,
    pixels: u32,
    source: wgpu::Buffer,
    work: wgpu::Buffer,
    output: wgpu::Buffer,
    readback: wgpu::Buffer,
    /// Four single-channel planes for the neighbourhood kernels, allocated
    /// on the first frame that needs them and kept with the frame.
    planes: OnceLock<[wgpu::Buffer; 4]>,
}

impl Frame {
    /// Uploads `samples` — `width × height` RGB `f32`, the CPU's working
    /// buffer. `None` when the device refuses the allocation: the caller
    /// renders on the CPU (ADR 0156 §2).
    pub fn upload(gpu: &Gpu, width: u32, height: u32, samples: &[f32]) -> Option<Frame> {
        let pixels = width.checked_mul(height)?;
        if samples.len() != pixels as usize * 3 || pixels == 0 {
            return None;
        }
        let device = gpu.device();
        let float_bytes = u64::from(pixels) * 12;
        let source = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("frame source"),
            contents: &bytes_of_f32(samples),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        });
        let work = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("frame work"),
            size: float_bytes,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let output = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("frame output"),
            size: u64::from(pixels) * 4,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("frame readback"),
            size: u64::from(pixels) * 4,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Some(Frame {
            width,
            height,
            pixels,
            source,
            work,
            output,
            readback,
            planes: OnceLock::new(),
        })
    }

    /// The frame's size, in pixels.
    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Runs `ops` on a fresh copy of the uploaded buffer and reads the
    /// result back as its width, height and packed 8-bit RGB. `ops` must
    /// end with [`Op::OutputRendering`], the only step that produces 8-bit
    /// values; `None` otherwise, or when the device fails.
    pub fn render(&self, gpu: &Gpu, ops: &[Op]) -> Option<(u32, u32, Vec<u8>)> {
        let (last, steps) = ops.split_last()?;
        let Op::OutputRendering {
            knee,
            matrix,
            display_table,
        } = last
        else {
            return None;
        };
        let device = gpu.device();
        let (groups_x, groups_y) = dispatch(self.pixels);
        let stride = groups_x * WORKGROUP;
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        encoder.copy_buffer_to_buffer(&self.source, 0, &self.work, 0, self.source.size());

        // Uniforms live until the submission; collected so they outlive it.
        let mut uniforms = Vec::with_capacity(ops.len());
        // The window the output reads: the whole frame, unless cropped.
        let mut window = (0, 0, self.width, self.height);
        // The vignette the output applies, in the window's coordinates.
        let mut vignette = None;
        for (index, step) in steps.iter().enumerate() {
            if let Op::Vignette {
                amount,
                midpoint,
                exponent,
                corner,
                width,
            } = step
            {
                if index + 1 != steps.len() {
                    return None;
                }
                vignette = Some([*amount, *midpoint, *exponent, *corner, *width]);
                continue;
            }
            if let Op::Crop {
                x,
                y,
                width,
                height,
            } = step
            {
                // Nothing but the vignette and the output may follow a crop:
                // every kernel before it runs on the whole frame.
                let next = steps.get(index + 1);
                if !(next.is_none()
                    || index + 2 == steps.len() && matches!(next, Some(Op::Vignette { .. })))
                {
                    return None;
                }
                window = crop_window(self.width, self.height, *x, *y, *width, *height)?;
                continue;
            }
            if let Op::Sharpen {
                k,
                weights,
                edge,
                transfer,
            } = step
            {
                let params = SharpenParams {
                    groups: (groups_x, groups_y),
                    stride,
                    k: *k,
                    edge: *edge,
                    factor: 1,
                };
                self.sharpen(gpu, &mut encoder, &params, weights, transfer)?;
                continue;
            }
            if let Op::LocalContrast {
                k,
                factor,
                weights,
                transfer,
            } = step
            {
                let params = SharpenParams {
                    groups: (groups_x, groups_y),
                    stride,
                    k: *k,
                    edge: None,
                    factor: *factor,
                };
                self.sharpen(gpu, &mut encoder, &params, weights, transfer)?;
                continue;
            }
            if let Op::Dehaze {
                k,
                patch,
                top_fraction,
                omega,
                min_transmission,
                transfer,
            } = step
            {
                // Counted as the CPU counts it, in `f32`.
                let total = self.pixels as usize;
                let take = ((total as f32 * top_fraction).ceil() as usize).clamp(1, total);
                let params = DehazeParams {
                    groups: (groups_x, groups_y),
                    stride,
                    k: *k,
                    patch: *patch,
                    take: u32::try_from(take).ok()?,
                    omega: *omega,
                    min_transmission: *min_transmission,
                };
                self.dehaze(gpu, &mut encoder, &params, transfer)?;
                continue;
            }
            if let Op::Display(op, transfer) = step {
                let (mode, values, curve) = display_params(op);
                let mut params = Vec::with_capacity(48);
                for word in [mode, self.pixels, stride, curve_entries(curve.as_deref())?] {
                    params.extend_from_slice(&word.to_le_bytes());
                }
                for value in values {
                    params.extend_from_slice(&value.to_le_bytes());
                }
                params.resize(48, 0);
                uniforms.push(uniform(device, &params));
                let tables = [
                    table(device, &transfer.to_display),
                    table(device, &transfer.to_linear),
                    table(device, curve.as_deref().unwrap_or(&transfer.to_display)),
                ];
                let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("display"),
                    layout: &gpu.pipelines().display.get_bind_group_layout(0),
                    entries: &[
                        entry(0, &self.work),
                        entry(1, uniforms.last()?),
                        entry(2, &tables[0]),
                        entry(3, &tables[1]),
                        entry(4, &tables[2]),
                    ],
                });
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor::default());
                pass.set_pipeline(&gpu.pipelines().display);
                pass.set_bind_group(0, &bind_group, &[]);
                pass.dispatch_workgroups(groups_x, groups_y, 1);
                continue;
            }
            let Op::Gains(gains) = step else {
                return None;
            };
            let mut params = Vec::with_capacity(32);
            for gain in gains {
                params.extend_from_slice(&gain.to_le_bytes());
            }
            params.extend_from_slice(&self.pixels.to_le_bytes());
            params.extend_from_slice(&stride.to_le_bytes());
            params.resize(32, 0);
            uniforms.push(uniform(device, &params));
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("gains"),
                layout: &gpu.pipelines().gains.get_bind_group_layout(0),
                entries: &[entry(0, &self.work), entry(1, uniforms.last()?)],
            });
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor::default());
            pass.set_pipeline(&gpu.pipelines().gains);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups(groups_x, groups_y, 1);
        }

        let mut params = Vec::with_capacity(64);
        for row in matrix {
            for value in row {
                params.extend_from_slice(&value.to_le_bytes());
            }
            params.extend_from_slice(&0.0f32.to_le_bytes()); // a `vec4` row
        }
        let (x0, y0, out_width, out_height) = window;
        let out_pixels = out_width.checked_mul(out_height)?;
        let (out_groups_x, out_groups_y) = dispatch(out_pixels);
        let out_stride = out_groups_x * WORKGROUP;
        params.extend_from_slice(&knee.to_le_bytes());
        params.extend_from_slice(&out_pixels.to_le_bytes());
        params.extend_from_slice(&out_stride.to_le_bytes());
        let table_entries = u32::try_from(display_table.len()).ok()?;
        params.extend_from_slice(&table_entries.to_le_bytes());
        for word in [self.width, x0, y0, out_width] {
            params.extend_from_slice(&word.to_le_bytes());
        }
        params.extend_from_slice(&u32::from(vignette.is_some()).to_le_bytes());
        for value in vignette.unwrap_or_default() {
            params.extend_from_slice(&value.to_le_bytes());
        }
        params.extend_from_slice(&out_height.to_le_bytes());
        params.resize(112, 0);
        uniforms.push(uniform(device, &params));
        let table = table(device, display_table);
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("output rendering"),
            layout: &gpu.pipelines().output.get_bind_group_layout(0),
            entries: &[
                entry(0, &self.work),
                entry(1, uniforms.last()?),
                entry(2, &table),
                entry(3, &self.output),
            ],
        });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor::default());
            pass.set_pipeline(&gpu.pipelines().output);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups(out_groups_x, out_groups_y, 1);
        }
        let out_bytes = u64::from(out_pixels) * 4;
        encoder.copy_buffer_to_buffer(&self.output, 0, &self.readback, 0, out_bytes);
        gpu.queue().submit([encoder.finish()]);

        let (sender, receiver) = std::sync::mpsc::channel();
        self.readback
            .slice(..out_bytes)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = sender.send(result);
            });
        device.poll(wgpu::PollType::Wait).ok()?;
        receiver.recv().ok()?.ok()?;
        let mapped = self.readback.slice(..out_bytes).get_mapped_range();
        let mut rgb = Vec::with_capacity(out_pixels as usize * 3);
        for packed in mapped.chunks_exact(4) {
            rgb.extend_from_slice(&packed[..3]);
        }
        drop(mapped);
        self.readback.unmap();
        Some((out_width, out_height, rgb))
    }
}

impl Frame {
    /// The four single-channel planes, allocated on first use.
    fn planes(&self, device: &wgpu::Device) -> &[wgpu::Buffer; 4] {
        self.planes.get_or_init(|| {
            [0, 1, 2, 3].map(|_| {
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("plane"),
                    size: u64::from(self.pixels) * 4,
                    usage: wgpu::BufferUsages::STORAGE,
                    mapped_at_creation: false,
                })
            })
        })
    }
}

/// What one sharpen records, besides its tables.
struct SharpenParams {
    groups: (u32, u32),
    stride: u32,
    k: f32,
    edge: Option<[f32; 2]>,
    /// The blur's reduction, 1 for none.
    factor: u32,
}

impl Frame {
    /// Records `sharpen` v1/v2 or `local_contrast` into `encoder`: the luma
    /// plane, its blur (through a reduced plane when `factor` is above 1),
    /// v2's blurred edge mask when there is one, then the luma shift — the
    /// CPU's passes in the CPU's order.
    fn sharpen(
        &self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        sharpen: &SharpenParams,
        weights: &[f32],
        transfer: &Transfer,
    ) -> Option<()> {
        let device = gpu.device();
        let [luma, scratch, blurred, mask] = self.planes(device);
        let taps = u32::try_from(weights.len().checked_sub(1)?).ok()?;
        let masked = u32::from(sharpen.edge.is_some());
        let [threshold, softness] = sharpen.edge.unwrap_or_default();
        // One layout for both planes' passes: its own size first, then the
        // other plane's, which the reduction and its return read across.
        let params_of = |(pixels, stride, width, height): (u32, u32, u32, u32),
                         (other_width, other_height): (u32, u32)| {
            let mut params = Vec::with_capacity(48);
            for word in [pixels, stride, width, height, taps, masked] {
                params.extend_from_slice(&word.to_le_bytes());
            }
            for value in [sharpen.k, threshold, softness] {
                params.extend_from_slice(&value.to_le_bytes());
            }
            for word in [other_width, other_height, sharpen.factor] {
                params.extend_from_slice(&word.to_le_bytes());
            }
            uniform(device, &params)
        };
        let factor = sharpen.factor.max(1);
        let small =
            (factor > 1).then(|| (self.width.div_ceil(factor), self.height.div_ceil(factor)));
        // `approx_blur` blurs a one-pixel side literally; so does the CPU
        // fallback this answer hands it to.
        if small.is_some() && (self.width < 2 || self.height < 2) {
            return None;
        }
        let (small_width, small_height) = small.unwrap_or((self.width, self.height));
        let params = params_of(
            (self.pixels, sharpen.stride, self.width, self.height),
            (small_width, small_height),
        );
        let small_groups = dispatch(small_width * small_height);
        let small_params = params_of(
            (
                small_width * small_height,
                small_groups.0 * WORKGROUP,
                small_width,
                small_height,
            ),
            (self.width, self.height),
        );
        let to_display = table(device, &transfer.to_display);
        let to_linear = table(device, &transfer.to_linear);
        let weights = table(device, weights);
        let pipelines = &gpu.pipelines().sharpen;
        let pass = |encoder: &mut wgpu::CommandEncoder,
                    pipeline: &wgpu::ComputePipeline,
                    entries: &[wgpu::BindGroupEntry<'_>],
                    groups: (u32, u32)| {
            record(device, encoder, pipeline, entries, groups);
        };
        let blur = |encoder: &mut wgpu::CommandEncoder,
                    params: &wgpu::Buffer,
                    groups: (u32, u32),
                    from: &wgpu::Buffer,
                    to: &wgpu::Buffer| {
            let rows = [
                entry(1, params),
                entry(4, from),
                entry(7, scratch),
                entry(8, &weights),
            ];
            pass(encoder, &pipelines.blur_rows, &rows, groups);
            let columns = [
                entry(1, params),
                entry(4, scratch),
                entry(7, to),
                entry(8, &weights),
            ];
            pass(encoder, &pipelines.blur_columns, &columns, groups);
        };
        let groups = sharpen.groups;
        let luma_entries = [
            entry(0, &self.work),
            entry(1, &params),
            entry(2, &to_display),
            entry(7, luma),
        ];
        pass(encoder, &pipelines.luma, &luma_entries, groups);
        if small.is_some() {
            // The reduced plane fits in `mask`, which only v2's edge uses.
            let reduce = [entry(1, &small_params), entry(4, luma), entry(7, mask)];
            pass(encoder, &pipelines.reduce, &reduce, small_groups);
            blur(encoder, &small_params, small_groups, mask, mask);
            let expand = [entry(1, &params), entry(4, mask), entry(7, blurred)];
            pass(encoder, &pipelines.expand, &expand, groups);
        } else {
            blur(encoder, &params, groups, luma, blurred);
        }
        if sharpen.edge.is_some() {
            pass(
                encoder,
                &pipelines.edge,
                &[entry(1, &params), entry(4, luma), entry(7, mask)],
                groups,
            );
            blur(encoder, &params, groups, mask, mask);
        }
        pass(
            encoder,
            &pipelines.apply,
            &[
                entry(0, &self.work),
                entry(1, &params),
                entry(2, &to_display),
                entry(3, &to_linear),
                entry(4, luma),
                entry(5, blurred),
                entry(6, mask),
            ],
            groups,
        );
        Some(())
    }
}

/// Records one compute pass of `pipeline` over the frame.
fn record(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    pipeline: &wgpu::ComputePipeline,
    entries: &[wgpu::BindGroupEntry<'_>],
    (groups_x, groups_y): (u32, u32),
) {
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(0),
        entries,
    });
    let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor::default());
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, &bind_group, &[]);
    pass.dispatch_workgroups(groups_x, groups_y, 1);
}

/// The compiled pipelines, built once per device.
#[derive(Debug)]
pub(crate) struct Pipelines {
    gains: wgpu::ComputePipeline,
    display: wgpu::ComputePipeline,
    sharpen: SharpenPipelines,
    dehaze: DehazePipelines,
    output: wgpu::ComputePipeline,
}

/// The passes of [`Op::Sharpen`] and [`Op::LocalContrast`], entry points of
/// one shader module.
#[derive(Debug)]
pub(crate) struct SharpenPipelines {
    luma: wgpu::ComputePipeline,
    blur_rows: wgpu::ComputePipeline,
    blur_columns: wgpu::ComputePipeline,
    reduce: wgpu::ComputePipeline,
    expand: wgpu::ComputePipeline,
    edge: wgpu::ComputePipeline,
    apply: wgpu::ComputePipeline,
}

impl Pipelines {
    pub(crate) fn new(device: &wgpu::Device) -> Pipelines {
        let display = format!("{AXIS}{DISPLAY}");
        let sharpen = format!("{AXIS}{SHARPEN}");
        Pipelines {
            gains: pipeline(device, "gains", GAINS, "main"),
            display: pipeline(device, "display", &display, "main"),
            sharpen: SharpenPipelines {
                luma: pipeline(device, "sharpen luma", &sharpen, "luma"),
                blur_rows: pipeline(device, "sharpen blur rows", &sharpen, "blur_rows"),
                blur_columns: pipeline(device, "sharpen blur columns", &sharpen, "blur_columns"),
                reduce: pipeline(device, "sharpen reduce", &sharpen, "reduce"),
                expand: pipeline(device, "sharpen expand", &sharpen, "expand"),
                edge: pipeline(device, "sharpen edge", &sharpen, "edge"),
                apply: pipeline(device, "sharpen apply", &sharpen, "apply"),
            },
            dehaze: DehazePipelines::new(device, AXIS),
            output: pipeline(device, "output rendering", OUTPUT_RENDERING, "main"),
        }
    }
}

fn pipeline(
    device: &wgpu::Device,
    label: &str,
    source: &str,
    entry_point: &str,
) -> wgpu::ComputePipeline {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some(label),
        layout: None,
        module: &module,
        entry_point: Some(entry_point),
        compilation_options: wgpu::PipelineCompilationOptions::default(),
        cache: None,
    })
}

/// Workgroups for `pixels` invocations, folded into two dimensions: one
/// dimension stops at 65 535 groups, which a 4K frame exceeds.
fn dispatch(pixels: u32) -> (u32, u32) {
    let groups = pixels.div_ceil(WORKGROUP);
    let x = groups.min(MAX_GROUPS);
    (x, groups.div_ceil(x))
}

fn uniform(device: &wgpu::Device, bytes: &[u8]) -> wgpu::Buffer {
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("params"),
        contents: bytes,
        usage: wgpu::BufferUsages::UNIFORM,
    })
}

fn entry(binding: u32, buffer: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry {
        binding,
        resource: buffer.as_entire_binding(),
    }
}

fn table(device: &wgpu::Device, values: &[f32]) -> wgpu::Buffer {
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("table"),
        contents: &bytes_of_f32(values),
        usage: wgpu::BufferUsages::STORAGE,
    })
}

/// `crop` v1's rectangle in pixels — the CPU's arithmetic, in the CPU's
/// `f64`, so both cut the same pixels.
fn crop_window(
    width: u32,
    height: u32,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) -> Option<(u32, u32, u32, u32)> {
    if width == 0 || height == 0 {
        return None;
    }
    let (fw, fh) = (f64::from(width), f64::from(height));
    let x0 = ((x * fw).round() as u32).min(width - 1);
    let y0 = ((y * fh).round() as u32).min(height - 1);
    let out_w = ((w * fw).round() as u32).clamp(1, width - x0);
    let out_h = ((h * fh).round() as u32).clamp(1, height - y0);
    Some((x0, y0, out_w, out_h))
}

/// The shader's mode, its five values, and the curve table if any.
fn display_params(op: &DisplayOp) -> (u32, [f32; 5], Option<Arc<Vec<f32>>>) {
    match op {
        DisplayOp::Contrast { k } => (0, [*k, 0.0, 0.0, 0.0, 0.0], None),
        DisplayOp::WhitesBlacks { black, scale } => (1, [*black, *scale, 0.0, 0.0, 0.0], None),
        DisplayOp::HighlightsShadows {
            highlights,
            shadows,
        } => (2, [*highlights, *shadows, 0.0, 0.0, 0.0], None),
        DisplayOp::Levels {
            in_black,
            in_range,
            out_black,
            out_range,
            exponent,
        } => (
            3,
            [*in_black, *in_range, *out_black, *out_range, *exponent],
            None,
        ),
        DisplayOp::Curve(table) => (4, [0.0; 5], Some(Arc::clone(table))),
        DisplayOp::Saturate { k, vibrance } => (
            5,
            [*k, if *vibrance { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0],
            None,
        ),
        // The bands go through the table binding: hues, then saturations,
        // then luminances.
        DisplayOp::Hsl { bands } => {
            let mut table = Vec::with_capacity(24);
            for component in 0..3 {
                table.extend(bands.iter().map(|band| band[component]));
            }
            (6, [0.0; 5], Some(Arc::new(table)))
        }
        DisplayOp::Monochrome => (7, [0.0; 5], None),
        // Through the table binding: nine tint values, three shifts.
        DisplayOp::ColorGrading {
            tints,
            luminance,
            balance,
            blending,
        } => {
            let mut table: Vec<f32> = tints.iter().flatten().copied().collect();
            table.extend_from_slice(luminance);
            (
                8,
                [*balance, *blending, 0.0, 0.0, 0.0],
                Some(Arc::new(table)),
            )
        }
    }
}

/// Entries of the curve table bound for this operation, the transfer table
/// standing in when there is none.
fn curve_entries(curve: Option<&Vec<f32>>) -> Option<u32> {
    u32::try_from(curve.map_or(0, Vec::len)).ok()
}

fn bytes_of_f32(values: &[f32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// `gains` v1: each sample times its channel's gain, floored at 0.
const GAINS: &str = r"
struct Params { gains: vec3<f32>, pixels: u32, stride: u32 }
@group(0) @binding(0) var<storage, read_write> samples: array<f32>;
@group(0) @binding(1) var<uniform> params: Params;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let pixel = id.y * params.stride + id.x;
    if (pixel >= params.pixels) { return; }
    let base = pixel * 3u;
    samples[base] = max(samples[base] * params.gains.x, 0.0);
    samples[base + 1u] = max(samples[base + 1u] * params.gains.y, 0.0);
    samples[base + 2u] = max(samples[base + 2u] * params.gains.z, 0.0);
}
";

/// `output_rendering` v1, then the 8-bit quantisation of `Pixels::to_rgb8`
/// (round half away from zero, as `f32::round` does — not WGSL's `round`,
/// which rounds half to even).
const OUTPUT_RENDERING: &str = r"
struct Params {
    row0: vec4<f32>,
    row1: vec4<f32>,
    row2: vec4<f32>,
    knee: f32,
    pixels: u32,
    stride: u32,
    table_entries: u32,
    source_width: u32,
    x0: u32,
    y0: u32,
    out_width: u32,
    vignette_on: u32,
    vignette_amount: f32,
    vignette_midpoint: f32,
    vignette_exponent: f32,
    vignette_corner: f32,
    vignette_width: f32,
    out_height: u32,
}
@group(0) @binding(0) var<storage, read_write> samples: array<f32>;
@group(0) @binding(1) var<uniform> params: Params;
@group(0) @binding(2) var<storage, read> display_table: array<f32>;
@group(0) @binding(3) var<storage, read_write> packed: array<u32>;

fn rolloff(value: f32, knee: f32) -> f32 {
    if (value <= knee || knee >= 1.0) { return min(value, 1.0); }
    let white = 2.0 - knee;
    if (value >= white) { return 1.0; }
    let t = (value - knee) / (white - knee);
    let eased = 1.0 - (1.0 - t) * (1.0 - t);
    return knee + (1.0 - knee) * eased;
}

fn display(value: f32) -> f32 {
    let size = params.table_entries - 1u;
    let x = clamp(value, 0.0, 1.0) * f32(size);
    let i = min(u32(x), size - 1u);
    let t = x - f32(i);
    return display_table[i] + (display_table[i + 1u] - display_table[i]) * t;
}

// `vignette` v1's gain at an output pixel, floored at 0 like the stage.
fn vignette_gain(x: u32, y: u32) -> f32 {
    let n = params.vignette_exponent;
    let u = 2.0 * (f32(x) + 0.5) / f32(params.out_width) - 1.0;
    let v = 2.0 * (f32(y) + 0.5) / f32(params.out_height) - 1.0;
    let d = pow(pow(abs(u), n) + pow(abs(v), n), 1.0 / n) / params.vignette_corner;
    let edge0 = params.vignette_midpoint;
    let edge1 = params.vignette_midpoint + params.vignette_width;
    let t0 = clamp((d - edge0) / (edge1 - edge0), 0.0, 1.0);
    let t = t0 * t0 * (3.0 - 2.0 * t0);
    return max(exp2(3.0 * params.vignette_amount * t), 0.0);
}

fn quantize(value: f32) -> u32 {
    return u32(floor(clamp(value, 0.0, 1.0) * 255.0 + 0.5));
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let pixel = id.y * params.stride + id.x;
    if (pixel >= params.pixels) { return; }
    // The crop's window into the frame: row and column of the output pixel,
    // offset into the source.
    let source = (params.y0 + pixel / params.out_width) * params.source_width
        + params.x0 + pixel % params.out_width;
    let base = source * 3u;
    var linear = vec3<f32>(samples[base], samples[base + 1u], samples[base + 2u]);
    if (params.vignette_on == 1u) {
        linear = linear * vignette_gain(pixel % params.out_width, pixel / params.out_width);
    }
    let rolled = vec3<f32>(
        rolloff(linear.x, params.knee),
        rolloff(linear.y, params.knee),
        rolloff(linear.z, params.knee),
    );
    let r4 = vec4<f32>(rolled, 0.0);
    let srgb = vec3<f32>(dot(params.row0, r4), dot(params.row1, r4), dot(params.row2, r4));
    let r = quantize(display(clamp(srgb.x, 0.0, 1.0)));
    let g = quantize(display(clamp(srgb.y, 0.0, 1.0)));
    let b = quantize(display(clamp(srgb.z, 0.0, 1.0)));
    packed[pixel] = r | (g << 8u) | (b << 16u);
}
";

/// The CPU's `display`/`linear` pair: its tables below 1, the exact sRGB
/// formulas above. Prepended to every module that crosses the display
/// axis, which declares the two tables it reads.
const AXIS: &str = r"
fn lookup_display(v: f32) -> f32 {
    let size = arrayLength(&to_display_table) - 1u;
    let x = clamp(v, 0.0, 1.0) * f32(size);
    let i = min(u32(x), size - 1u);
    let t = x - f32(i);
    return to_display_table[i] + (to_display_table[i + 1u] - to_display_table[i]) * t;
}

fn lookup_linear(v: f32) -> f32 {
    let size = arrayLength(&to_linear_table) - 1u;
    let x = clamp(v, 0.0, 1.0) * f32(size);
    let i = min(u32(x), size - 1u);
    let t = x - f32(i);
    return to_linear_table[i] + (to_linear_table[i + 1u] - to_linear_table[i]) * t;
}

fn to_display(v: f32) -> f32 {
    if (v <= 1.0) { return lookup_display(v); }
    if (v <= 0.0031308) { return v * 12.92; }
    return 1.055 * pow(v, 1.0 / 2.4) - 0.055;
}

fn to_linear(v: f32) -> f32 {
    if (v <= 1.0) { return lookup_linear(v); }
    if (v <= 0.04045) { return v / 12.92; }
    return pow((v + 0.055) / 1.055, 2.4);
}
";

/// The display-axis operators: `in_display` (to the display axis, the
/// operator, back to linear from a value floored at 0), across [`AXIS`].
const DISPLAY: &str = r"
struct Params {
    mode: u32,
    pixels: u32,
    stride: u32,
    curve_entries: u32,
    a: f32,
    b: f32,
    c: f32,
    d: f32,
    e: f32,
}
@group(0) @binding(0) var<storage, read_write> samples: array<f32>;
@group(0) @binding(1) var<uniform> params: Params;
@group(0) @binding(2) var<storage, read> to_display_table: array<f32>;
@group(0) @binding(3) var<storage, read> to_linear_table: array<f32>;
@group(0) @binding(4) var<storage, read> curve_table: array<f32>;

fn lookup_curve(v: f32) -> f32 {
    let size = params.curve_entries - 1u;
    let x = clamp(v, 0.0, 1.0) * f32(size);
    let i = min(u32(x), size - 1u);
    let t = x - f32(i);
    return curve_table[i] + (curve_table[i + 1u] - curve_table[i]) * t;
}

// The per-sample function of the curve-like modes, defined on [0, 1].
fn curve(x: f32) -> f32 {
    switch params.mode {
        case 0u: {
            let k = params.a;
            if (k >= 0.0) {
                let s = x * x * (3.0 - 2.0 * x);
                return max((1.0 - k) * x + k * s, 0.0);
            }
            let flat = 0.25 + 0.5 * x;
            return max((1.0 + k) * x - k * flat, 0.0);
        }
        case 1u: {
            return max((x - params.a) * params.b, 0.0);
        }
        case 3u: {
            let t = clamp((x - params.a) / params.b, 0.0, 1.0);
            var shaped = t;
            if (params.e != 1.0) { shaped = pow(t, params.e); }
            return params.c + params.d * shaped;
        }
        default: {
            return lookup_curve(x);
        }
    }
}

// `display_curve`: the function below 1, its gain at white above.
fn display_curve(x: f32) -> f32 {
    if (x <= 1.0) { return curve(x); }
    return x * curve(1.0);
}

fn highlights_shadows(rgb: vec3<f32>) -> vec3<f32> {
    let l = clamp(0.2627 * rgb.x + 0.6780 * rgb.y + 0.0593 * rgb.z, 0.0, 1.0);
    let delta = 0.5 * (params.b * (1.0 - l) * (1.0 - l) + params.a * l * l);
    var out = rgb;
    for (var c = 0u; c < 3u; c = c + 1u) {
        let x = rgb[c];
        var moved: f32;
        if (x > 1.0) {
            if (delta >= 0.0) { moved = x; } else { moved = x * (1.0 + delta); }
        } else if (delta >= 0.0) {
            moved = x + delta * (1.0 - x);
        } else {
            moved = x + delta * x;
        }
        out[c] = max(moved, 0.0);
    }
    return out;
}

fn euclid(x: f32, m: f32) -> f32 {
    return x - floor(x / m) * m;
}

fn rgb_to_hsl(rgb: vec3<f32>) -> vec3<f32> {
    let mx = max(rgb.x, max(rgb.y, rgb.z));
    let mn = min(rgb.x, min(rgb.y, rgb.z));
    let l = (mx + mn) / 2.0;
    let delta = mx - mn;
    if (delta < 1e-6) { return vec3<f32>(0.0, 0.0, l); }
    var s: f32;
    if (l <= 0.5) { s = delta / (mx + mn); } else { s = delta / (2.0 - mx - mn); }
    var h: f32;
    if (mx == rgb.x) {
        h = 60.0 * euclid((rgb.y - rgb.z) / delta, 6.0);
    } else if (mx == rgb.y) {
        h = 60.0 * ((rgb.z - rgb.x) / delta + 2.0);
    } else {
        h = 60.0 * ((rgb.x - rgb.y) / delta + 4.0);
    }
    return vec3<f32>(euclid(h, 360.0), s, l);
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> vec3<f32> {
    if (s <= 1e-6) { return vec3<f32>(l); }
    let c = (1.0 - abs(2.0 * l - 1.0)) * s;
    let x = c * (1.0 - abs(euclid(h / 60.0, 2.0) - 1.0));
    let m = l - c / 2.0;
    var rgb1: vec3<f32>;
    if (h < 60.0) { rgb1 = vec3<f32>(c, x, 0.0); }
    else if (h < 120.0) { rgb1 = vec3<f32>(x, c, 0.0); }
    else if (h < 180.0) { rgb1 = vec3<f32>(0.0, c, x); }
    else if (h < 240.0) { rgb1 = vec3<f32>(0.0, x, c); }
    else if (h < 300.0) { rgb1 = vec3<f32>(x, 0.0, c); }
    else { rgb1 = vec3<f32>(c, 0.0, x); }
    return clamp(rgb1 + vec3<f32>(m), vec3<f32>(0.0), vec3<f32>(1.0));
}

// `hsl::v1`'s band centres, in degrees.
const CENTRES = array<f32, 8>(0.0, 30.0, 60.0, 120.0, 180.0, 240.0, 275.0, 315.0);

// `hsl` v1 inside `preserving_headroom`; the bands are in `curve_table`.
fn hsl_mix(input: vec3<f32>) -> vec3<f32> {
    let peak = max(1.0, max(input.x, max(input.y, input.z)));
    var rgb = input;
    if (peak > 1.0) { rgb = rgb / peak; }
    let hsl = rgb_to_hsl(rgb);
    let h = euclid(hsl.x, 360.0);
    var i0 = 0u;
    var i1 = 1u;
    var t = 0.0;
    for (var i = 0u; i < 8u; i = i + 1u) {
        let j = (i + 1u) % 8u;
        let start = CENTRES[i];
        var end = CENTRES[j];
        if (j == 0u) { end = end + 360.0; }
        var probe = h;
        if (h < start) { probe = h + 360.0; }
        if (probe >= start && probe <= end) {
            i0 = i;
            i1 = j;
            t = (probe - start) / (end - start);
            break;
        }
    }
    let w1 = t * t * (3.0 - 2.0 * t);
    let w0 = 1.0 - w1;
    let hue_shift = (curve_table[i0] * w0 + curve_table[i1] * w1) / 100.0 * 30.0;
    let sat_shift = (curve_table[8u + i0] * w0 + curve_table[8u + i1] * w1) / 100.0;
    let lum_shift = (curve_table[16u + i0] * w0 + curve_table[16u + i1] * w1) / 100.0;
    let new_h = euclid(hsl.x + hue_shift, 360.0);
    let new_s = clamp(hsl.y * (1.0 + sat_shift), 0.0, 1.0);
    let new_l = clamp(hsl.z + lum_shift * 0.25, 0.0, 1.0);
    rgb = hsl_to_rgb(new_h, new_s, new_l);
    if (peak > 1.0) { rgb = rgb * peak; }
    return rgb;
}

// `color_grading` v1 inside `preserving_headroom`; tints and shifts are in
// `curve_table`, balance and blending in `a` and `b`.
fn color_grading(input: vec3<f32>) -> vec3<f32> {
    let peak = max(1.0, max(input.x, max(input.y, input.z)));
    var rgb = input;
    if (peak > 1.0) { rgb = rgb / peak; }
    let l = 0.2627 * rgb.x + 0.6780 * rgb.y + 0.0593 * rgb.z;
    let gap = 0.3;
    let centre = clamp(0.5 - params.a * 0.25, 0.05, 0.95);
    let half_width = 0.02 + params.b * 0.13;
    let b1 = centre - gap / 2.0;
    let b2 = centre + gap / 2.0;
    let s0 = clamp((l - (b1 - half_width)) / (2.0 * half_width), 0.0, 1.0);
    let shadow = 1.0 - s0 * s0 * (3.0 - 2.0 * s0);
    let h0 = clamp((l - (b2 - half_width)) / (2.0 * half_width), 0.0, 1.0);
    let highlight = h0 * h0 * (3.0 - 2.0 * h0);
    let midtone = max(1.0 - shadow - highlight, 0.0);
    let sum = shadow + midtone + highlight;
    var weights = vec3<f32>(0.0, 1.0, 0.0);
    if (sum > 1e-6) { weights = vec3<f32>(shadow, midtone, highlight) / sum; }
    var tint = vec3<f32>(0.0);
    var lum_shift = 0.0;
    for (var zone = 0u; zone < 3u; zone = zone + 1u) {
        let base = zone * 3u;
        tint = tint + vec3<f32>(curve_table[base], curve_table[base + 1u], curve_table[base + 2u]) * weights[zone];
        lum_shift = lum_shift + curve_table[9u + zone] * weights[zone];
    }
    let delta_l = lum_shift * 0.25;
    rgb = clamp(rgb + vec3<f32>(delta_l) + tint * 0.6, vec3<f32>(0.0), vec3<f32>(1.0));
    if (peak > 1.0) { rgb = rgb * peak; }
    return rgb;
}

// `saturate` with `preserving_headroom` around it: the pixel is brought into
// the unit cube by its peak, adjusted there, and scaled back.
fn saturate(input: vec3<f32>) -> vec3<f32> {
    let peak = max(1.0, max(input.x, max(input.y, input.z)));
    var rgb = input;
    if (peak > 1.0) { rgb = rgb / peak; }
    let l = 0.2627 * rgb.x + 0.6780 * rgb.y + 0.0593 * rgb.z;
    var factor = 1.0 + params.a;
    if (params.b > 0.5) {
        let chroma = max(0.0, max(rgb.x, max(rgb.y, rgb.z))) - min(1.0, min(rgb.x, min(rgb.y, rgb.z)));
        factor = 1.0 + params.a * (1.0 - chroma);
    }
    rgb = clamp(vec3<f32>(l) + (rgb - vec3<f32>(l)) * factor, vec3<f32>(0.0), vec3<f32>(1.0));
    if (peak > 1.0) { rgb = rgb * peak; }
    return rgb;
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let pixel = id.y * params.stride + id.x;
    if (pixel >= params.pixels) { return; }
    let base = pixel * 3u;
    if (params.mode == 7u) {
        let l = 0.2627 * samples[base] + 0.6780 * samples[base + 1u] + 0.0593 * samples[base + 2u];
        samples[base] = l;
        samples[base + 1u] = l;
        samples[base + 2u] = l;
        return;
    }
    var rgb = vec3<f32>(
        to_display(samples[base]),
        to_display(samples[base + 1u]),
        to_display(samples[base + 2u]),
    );
    if (params.mode == 2u) {
        rgb = highlights_shadows(rgb);
    } else if (params.mode == 5u) {
        rgb = saturate(rgb);
    } else if (params.mode == 6u) {
        rgb = hsl_mix(rgb);
    } else if (params.mode == 8u) {
        rgb = color_grading(rgb);
    } else {
        rgb = vec3<f32>(display_curve(rgb.x), display_curve(rgb.y), display_curve(rgb.z));
    }
    samples[base] = to_linear(max(rgb.x, 0.0));
    samples[base + 1u] = to_linear(max(rgb.y, 0.0));
    samples[base + 2u] = to_linear(max(rgb.z, 0.0));
}
";

/// `sharpen` v1/v2, one entry point per pass, across [`AXIS`]: `luma`
/// writes the display-axis luma plane, `blur_rows` then `blur_columns` are
/// `gaussian_blur` (edges replicated, the same half kernel), `edge` is
/// v2's Sobel mask before its blur, and `apply` adds the delta on the
/// display axis and returns to linear, as `add_luma_delta` inside
/// `in_display` does.
const SHARPEN: &str = r"
struct Params {
    pixels: u32,
    stride: u32,
    width: u32,
    height: u32,
    taps: u32,
    masked: u32,
    k: f32,
    threshold: f32,
    softness: f32,
    other_width: u32,
    other_height: u32,
    factor: u32,
}
@group(0) @binding(0) var<storage, read_write> samples: array<f32>;
@group(0) @binding(1) var<uniform> params: Params;
@group(0) @binding(2) var<storage, read> to_display_table: array<f32>;
@group(0) @binding(3) var<storage, read> to_linear_table: array<f32>;
@group(0) @binding(4) var<storage, read> plane: array<f32>;
@group(0) @binding(5) var<storage, read> blurred: array<f32>;
@group(0) @binding(6) var<storage, read> mask: array<f32>;
@group(0) @binding(7) var<storage, read_write> plane_out: array<f32>;
@group(0) @binding(8) var<storage, read> weights: array<f32>;

fn pixel_of(id: vec3<u32>) -> u32 {
    return id.y * params.stride + id.x;
}

@compute @workgroup_size(64)
fn luma(@builtin(global_invocation_id) id: vec3<u32>) {
    let pixel = pixel_of(id);
    if (pixel >= params.pixels) { return; }
    let base = pixel * 3u;
    plane_out[pixel] = 0.2627 * to_display(samples[base])
        + 0.6780 * to_display(samples[base + 1u])
        + 0.0593 * to_display(samples[base + 2u]);
}

@compute @workgroup_size(64)
fn blur_rows(@builtin(global_invocation_id) id: vec3<u32>) {
    let pixel = pixel_of(id);
    if (pixel >= params.pixels) { return; }
    let x = pixel % params.width;
    let row = pixel - x;
    var acc = weights[0] * plane[pixel];
    for (var k = 1u; k <= params.taps; k++) {
        let lo = select(0u, x - k, x >= k);
        let hi = min(x + k, params.width - 1u);
        acc += weights[k] * (plane[row + lo] + plane[row + hi]);
    }
    plane_out[pixel] = acc;
}

@compute @workgroup_size(64)
fn blur_columns(@builtin(global_invocation_id) id: vec3<u32>) {
    let pixel = pixel_of(id);
    if (pixel >= params.pixels) { return; }
    let x = pixel % params.width;
    let y = pixel / params.width;
    var acc = weights[0] * plane[pixel];
    for (var k = 1u; k <= params.taps; k++) {
        let lo = select(0u, y - k, y >= k);
        let hi = min(y + k, params.height - 1u);
        acc += weights[k] * (plane[lo * params.width + x] + plane[hi * params.width + x]);
    }
    plane_out[pixel] = acc;
}

// `approx_blur`'s box reduction, one invocation per pixel of the reduced
// plane, summed in the CPU's order.
@compute @workgroup_size(64)
fn reduce(@builtin(global_invocation_id) id: vec3<u32>) {
    let pixel = pixel_of(id);
    if (pixel >= params.pixels) { return; }
    let sx = pixel % params.width;
    let sy = pixel / params.width;
    var sum = 0.0;
    var count = 0u;
    for (var dy = 0u; dy < params.factor; dy++) {
        let y = sy * params.factor + dy;
        if (y >= params.other_height) { continue; }
        for (var dx = 0u; dx < params.factor; dx++) {
            let x = sx * params.factor + dx;
            if (x >= params.other_width) { continue; }
            sum += plane[y * params.other_width + x];
            count += 1u;
        }
    }
    plane_out[pixel] = select(0.0, sum / f32(count), count > 0u);
}

// `approx_blur`'s bilinear return to the full plane.
@compute @workgroup_size(64)
fn expand(@builtin(global_invocation_id) id: vec3<u32>) {
    let pixel = pixel_of(id);
    if (pixel >= params.pixels) { return; }
    let x = pixel % params.width;
    let y = pixel / params.width;
    let factor = f32(params.factor);
    let sy = min(f32(y) / factor, f32(params.other_height) - 1.0);
    let y0 = u32(floor(sy));
    let y1 = min(y0 + 1u, params.other_height - 1u);
    let ty = sy - f32(y0);
    let sx = min(f32(x) / factor, f32(params.other_width) - 1.0);
    let x0 = u32(floor(sx));
    let x1 = min(x0 + 1u, params.other_width - 1u);
    let tx = sx - f32(x0);
    let v00 = plane[y0 * params.other_width + x0];
    let v10 = plane[y0 * params.other_width + x1];
    let v01 = plane[y1 * params.other_width + x0];
    let v11 = plane[y1 * params.other_width + x1];
    let v0 = v00 + (v10 - v00) * tx;
    let v1 = v01 + (v11 - v01) * tx;
    plane_out[pixel] = v0 + (v1 - v0) * ty;
}

fn at(x: u32, y: u32) -> f32 {
    return plane[y * params.width + x];
}

@compute @workgroup_size(64)
fn edge(@builtin(global_invocation_id) id: vec3<u32>) {
    let pixel = pixel_of(id);
    if (pixel >= params.pixels) { return; }
    let x = pixel % params.width;
    let y = pixel / params.width;
    let x0 = select(0u, x - 1u, x > 0u);
    let x1 = min(x + 1u, params.width - 1u);
    let y0 = select(0u, y - 1u, y > 0u);
    let y1 = min(y + 1u, params.height - 1u);
    let gx = (at(x1, y0) + 2.0 * at(x1, y) + at(x1, y1))
        - (at(x0, y0) + 2.0 * at(x0, y) + at(x0, y1));
    let gy = (at(x0, y1) + 2.0 * at(x, y1) + at(x1, y1))
        - (at(x0, y0) + 2.0 * at(x, y0) + at(x1, y0));
    let magnitude = sqrt(gx * gx + gy * gy) / 4.0;
    let t = clamp(
        (magnitude - params.threshold + params.softness) / (2.0 * params.softness),
        0.0,
        1.0,
    );
    plane_out[pixel] = t * t * (3.0 - 2.0 * t);
}

@compute @workgroup_size(64)
fn apply(@builtin(global_invocation_id) id: vec3<u32>) {
    let pixel = pixel_of(id);
    if (pixel >= params.pixels) { return; }
    var weight = 1.0;
    if (params.masked != 0u) { weight = mask[pixel]; }
    let delta = weight * params.k * (plane[pixel] - blurred[pixel]);
    let base = pixel * 3u;
    for (var c = 0u; c < 3u; c++) {
        let shifted = max(to_display(samples[base + c]) + delta, 0.0);
        samples[base + c] = to_linear(shifted);
    }
}
";
