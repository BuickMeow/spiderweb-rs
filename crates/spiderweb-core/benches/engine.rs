//! 手写微基准：输出 libtest 风格 `test NAME ... bench: N ns/iter` 行，
//! 供 CI 的 benchmark-action（tool: cargo）解析并画性能折线。
//!
//! 运行：cargo bench -p spiderweb-core --bench engine

use std::hint::black_box;
use std::time::Instant;

use spiderweb_core::Pt;
use spiderweb_core::engine::{self, Mode, Split};
use spiderweb_core::shape::{
    Align, Fill, FunnelCurve, FunnelStart, Kind, Shape, Stroke, Tumour, TumourShape, TumourSide,
};

const PPQ: f64 = 960.0;
const GATE: f64 = 0.0625;

fn bench(name: &str, iters: u32, mut f: impl FnMut() -> usize) {
    let mut n = 0usize;
    for _ in 0..2 {
        n = black_box(f());
    }
    let start = Instant::now();
    for _ in 0..iters {
        n = black_box(f());
    }
    let ns = start.elapsed().as_nanos() as f64 / f64::from(iters);
    println!("test {name} ... bench: {ns:.0} ns/iter (+/- 0)");
    eprintln!("  ({name}: {n} notes)");
}

/// 一条跨越 64 个键的斜线。
fn line_shape(tumour: bool) -> Shape {
    let mut sh = engine::make_shape(
        Kind::Line,
        &[[0.0, 40.0], [256.0, 104.0]],
        &Shape::default(),
    );
    if tumour {
        sh.tumour = Some(Tumour {
            on: true,
            shape: TumourShape::Triangle,
            size: 4.0,
            length: 0.25,
            dist: 0.25,
            side: TumourSide::Alt,
            fit: true,
            seed: 7,
            k: 0.25,
            ..Tumour::default()
        });
    }
    sh
}

/// 覆盖 128 键的 spam 矩形，约 `notes` 个音符。
fn spam_shape(notes: i64) -> Shape {
    let per_key = (notes / 128).max(1) as f64;
    let span = per_key * GATE;
    let box_stroke = vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0], [0.0, 0.0]];
    Shape {
        kind: Kind::Custom,
        pts: vec![[0.0, -0.5], [span, -0.5], [0.0, 127.5]],
        name: "bench spam".to_string(),
        strokes: vec![Stroke::Poly {
            pts: box_stroke,
            free: false,
            smooth: 0,
            k: 1.0,
        }],
        fill: Fill::Spam,
        gate: GATE,
        align: Align::Aligned,
        ..Shape::default()
    }
}

/// 一个 spam 漏斗：从一条线张开到墙，约 `notes` 个音符。
fn funnel_shape(notes: i64) -> Shape {
    let span = (notes as f64 / 128.0).max(4.0);
    let curve = FunnelCurve {
        pts: vec![[0.0, 0.0], [0.7, 0.06], [0.94, 0.3], [1.0, 1.0]],
        sharp: Vec::new(),
        link: None,
        flip: false,
    };
    Shape {
        kind: Kind::Funnel,
        pts: vec![[0.0, 60.0], [span, 60.0], [span, 20.0], [span, 100.0]],
        starts: vec![FunnelStart {
            line: 0,
            at: 0.0,
            ends: [Some(curve.clone()), Some(curve)],
        }],
        funnel_fill: spiderweb_core::shape::FunnelFill::Spam,
        gate0: GATE,
        gate1: GATE,
        ..Shape::default()
    }
}

fn note_count(sh: &Shape) -> usize {
    engine::shape_notes_default(sh, PPQ).len()
}

fn main() {
    let line = line_shape(false);
    let tumour = line_shape(true);
    let spam = spam_shape(100_000);
    let funnel = funnel_shape(100_000);
    let spam_notes: Vec<Pt> = Vec::new();
    let _ = spam_notes;

    bench("line_notes_64keys", 200, || note_count(&line));
    bench("tumour_line_64keys", 20, || note_count(&tumour));
    bench("custom_spam_100k", 10, || note_count(&spam));
    bench("funnel_spam_100k", 10, || note_count(&funnel));

    // render：把 spam 的音符跑一遍重叠处理 + 通道分配
    let notes = engine::shape_notes_default(&spam, PPQ);
    bench("render_single_100k", 10, || {
        let (rendered, _) =
            engine::render(std::slice::from_ref(&notes), Mode::Single, Split::Key, None);
        rendered.len()
    });
}
