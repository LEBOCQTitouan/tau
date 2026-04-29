//! `tau plugin protocol decode <path>` — render a JSONL recording.
//!
//! Per spec §9 (debug tier): each line of the input file is one frame
//! captured by `--record-protocol` (see `tau-runtime::plugin_host::recording`).
//! The decoder:
//!
//! 1. Parses the line as JSON.
//! 2. Base64-decodes the `frame` field.
//! 3. MessagePack-decodes the resulting bytes via [`Frame::decode`] to
//!    surface the inner request / response / notification body as
//!    JSON-shaped MessagePack content.
//! 4. Emits the decoded shape — either a human-readable transcript or
//!    one machine-readable JSON object per line (`--json`).
//!
//! Filters (`--filter key=value`), time range (`--from` / `--to`), and
//! `--json` are honored before emission.

use std::collections::BTreeMap;

use anyhow::{anyhow, Context};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use tau_plugin_protocol::Frame;

use crate::cli::PluginProtocolDecodeArgs;
use crate::output::Output;

/// Run `tau plugin protocol decode`.
pub async fn run(args: &PluginProtocolDecodeArgs, output: &mut Output) -> anyhow::Result<()> {
    let filters = parse_filters(&args.filter)?;

    let contents = tokio::fs::read_to_string(&args.path)
        .await
        .with_context(|| format!("reading recording {}", args.path.display()))?;

    // The base timestamp anchors the human transcript's `[+1.234s]`
    // column to the first frame in range, so transcripts are
    // self-contained and don't drag wall-clock noise into review.
    let mut base_ts: Option<f64> = None;

    for (lineno, line) in contents.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let raw: serde_json::Value = serde_json::from_str(line).with_context(|| {
            format!(
                "parsing line {} of {} as JSON",
                lineno + 1,
                args.path.display()
            )
        })?;

        let ts = raw.get("ts").and_then(serde_json::Value::as_f64);
        let plugin = raw
            .get("plugin")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("?");
        let dir = raw
            .get("dir")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("?");
        let msgid = raw.get("msgid").and_then(serde_json::Value::as_u64);
        let method = raw.get("method").and_then(serde_json::Value::as_str);
        let frame_b64 = raw.get("frame").and_then(serde_json::Value::as_str);

        // ---- Time range ----
        if let Some(t) = ts {
            if let Some(from) = args.from {
                if t < from {
                    continue;
                }
            }
            if let Some(to) = args.to {
                if t > to {
                    continue;
                }
            }
        }

        // ---- Predicate filters ----
        if !matches_filters(&filters, plugin, dir, method) {
            continue;
        }

        // ---- Frame decode ----
        let decoded = match frame_b64 {
            Some(s) => match B64.decode(s) {
                Ok(bytes) => decode_inner(&bytes),
                Err(e) => Err(anyhow!("base64 decode: {e}")),
            },
            None => Err(anyhow!("missing `frame` field")),
        };

        // ---- Emission ----
        let rel_ts = ts.map(|t| {
            let base = *base_ts.get_or_insert(t);
            t - base
        });

        if args.json {
            let payload = serde_json::json!({
                "ts": ts,
                "rel_ts": rel_ts,
                "plugin": plugin,
                "dir": dir,
                "msgid": msgid,
                "method": method,
                "decoded": match &decoded {
                    Ok(v) => v.clone(),
                    Err(e) => serde_json::json!({ "decode_error": format!("{e}") }),
                },
            });
            output.json(&payload)?;
        } else {
            let ts_col = match rel_ts {
                Some(t) => format!("[+{t:>6.3}s]"),
                None => "[ ?     ]".to_string(),
            };
            let header = match (msgid, method) {
                (Some(id), Some(m)) => format!("{ts_col} {plugin:<16} {dir} msgid={id} {m}"),
                (Some(id), None) => format!("{ts_col} {plugin:<16} {dir} msgid={id}"),
                (None, Some(m)) => format!("{ts_col} {plugin:<16} {dir} {m}"),
                (None, None) => format!("{ts_col} {plugin:<16} {dir}"),
            };
            output.human(&header)?;
            match decoded {
                Ok(v) => {
                    let pretty = serde_json::to_string_pretty(&v).unwrap_or_default();
                    for ln in pretty.lines() {
                        output.human(&format!("  {ln}"))?;
                    }
                }
                Err(e) => {
                    output.human(&format!("  <decode error: {e}>"))?;
                }
            }
        }
    }

    Ok(())
}

