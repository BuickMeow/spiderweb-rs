//! 速度包络（Python notes/envelope.py）：逐点求值、批量求值、涂改、清理。

use crate::Pt;
use crate::shape::Shape;

/// 包络里同一 u 上两个点的判定容差（Python ENV_EPS）。
pub const ENV_EPS: f64 = 1e-9;

/// 形状的速度包络；没有自定义包络时是 vel0 → vel1 的直线（envelope.velocity_env）。
pub fn velocity_env(sh: &Shape) -> Vec<Pt> {
    if sh.vel_env.is_empty() {
        vec![[0.0, sh.vel0], [1.0, sh.vel1]]
    } else {
        sh.vel_env.clone()
    }
}

/// u 处的包络值（envelope.env_at）。`left`：取 u 左侧（跳变前）的值。
pub fn env_at(env: &[Pt], u: f64, left: bool) -> f64 {
    // bisect_left / bisect_right on the u column
    let i = if left {
        env.partition_point(|p| p[0] < u)
    } else {
        env.partition_point(|p| p[0] <= u)
    };
    if i == 0 {
        return env[0][1];
    }
    if i == env.len() {
        return env[env.len() - 1][1];
    }
    let (u0, v0) = (env[i - 1][0], env[i - 1][1]);
    let (u1, v1) = (env[i][0], env[i][1]);
    if u1 == u0 { v0 } else { v0 + (v1 - v0) * (u - u0) / (u1 - u0) }
}

/// 一批 u 的包络值（envelope.env_values），与逐点调用同结果。
pub fn env_values(env: &[Pt], us: &[f64]) -> Vec<f64> {
    us.iter().map(|&u| env_at(env, u, false)).collect()
}

/// 用 pts（按 u 排序）替换 env 在 pts[0] 与 pts[-1] 之间的部分，两端之外原样保留（envelope.paint_env）。
pub fn paint_env(env: &[Pt], pts: &[Pt]) -> Vec<Pt> {
    let ua = pts[0][0];
    let ub = pts[pts.len() - 1][0] + ENV_EPS;
    let before = env_at(env, ua, true);
    let after = env_at(env, ub, false);
    let mut out: Vec<Pt> = env.iter().copied().filter(|p| p[0] < ua).collect();
    out.push([ua, before]);
    out.extend_from_slice(pts);
    out.push([ub, after]);
    out.extend(env.iter().copied().filter(|p| p[0] > ub));
    out
}

/// 去掉 0..1 之外无关的点、重复点、直线段中间的点（envelope.tidy_env）。
pub fn tidy_env(env: &[Pt]) -> Vec<Pt> {
    let inside: Vec<usize> = (0..env.len()).filter(|&i| (0.0..=1.0).contains(&env[i][0])).collect();
    let (lo, hi) = if inside.is_empty() {
        (0, env.len())
    } else {
        (
            inside[0].saturating_sub(1),
            (inside[inside.len() - 1] + 2).min(env.len()),
        )
    };
    let mut out: Vec<Pt> = Vec::new();
    for &p in &env[lo..hi] {
        if let Some(&last) = out.last()
            && p == last
        {
            continue;
        }
        if out.len() >= 2 {
            let (u0, v0) = (out[out.len() - 2][0], out[out.len() - 2][1]);
            let (u1, v1) = (out[out.len() - 1][0], out[out.len() - 1][1]);
            if u0 == u1 && u1 == p[0] {
                *out.last_mut().unwrap() = p;
                continue;
            }
            if u0 < u1 && u1 < p[0] && (v0 + (p[1] - v0) * (u1 - u0) / (p[0] - u0) - v1).abs() < 1e-7 {
                *out.last_mut().unwrap() = p;
                continue;
            }
        }
        out.push(p);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env() -> Vec<Pt> {
        vec![[0.0, 10.0], [0.5, 20.0], [0.5, 40.0], [1.0, 100.0]]
    }

    #[test]
    fn env_at_jumps_take_right_side() {
        let e = env();
        assert_eq!(env_at(&e, 0.25, false), 15.0);
        assert_eq!(env_at(&e, 0.5, false), 40.0); // 跳变取右侧
        assert_eq!(env_at(&e, 0.5, true), 20.0); // left 取左侧
        assert_eq!(env_at(&e, -1.0, false), 10.0);
        assert_eq!(env_at(&e, 2.0, false), 100.0);
    }

    #[test]
    fn tidy_drops_middle_and_repeats() {
        let e = vec![[0.0, 0.0], [0.5, 50.0], [1.0, 100.0], [1.0, 100.0]];
        assert_eq!(tidy_env(&e), vec![[0.0, 0.0], [1.0, 100.0]]);
    }

    #[test]
    fn paint_keeps_outside() {
        let e = vec![[0.0, 0.0], [0.25, 10.0], [0.75, 30.0], [1.0, 40.0]];
        let painted = paint_env(&e, &[[0.4, 5.0], [0.6, 6.0]]);
        assert_eq!(painted[0], [0.0, 0.0]);
        assert_eq!(painted[1], [0.25, 10.0]);
        assert_eq!(painted[2], [0.4, 16.0]); // 插入点在 0.4 处的原值（左边值）
        assert_eq!(painted[3], [0.4, 5.0]);
        assert_eq!(painted[4], [0.6, 6.0]);
        assert!(painted[5][0] > 0.6 && painted[5][0] < 0.6 + 1e-6); // ub = 0.6 + ENV_EPS
        assert_eq!(painted[6], [0.75, 30.0]);
        assert_eq!(*painted.last().unwrap(), [1.0, 40.0]);
    }
}
