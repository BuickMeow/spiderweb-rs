//! Spiderweb 核心：形状 → 音符的纯算法层（不含任何 UI / 平台依赖）。
//!
//! 与 Python 原版逐模块对应：
//! - [`paths`] 线条 / 折线 / 自由笔 / 曲线 → 音符
//! - [`bezier`] 贝塞尔曲线采样、编辑与拟合
//! - [`arc`] 三点圆弧
//! - [`smooth`] 自由笔的"画整齐"（直线 / 平滑曲线 / 完美图形）
//! - [`tumour`] 线条上的肿瘤（凸起）
//! - [`envelope`] 速度包络
//! - [`custom`] 自定义形状（轮廓 / 填充 / spam）与粘贴音符
//! - [`joined`] joining lines / curves / arcs into one Curve shape (1.2.0)
//! - [`funnel`] 漏斗
//! - [`text`] 文本 → 字形轮廓
//! - [`engine`] 汇总：形状 → 音符、重叠处理、通道分配

pub mod arc;
pub mod bezier;
pub mod custom;
pub mod engine;
pub mod envelope;
pub mod fonts;
pub mod funnel;
pub mod joined;
pub mod paths;
pub mod pyrandom;
pub mod shape;
pub mod smooth;
pub mod text;
pub mod tumour;

/// 形状点：(beat, pitch)，均为浮点。
pub type Pt = [f64; 2];

/// (start, end, key) 音符行，tick 为整数。
pub type Note3 = [i64; 3];

/// (start, end, key, velocity) 音符行。
pub type Note4 = [i64; 4];

/// (start, end, key, velocity, slot, owner) 音符行（engine.render 的最终形态）。
pub type Note6 = [i64; 6];

/// CPython 3.9 `math.hypot` 的复刻：按最大分量缩放后用 Neumaier 补偿求和
/// （见 CPython Modules/mathmodule.c 的 vector_norm；普通 sqrt(x²+y²) 会有 ulp 偏差，
/// 而对照向量要求逐位一致）。
pub fn hypot2(x0: f64, x1: f64) -> f64 {
    let x0 = x0.abs();
    let x1 = x1.abs();
    let mut max = 0.0;
    if x0 > max {
        max = x0;
    }
    if x1 > max {
        max = x1;
    }
    if max.is_infinite() {
        return max;
    }
    if x0.is_nan() || x1.is_nan() {
        return f64::NAN;
    }
    if max == 0.0 {
        return max;
    }
    let (mut csum, mut frac) = (1.0, 0.0);
    for x in [x0, x1] {
        let x = x / max;
        let x = x * x;
        let oldcsum = csum;
        csum += x;
        frac += (oldcsum - csum) + x;
    }
    max * (csum - 1.0 + frac).sqrt()
}

/// 两点距离（CPython `math.dist` 同款）。
pub fn dist(a: Pt, b: Pt) -> f64 {
    hypot2(a[0] - b[0], a[1] - b[1])
}

/// Python `round()` / NumPy `round()` 的银行家舍入（.5 取偶）。
pub fn round_half_even(x: f64) -> f64 {
    let f = x.floor();
    let frac = x - f;
    if frac == 0.5 {
        if (f as i64) % 2 == 0 { f } else { f + 1.0 }
    } else {
        x.round()
    }
}

/// Python `round()` 到 i64。
pub fn round_i64(x: f64) -> i64 {
    round_half_even(x) as i64
}

/// Python `math.floor(x + 0.5)`：tick 取整的通用写法。
pub fn floor_half(x: f64) -> i64 {
    (x + 0.5).floor() as i64
}