/// Parse `--filter k=v` arguments into a map. Multiple values for the
/// same key are not currently supported; the last write wins (kept
/// simple for v0.1; only one value per key is meaningful for the
/// supported keys `plugin`, `method`, `dir`).
fn parse_filters(raw: &[String]) -> anyhow::Result<BTreeMap<String, String>> {
    let mut out = BTreeMap::new();
    for entry in raw {
        let (k, v) = entry
            .split_once('=')
            .ok_or_else(|| anyhow!("invalid --filter {entry:?}: expected `key=value`"))?;
        out.insert(k.to_string(), v.to_string());
    }
    Ok(out)
}

/// Test a frame's metadata against the user-supplied predicates.
/// Supported keys: `plugin`, `dir`, `method`.
fn matches_filters(
    filters: &BTreeMap<String, String>,
    plugin: &str,
    dir: &str,
    method: Option<&str>,
) -> bool {
    for (k, v) in filters {
        let actual = match k.as_str() {
            "plugin" => plugin,
            "dir" => dir,
            "method" => method.unwrap_or(""),
            // Unknown keys are not a hard error: log-and-include keeps
            // the decoder permissive against future recording fields.
            _ => continue,
        };
        if actual != v {
            return false;
        }
    }
    true
}

/// Decode the inner [`Frame`] from raw MessagePack bytes and project
/// it to a JSON-friendly summary.
fn decode_inner(bytes: &[u8]) -> anyhow::Result<serde_json::Value> {
    let frame = Frame::decode(bytes).context("decoding inner Frame")?;
    let json = match frame {
        Frame::Request { id, method, params } => serde_json::json!({
            "kind": "request",
            "id": id,
            "method": method,
            "params": decode_msgpack_to_json(&params),
        }),
        Frame::Response { id, error, result } => {
            let err_json = error.map(|e| {
                serde_json::json!({
                    "code": e.code,
                    "message": e.message,
                })
            });
            let result_json = result.as_deref().map(decode_msgpack_to_json);
            serde_json::json!({
                "kind": "response",
                "id": id,
                "error": err_json,
                "result": result_json,
            })
        }
        Frame::Notification { method, params } => serde_json::json!({
            "kind": "notification",
            "method": method,
            "params": decode_msgpack_to_json(&params),
        }),
        // Frame is `#[non_exhaustive]`; future variants summarise
        // generically rather than panicking.
        _ => serde_json::json!({ "kind": "unknown" }),
    };
    Ok(json)
}

/// Decode a MessagePack-encoded byte slice to a `serde_json::Value`,
/// degrading on decode failure to a placeholder string. Used for the
/// human-friendly `params` / `result` projection only — the lossless
/// representation is the original base64 in the JSONL line.
fn decode_msgpack_to_json(bytes: &[u8]) -> serde_json::Value {
    let value: rmpv::Value = match rmpv::decode::read_value(&mut &bytes[..]) {
        Ok(v) => v,
        Err(_) => {
            return serde_json::json!(format!("<msgpack decode error: {} bytes>", bytes.len()))
        }
    };
    msgpack_value_to_json(&value)
}

