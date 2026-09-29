//! `project.short_shape` differential test (vectors from tools/gen_io_vectors.py).
//!
//! The vector holds the shape JSON and the exact text Python's `short_shape` wrote for it
//! (12 significant digits, tumour graphs included). The port must produce the same numbers.

mod common;

use common::*;
use serde_json::Value;
use spiderweb_io::project::short_shape_value;

/// Canonical JSON text: keys sorted (both sides are serde_json maps) and numbers spelled the
/// Python way, so 1 vs 1.0 and 0.1 vs 0.1000000000000001 can't pass as "close enough".
fn canon(v: &Value) -> String {
    serde_json::to_string(v).expect("JSON")
}

#[test]
fn short_shape_vectors() {
    for case in cases("short", "cases") {
        let name = s(&case["name"]);
        let input = &case["input"];
        let want: Value = serde_json::from_str(s(&case["saved"])).expect("saved is not JSON");
        let got = short_shape_value(input);
        assert_eq!(canon(&got), canon(&want), "{name}: short_shape");
    }
}
