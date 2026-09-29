//! `cargo xtask bench-project`: a port of `tools/gen_bench_project.py`.
//!
//! The project is built with the real `spiderweb-core` shape types and written
//! with `spiderweb-io`, so the output is guaranteed to be a valid project file.

use spiderweb_core::shape::{
    Align, Fill, FunnelCurve, FunnelStart, Kind, Shape, Stroke, Tumour, TumourShape, TumourSide,
};
use spiderweb_io::project::{CustomDefaults, FunnelDefaults, Project};

const PPQ: &str = "960";
const GATE: f64 = 0.0625; // a 1/64 note
const KEYS: i64 = 128;

fn spam_shape(name: &str, notes: i64, start_beat: f64) -> (Shape, f64) {
    let per_key = (notes / KEYS).max(1) as f64;
    let span = per_key * GATE;
    let box_stroke = vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0], [0.0, 0.0]];
    let sh = Shape {
        kind: Kind::Custom,
        pts: vec![
            [start_beat, -0.5],
            [start_beat + span, -0.5],
            [start_beat, -0.5 + KEYS as f64],
        ],
        name: name.to_string(),
        strokes: vec![Stroke::Poly {
            pts: box_stroke,
            free: false,
            smooth: 0,
            k: 1.0,
            src: None,
        }],
        fill: Fill::Spam,
        gate: GATE,
        align: Align::Aligned,
        ..Shape::default()
    };
    (sh, span)
}

fn tumour_line(start_beat: f64, span: f64) -> Shape {
    Shape {
        kind: Kind::Line,
        pts: vec![[start_beat, 40.0], [start_beat + span, 104.0]],
        tumour: Some(Tumour {
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
        }),
        ..Shape::default()
    }
}

fn funnel_shape(start_beat: f64, span: f64) -> Shape {
    let curve = FunnelCurve {
        pts: vec![[0.0, 0.0], [0.7, 0.06], [0.94, 0.3], [1.0, 1.0]],
        sharp: Vec::new(),
        link: None,
        flip: false,
    };
    Shape {
        kind: Kind::Funnel,
        pts: vec![
            [start_beat, 60.0],
            [start_beat + span, 60.0],
            [start_beat + span, 20.0],
            [start_beat + span, 100.0],
        ],
        starts: vec![FunnelStart {
            line: 0,
            at: 0.0,
            ends: [Some(curve.clone()), Some(curve)],
        }],
        gate0: GATE,
        gate1: GATE,
        ..Shape::default()
    }
}

pub fn run(args: &[String]) -> i32 {
    let mut notes = 1_000_000i64;
    let mut out = "bench.json".to_string();
    let mut mix = true;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--notes" => {
                i += 1;
                match args.get(i).and_then(|v| v.parse::<i64>().ok()) {
                    Some(n) if n > 0 => notes = n,
                    _ => {
                        eprintln!("--notes needs a positive number");
                        return 1;
                    }
                }
            }
            "-o" | "--out" => {
                i += 1;
                match args.get(i) {
                    Some(v) => out = v.clone(),
                    None => {
                        eprintln!("-o needs a path");
                        return 1;
                    }
                }
            }
            "--no-mix" => mix = false,
            other => {
                eprintln!("unknown argument: {other}");
                return 1;
            }
        }
        i += 1;
    }

    let (spam, span) = spam_shape("Bench spam", notes, 0.0);
    let mut shapes = vec![spam];
    if mix {
        shapes.push(tumour_line(0.0, span));
        shapes.push(funnel_shape(span * 0.2, span * 0.6));
    }

    let mut project = Project {
        ppq: PPQ.to_string(),
        bpm: "120".to_string(),
        beats: "4".to_string(),
        shapes,
        ..Project::default()
    };
    project.custom_defaults = CustomDefaults {
        gate: GATE,
        align: Align::Aligned,
        fill: Fill::Spam,
        ..project.custom_defaults
    };
    project.funnel_defaults = FunnelDefaults {
        gate0: GATE,
        gate1: GATE,
        ..project.funnel_defaults
    };

    let path = std::path::Path::new(&out);
    if let Err(e) = project.write(path, None) {
        eprintln!("could not write {out}: {e}");
        return 1;
    }
    println!(
        "wrote {out}: {} shapes, spam span {span:.0} beats ({:.0} bars), target ~{notes} notes",
        project.shapes.len(),
        span / 4.0
    );
    0
}