fn msgpack_value_to_json(value: &rmpv::Value) -> serde_json::Value {
    match value {
        rmpv::Value::Nil => serde_json::Value::Null,
        rmpv::Value::Boolean(b) => serde_json::Value::Bool(*b),
        rmpv::Value::Integer(i) => {
            if let Some(n) = i.as_i64() {
                serde_json::Value::Number(n.into())
            } else if let Some(n) = i.as_u64() {
                serde_json::Value::Number(n.into())
            } else {
                serde_json::Value::String(format!("{i:?}"))
            }
        }
        rmpv::Value::F32(f) => serde_json::Number::from_f64((*f).into())
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        rmpv::Value::F64(f) => serde_json::Number::from_f64(*f)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        rmpv::Value::String(s) => match s.as_str() {
            Some(s) => serde_json::Value::String(s.to_string()),
            None => serde_json::Value::String(format!("{s:?}")),
        },
        rmpv::Value::Binary(b) => serde_json::Value::String(format!("<{}b binary>", b.len())),
        rmpv::Value::Array(a) => {
            serde_json::Value::Array(a.iter().map(msgpack_value_to_json).collect())
        }
        rmpv::Value::Map(m) => {
            let mut obj = serde_json::Map::with_capacity(m.len());
            for (k, v) in m {
                let key = match k {
                    rmpv::Value::String(s) => s.as_str().unwrap_or("<binary>").to_string(),
                    other => format!("{other:?}"),
                };
                obj.insert(key, msgpack_value_to_json(v));
            }
            serde_json::Value::Object(obj)
        }
        rmpv::Value::Ext(tag, _bytes) => serde_json::Value::String(format!("<ext tag={tag}>")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_filters_accepts_kv_pairs() {
        let f = parse_filters(&["plugin=echo".into(), "dir=h2p".into()]).unwrap();
        assert_eq!(f.get("plugin").map(String::as_str), Some("echo"));
        assert_eq!(f.get("dir").map(String::as_str), Some("h2p"));
    }

    #[test]
    fn parse_filters_rejects_missing_equals() {
        let err = parse_filters(&["plugin".into()]).unwrap_err();
        assert!(format!("{err}").contains("expected `key=value`"));
    }

    #[test]
    fn matches_filters_passes_when_all_match() {
        let mut f = BTreeMap::new();
        f.insert("plugin".into(), "echo".into());
        assert!(matches_filters(&f, "echo", "h2p", Some("llm.complete")));
    }

    #[test]
    fn matches_filters_rejects_when_any_differs() {
        let mut f = BTreeMap::new();
        f.insert("plugin".into(), "echo".into());
        f.insert("dir".into(), "p2h".into());
        assert!(!matches_filters(&f, "echo", "h2p", Some("llm.complete")));
    }

    #[test]
    fn matches_filters_unknown_key_is_ignored_not_rejected() {
        let mut f = BTreeMap::new();
        f.insert("nonsense".into(), "value".into());
        assert!(matches_filters(&f, "echo", "h2p", None));
    }

    #[test]
    fn decode_inner_round_trips_request() {
        let frame = Frame::Request {
            id: 7,
            method: "llm.complete".into(),
            params: rmp_serde::to_vec(&serde_json::json!({"prompt": "hi"})).unwrap(),
        };
        let bytes = frame.encode().unwrap();
        let json = decode_inner(&bytes).unwrap();
        assert_eq!(json["kind"], "request");
        assert_eq!(json["id"], 7);
        assert_eq!(json["method"], "llm.complete");
        assert_eq!(json["params"]["prompt"], "hi");
    }

    #[test]
    fn decode_inner_handles_response_with_error() {
        let frame = Frame::Response {
            id: 1,
            error: Some(tau_plugin_protocol::RpcErrorEnvelope::new(
                -32601,
                "method not found".to_string(),
                None,
            )),
            result: None,
        };
        let bytes = frame.encode().unwrap();
        let json = decode_inner(&bytes).unwrap();
        assert_eq!(json["kind"], "response");
        assert_eq!(json["error"]["code"], -32601);
        assert_eq!(json["error"]["message"], "method not found");
    }
}
