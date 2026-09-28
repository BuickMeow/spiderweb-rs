// 卷帘音符实例化渲染（一遍 instanced draw，不做 cull / LOD）。
//
// 每实例 16 字节（@location(0)）= (start_tick, end_tick, meta, pad)，
// meta = key | vel << 8 | slot << 16 | layer << 24。
// 平移 / 缩放只改 uniform；instance 只在音符或选择变化时重传。

struct Globals {
    canvas_min: vec2<f32>, // 画布左上角（逻辑点，屏幕坐标）
    ppp: f32,              // 物理像素 / 逻辑点
    kb_w: f32,             // 键盘宽度（逻辑点，对应 View::kb_w）
    origin_tick: u32,      // 视图左边缘 tick
    _pad0: u32,
    px_per_tick: f32, // 每 tick 的逻辑点数 = sx / ppq
    top: f32,         // 视图顶端的音高（View::top）
    sy: f32,          // 每行音高的逻辑点数（View::sy）
    ruler_h: f32,     // 标尺高度（逻辑点，对应 View::ruler_h）
    screen_px: vec2<f32>, // 屏幕尺寸（物理像素）
    srgb: u32,            // 目标格式是否 sRGB：1 = 直接输出线性色
    _pad1: u32,
    _pad2: vec2<u32>,
    fill: array<vec4<f32>, 16>,    // 线性填充色：15 个槽位 + 选中色
    outline: array<vec4<f32>, 16>, // 线性描边色：同上
};

@group(0) @binding(0) var<uniform> g: Globals;

struct VOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) @interpolate(flat) inst: vec4<u32>,
    // 相对矩形左上角的物理像素坐标 / 矩形物理像素尺寸
    @location(1) @interpolate(linear) local: vec2<f32>,
    @location(2) @interpolate(linear) size: vec2<f32>,
    // 左、右、上、下四条边是否没被夹出可见范围（夹过就不画那条描边）
    @location(3) @interpolate(flat) keep: vec4<u32>,
};

// 把远处坐标夹到这个范围（2^20 物理像素，远超任何窗口），
// 避免 f32 大数相减时的精度丢失；被夹掉的那条边不再画描边。
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

    // u32 环绕减法再按补码读成 i32：即使 origin_tick 很大，
    // (tick - origin_tick) 的差仍是精确的小整数，转 f32 不丢精度。
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

    // 与 painter 路径一致：round 到整物理像素，宽 / 高至少 1 像素
    var x0 = round(clamp(raw0, -LIM, LIM));
    var x1 = round(clamp(raw1, -LIM, LIM));
    var y0 = round(clamp(rawy0, -LIM, LIM));
    var y1 = round(clamp(rawy1, -LIM, LIM));
    x1 = max(x1, x0 + 1.0);
    y1 = max(y1, y0 + 1.0);

    // 三角形带：0=(0,0) 1=(1,0) 2=(0,1) 3=(1,1)
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

// 0-1 sRGB gamma ← 0-1 线性（目标缓冲不是 sRGB 时手动转）
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

    // layer 1 = 选中形状的音符，统一用调色板最后一组（SELECTED_COLOR）
    let idx = select(slot % 15u, 15u, layer == 1u);
    let base = g.fill[idx].rgb;
    let outline = g.outline[idx].rgb;

    // 与 roll.rs::fade 一致：力度越大力色越接近原色
    let level = min(vel, 127.0) / 4.0;
    let amount = 1.0 - level * 4.0 / 124.0;
    var color = mix(base, vec3<f32>(1.0), amount);

    // 1 个逻辑点（= ppp 物理像素）的内描边；夹掉 / 在视口外的边不画
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
