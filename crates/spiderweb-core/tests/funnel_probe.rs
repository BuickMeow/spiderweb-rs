use spiderweb_core::engine;
use spiderweb_core::funnel;
use spiderweb_core::shape::{FunnelFill, FunnelStart, Kind, Shape};

fn count(pts: Vec<[f64; 2]>, at: f64) -> usize {
    let c = funnel::new_curve(None);
    let sh = Shape {
        kind: Kind::Funnel,
        pts,
        starts: vec![FunnelStart {
            line: 0,
            at,
            ends: [Some(c.clone()), Some(c)],
        }],
        funnel_fill: FunnelFill::Spam,
        gate0: 0.0625,
        gate1: 0.0625,
        ..Shape::default()
    };
    engine::shape_notes(&sh, 960.0, 128).len()
}

#[test]
fn probe() {
    // post-extension shape from the screenshot
    println!(
        "post  at 0.0: {}",
        count(
            vec![[2.25, 93.], [6.75, 88.], [3.5, 119.], [3.25, 63.]],
            0.0
        )
    );
    println!(
        "post  at 0.2: {}",
        count(
            vec![[2.25, 93.], [6.75, 88.], [3.5, 119.], [3.25, 63.]],
            0.2
        )
    );
    // line at 1/4 length (before stretching), wall unchanged
    println!(
        "quarter at 0.0: {}",
        count(
            vec![[2.25, 93.], [3.375, 88.], [3.5, 119.], [3.25, 63.]],
            0.0
        )
    );
    // line ending at the wall
    println!(
        "wall at 0.0: {}",
        count(vec![[2.25, 93.], [3.5, 88.], [3.5, 119.], [3.25, 63.]], 0.0)
    );
}
