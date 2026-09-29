// Piano-roll note instanced rendering (one instanced draw, no cull / LOD).
//
// 16 bytes per instance (@location(0)) = (start_tick, end_tick, meta, pad),
// meta = key | vel << 8 | slot << 16 | layer << 24.
// Pan / zoom only touch uniforms; instances are re-uploaded only when notes or selection change.

struct Globals {
    canvas_min: vec2<f32>, // canvas top-left (logical points, screen coordinates)
    ppp: f32,              // physical pixels per logical point
    kb_w: f32,             // keyboard width (logical points, matches View::kb_w)
    origin_tick: u32,      // tick at the left edge of the view
    _pad0: u32,
    px_per_tick: f32, // logical points per tick = sx / ppq
    top: f32,         // pitch at the top of the view (View::top)
    sy: f32,          // logical points per pitch row (View::sy)
    ruler_h: f32,     // ruler height (logical points, matches View::ruler_h)
    screen_px: vec2<f32>, // screen size (physical pixels)
    srgb: u32,            // whether the target format is sRGB: 1 = output linear color directly
    _pad1: u32,
    _pad2: vec2<u32>,
    fill: array<vec4<f32>, 16>,    // linear fill colors: 15 slots + selection color
    outline: array<vec4<f32>, 16>, // linear outline colors: same as above
};

@group(0) @binding(0) var<uniform> g: Globals;

struct VOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) @interpolate(flat) inst: vec4<u32>,
    // physical-pixel coordinate relative to the rectangle's top-left / rectangle size in physical pixels
    @location(1) @interpolate(linear) local: vec2<f32>,
    @location(2) @interpolate(linear) size: vec2<f32>,
    // whether the left, right, top and bottom edges were not clamped out of view (a clamped edge is not outlined)
    @location(3) @interpolate(flat) keep: vec4<u32>,
};

// Clamp far-away coordinates to this range (2^20 physical pixels, far beyond any window)
// to avoid precision loss when subtracting large f32 values; a clamped edge is no longer outlined.
const LIM: f32 = 1048576.0;

fn keep01(on: bool) -> u32 {
    return select(0u, 1u, on);
}

@vertex
fn vs_main(@builtin(vertex_index) vi: u32, @location(0) inst: vec4<u32>) -> VOut {
    let start = inst.x;
    let end = inst.y;
    let m = inst.z;
    let key = f32(m & 0xffu);

    // Wrapping u32 subtraction read back as two's-complement i32: even with a huge
    // origin_tick, (tick - origin_tick) stays an exact small integer and converts to f32 without precision loss.
    let dt0 = f32(i32(start - g.origin_tick));
    let dt1 = f32(i32(end - g.origin_tick));

    let raw0 = (g.canvas_min.x + g.kb_w + dt0 * g.px_per_tick) * g.ppp;
    let raw1 = (g.canvas_min.x + g.kb_w + dt1 * g.px_per_tick) * g.ppp;
    let rawy0 = (g.canvas_min.y + g.ruler_h + (g.top - key - 0.5) * g.sy) * g.ppp;
    let rawy1 = (g.canvas_min.y + g.ruler_h + (g.top - key + 0.5) * g.sy) * g.ppp;

    let keep = vec4<u32>(
        keep01(raw0 >= -LIM),
        keep01(raw1 <= LIM),
        keep01(rawy0 >= -LIM),
        keep01(rawy1 <= LIM),
    );

    // Matches the painter path: round to whole physical pixels, width / height at least 1 pixel
    var x0 = round(clamp(raw0, -LIM, LIM));
    var x1 = round(clamp(raw1, -LIM, LIM));
    var y0 = round(clamp(rawy0, -LIM, LIM));
    var y1 = round(clamp(rawy1, -LIM, LIM));
    x1 = max(x1, x0 + 1.0);
    y1 = max(y1, y0 + 1.0);

    // Triangle strip: 0=(0,0) 1=(1,0) 2=(0,1) 3=(1,1)
    let u = f32(vi & 1u);
    let v = f32((vi >> 1u) & 1u);
    let x = mix(x0, x1, u);
    let y = mix(y0, y1, v);
    let ndc = vec2<f32>(
        2.0 * x / g.screen_px.x - 1.0,
        1.0 - 2.0 * y / g.screen_px.y,
    );

    var out: VOut;
    out.pos = vec4<f32>(ndc, 0.0, 1.0);
    out.inst = inst;
    out.local = vec2<f32>(x - x0, y - y0);
    out.size = vec2<f32>(x1 - x0, y1 - y0);
    out.keep = keep;
    return out;
}

// 0-1 sRGB gamma ← 0-1 linear (converted manually when the target buffer is not sRGB)
fn gamma_from_linear(rgb: vec3<f32>) -> vec3<f32> {
    let cutoff = rgb < vec3<f32>(0.0031308);
    let lower = rgb * 12.92;
    let higher = 1.055 * pow(rgb, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(higher, lower, cutoff);
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    let m = in.inst.z;
    let vel = f32((m >> 8u) & 0xffu);
    let slot = (m >> 16u) & 0xffu;
    let layer = (m >> 24u) & 0xffu;

    // layer 1 = notes of the selected shape, all using the last palette slot (SELECTED_COLOR)
    let idx = select(slot % 15u, 15u, layer == 1u);
    let base = g.fill[idx].rgb;
    let outline = g.outline[idx].rgb;

    // Matches roll.rs::fade: the higher the velocity, the closer the note color is to the base color
    let level = min(vel, 127.0) / 4.0;
    let amount = 1.0 - level * 4.0 / 124.0;
    var color = mix(base, vec3<f32>(1.0), amount);

    // 1-logical-point (= ppp physical pixels) inner outline; clamped / off-screen edges are not drawn
    let d = vec2<f32>(
        min(
            select(1e9, in.local.x, in.keep.x == 1u),
            select(1e9, in.size.x - in.local.x, in.keep.y == 1u),
        ),
        min(
            select(1e9, in.local.y, in.keep.z == 1u),
            select(1e9, in.size.y - in.local.y, in.keep.w == 1u),
        ),
    );
    if min(d.x, d.y) < g.ppp {
        color = outline;
    }

    let out_rgb = select(gamma_from_linear(color), color, g.srgb == 1u);
    return vec4<f32>(out_rgb, 1.0);
}
