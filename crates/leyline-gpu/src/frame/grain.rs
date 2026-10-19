//! `grain` v1 and v2 on the GPU: the CPU's value noise, hash for hash, on
//! the display axis of each output pixel.
//!
//! The lattice hash is a 64-bit mix (splitmix's finaliser); WGSL has no
//! 64-bit integer, so each `u64` is a `vec2<u32>` (low, high) and the
//! multiplication is rebuilt from 16-bit halves. The result is the same
//! 24 bits the CPU keeps, so the field matches exactly; only the `f32`
//! arithmetic around it may round differently.

/// The grain's functions, joined to the output module, which declares the
/// parameters (`grain_*`) and the transfer tables they read.
pub(super) const GRAIN: &str = r"
// The high 32 bits of a 32×32-bit product.
fn mul_high(a: u32, b: u32) -> u32 {
    let a_low = a & 0xffffu;
    let a_high = a >> 16u;
    let b_low = b & 0xffffu;
    let b_high = b >> 16u;
    let low_low = a_low * b_low;
    let high_low = a_high * b_low;
    let low_high = a_low * b_high;
    let carry = ((low_low >> 16u) + (high_low & 0xffffu) + (low_high & 0xffffu)) >> 16u;
    return a_high * b_high + (high_low >> 16u) + (low_high >> 16u) + carry;
}

// A wrapping 64-bit product, each operand as (low, high).
fn mul64(a: vec2<u32>, b: vec2<u32>) -> vec2<u32> {
    return vec2<u32>(a.x * b.x, mul_high(a.x, b.x) + a.x * b.y + a.y * b.x);
}

// `h ^= h >> 33`.
fn fold33(h: vec2<u32>) -> vec2<u32> {
    return vec2<u32>(h.x ^ (h.y >> 1u), h.y);
}

// The CPU's `lattice`: v1's when `channel` is 0, v2's salted one for
// channel `channel - 1` otherwise.
fn lattice(x: i32, y: i32, channel: u32) -> f32 {
    var h = vec2<u32>(bitcast<u32>(x), bitcast<u32>(y));
    if (channel > 0u) {
        h = h ^ mul64(vec2<u32>(channel, 0u), vec2<u32>(0x7f4a7c15u, 0x9e3779b9u));
    }
    h = fold33(h);
    h = mul64(h, vec2<u32>(0xed558ccdu, 0xff51afd7u));
    h = fold33(h);
    h = mul64(h, vec2<u32>(0x1a85ec53u, 0xc4ceb9feu));
    h = fold33(h);
    return f32(h.y >> 8u) / 8388608.0 - 1.0;
}

// `a + (b − a)·t`, as the CPU writes it: WGSL's `mix` rounds otherwise.
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    return a + (b - a) * t;
}

fn value_noise(x: f32, y: f32, channel: u32) -> f32 {
    let x0 = floor(x);
    let y0 = floor(y);
    let fx = x - x0;
    let fy = y - y0;
    let wx = fx * fx * (3.0 - 2.0 * fx);
    let wy = fy * fy * (3.0 - 2.0 * fy);
    let ix = i32(x0);
    let iy = i32(y0);
    let top = lerp(lattice(ix, iy, channel), lattice(ix + 1, iy, channel), wx);
    let bottom = lerp(lattice(ix, iy + 1, channel), lattice(ix + 1, iy + 1, channel), wx);
    return lerp(top, bottom, wy);
}

fn octaves(u: f32, v: f32, channel: u32) -> f32 {
    let coarse = value_noise(u, v, channel);
    let fine = value_noise(2.0 * u, 2.0 * v, channel);
    return coarse * (1.0 - params.grain_fine) + fine * params.grain_fine;
}

// The grain added to a linear pixel at output position (x, y).
fn grain(linear: vec3<f32>, x: u32, y: u32) -> vec3<f32> {
    let fx = (f32(x) + 0.5) / params.grain_scale;
    let fy = (f32(y) + 0.5) / params.grain_scale;
    let u = fx / params.grain_cell;
    let v = fy / params.grain_cell;
    var noise = vec3<f32>(octaves(u, v, 0u));
    // v2's colour: three salted fields about their mean. At 0 the stage
    // renders v1, so the fields are not even computed.
    if (params.grain_colour > 0.0) {
        let layers = vec3<f32>(octaves(u, v, 1u), octaves(u, v, 2u), octaves(u, v, 3u));
        let mean = (layers.x + layers.y + layers.z) / 3.0;
        noise = noise + params.grain_colour * (layers - vec3<f32>(mean));
    }
    var out = linear;
    for (var c = 0u; c < 3u; c++) {
        let d = to_display(linear[c]);
        let weight = max(4.0 * d * (1.0 - d), 0.0);
        out[c] = to_linear(max(d + noise[c] * params.grain_amplitude * weight, 0.0));
    }
    return out;
}
";
