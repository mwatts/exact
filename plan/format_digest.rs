//! `FORMAT_DIGEST` from `tables/format.json`, shared by `build.rs` and the
//! format tests.
//!
//! The digest is taken over the table printed with every object's keys in
//! byte order, not as `serde_json` prints a `Value`. That printing follows
//! the `preserve_order` feature, which Cargo unifies across a build, so a
//! host whose build turns it on would compute another digest for the same
//! table and refuse every plan the stock compiler writes. Byte order is what
//! `serde_json` prints without the feature, so the digest is unchanged.

use serde_json::Value;
use sha2::{Digest, Sha256};

/// `value` as compact JSON with every object's keys in byte order.
pub fn canonical(value: &Value) -> String {
    let mut out = String::new();
    write(value, &mut out);
    out
}

fn write(value: &Value, out: &mut String) {
    match value {
        Value::Object(map) => {
            let mut entries: Vec<_> = map.iter().collect();
            entries.sort_unstable_by(|a, b| a.0.cmp(b.0));
            out.push('{');
            for (i, (key, item)) in entries.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&serde_json::to_string(key).unwrap());
                out.push(':');
                write(item, out);
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write(item, out);
            }
            out.push(']');
        }
        scalar => out.push_str(&serde_json::to_string(scalar).unwrap()),
    }
}

/// Domain-separated SHA-256 (first 8 bytes, little-endian) of the canonical
/// format table.
pub fn digest(table: &Value) -> u64 {
    let mut h = Sha256::new();
    h.update(b"EXACT.PLAN.FORMAT.SHA256.V1\0");
    h.update(canonical(table).as_bytes());
    let out = h.finalize();
    u64::from_le_bytes(out[..8].try_into().unwrap())
}
