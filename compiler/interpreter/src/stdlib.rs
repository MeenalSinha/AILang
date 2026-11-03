//! Standard library builtins for AILang.
//!
//! Implements the flat, always-in-scope v0.1 functions from the language
//! spec: `print`, `println`, `input`, `len`, `type`, `exit`, `assert`,
//! `clock`, `to_string`, `parse_int`, `parse_float` — plus, as of v0.5,
//! the *pure* AI-oriented functions `summarize`, `translate`, `classify`,
//! and `embed`, which need no interpreter state and so live here
//! alongside the v0.1 builtins. The *stateful* v0.5 functions
//! (`remember`/`recall`/`forget`/`invoke_tool`/`run_workflow`/`run_team`,
//! which need access to the interpreter's memory store, environment, or
//! provider registry) are dispatched directly in
//! `Interpreter::call_native_with_io` instead — see that method's doc
//! comment.
//!
//! Each name is registered into the global [`Environment`] as a
//! [`Value::Native`] tagged with its name; the interpreter dispatches the
//! actual call here via [`call`] so builtins that need I/O (`print`,
//! `input`) can share the interpreter's output sink.

use std::io::{self, BufRead, Write};

use ailang_runtime::{Environment, Value};

use crate::now_seconds;

pub const NAMES: &[&str] = &[
    "print",
    "println",
    "input",
    "len",
    "type",
    "exit",
    "assert",
    "clock",
    "to_string",
    "parse_int",
    "parse_float",
    // v0.5 AI-oriented (pure — see module doc for the stateful ones,
    // which are registered the same way but dispatched elsewhere).
    "summarize",
    "translate",
    "classify",
    "embed",
    // v1.0: general standard library expansion.
    "push",
    "pop",
    "reverse",
    "sort",
    "json_encode",
    "json_decode",
    "read_file",
    "write_file",
    "http_get",
    "http_post",
    "hash",
    "sleep",
    // v0.5 AI-oriented (stateful — dispatched directly in
    // Interpreter::call_native_with_io, but still registered here so
    // `install()` binds them as callable Value::Native names).
    "remember",
    "recall",
    "forget",
    "invoke_tool",
    "run_workflow",
    "run_team",
    "render",
    // v1.0: agent/team-scoped memory + versioning.
    "memory_history",
    "remember_in",
    "recall_in",
    "forget_in",
    "retrieve",
    "secret",
    "audit_log",
    "send_message",
    "receive_message",
    "broadcast",
    "agent_location",
    "get_trace",
    // v2.0: distributed-intelligence constructs.
    "network_members",
    "network_leader",
    "network_health",
    "network_join",
    "network_leave",
    "cluster_status",
    "cluster_dispatch",
    "on_request",
    "request",
    "subscribe",
    "unsubscribe",
    "publish",
    "emit",
    "vector_index_add",
    "vector_index_search",
    "compress_context",
    "dedupe_chunks",
    "schedule",
    "run_scheduled",
    "agent_models",
];

pub fn install(env: &Environment) {
    for name in NAMES {
        env.define(name, Value::native(name), true);
    }
}

pub fn call<W: Write>(name: &str, args: &[Value], out: &mut W) -> Result<Value, String> {
    match name {
        "print" => {
            for a in args {
                write!(out, "{a}").map_err(|e| e.to_string())?;
            }
            Ok(Value::Void)
        }
        "println" => {
            for a in args {
                write!(out, "{a}").map_err(|e| e.to_string())?;
            }
            writeln!(out).map_err(|e| e.to_string())?;
            Ok(Value::Void)
        }
        "input" => {
            if let Some(prompt) = args.first() {
                write!(out, "{prompt}").map_err(|e| e.to_string())?;
                out.flush().map_err(|e| e.to_string())?;
            }
            let mut line = String::new();
            io::stdin()
                .lock()
                .read_line(&mut line)
                .map_err(|e| e.to_string())?;
            Ok(Value::string(
                line.trim_end_matches(['\n', '\r']).to_string(),
            ))
        }
        "len" => match args.first() {
            Some(Value::String(s)) => Ok(Value::Int(s.chars().count() as i64)),
            Some(Value::Array(a)) => Ok(Value::Int(a.borrow().len() as i64)),
            Some(Value::Map(m)) => Ok(Value::Int(m.borrow().len() as i64)),
            Some(other) => Err(format!("len() is not defined for `{}`", other.type_name())),
            None => Err("len() expects 1 argument".to_string()),
        },
        "type" => match args.first() {
            Some(v) => Ok(Value::string(v.type_name())),
            None => Err("type() expects 1 argument".to_string()),
        },
        "exit" => {
            let code = match args.first() {
                Some(Value::Int(n)) => *n as i32,
                _ => 0,
            };
            out.flush().ok();
            std::process::exit(code);
        }
        "assert" => {
            let ok = args.first().map(|v| v.is_truthy()).unwrap_or(false);
            if ok {
                Ok(Value::Void)
            } else {
                let msg = args
                    .get(1)
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "assertion failed".to_string());
                Err(msg)
            }
        }
        "clock" => Ok(Value::Float(now_seconds())),
        "to_string" => match args.first() {
            Some(v) => Ok(Value::string(v.to_string())),
            None => Err("to_string() expects 1 argument".to_string()),
        },
        "parse_int" => match args.first() {
            Some(Value::String(s)) => s
                .trim()
                .parse::<i64>()
                .map(Value::Int)
                .map_err(|_| format!("cannot parse \"{s}\" as int")),
            Some(other) => Err(format!(
                "parse_int() expects a string, got `{}`",
                other.type_name()
            )),
            None => Err("parse_int() expects 1 argument".to_string()),
        },
        "parse_float" => match args.first() {
            Some(Value::String(s)) => s
                .trim()
                .parse::<f64>()
                .map(Value::Float)
                .map_err(|_| format!("cannot parse \"{s}\" as float")),
            Some(other) => Err(format!(
                "parse_float() expects a string, got `{}`",
                other.type_name()
            )),
            None => Err("parse_float() expects 1 argument".to_string()),
        },
        // ---- v0.5 AI-oriented pure functions ----
        "summarize" => match args.first() {
            Some(Value::String(s)) => Ok(Value::string(summarize_text(s))),
            Some(other) => Err(format!(
                "summarize() expects a string, got `{}`",
                other.type_name()
            )),
            None => Err("summarize() expects 1 argument".to_string()),
        },
        "translate" => {
            let text = match args.first() {
                Some(Value::String(s)) => s.to_string(),
                _ => return Err("translate() expects (text: string, lang: string)".to_string()),
            };
            let lang = match args.get(1) {
                Some(Value::String(s)) => s.to_string(),
                _ => return Err("translate() expects (text: string, lang: string)".to_string()),
            };
            // v0.5 foundation: a real translation call needs a real model
            // provider (see ailang_ai_runtime::MockProvider's doc comment
            // for why this crate doesn't fabricate translations). The
            // language-level tag makes it unmistakable this isn't a real
            // translation, rather than silently returning wrong text.
            Ok(Value::string(format!("[{lang}] {text}")))
        }
        "classify" => {
            let text = match args.first() {
                Some(Value::String(s)) => s.to_string(),
                _ => {
                    return Err(
                        "classify() expects (text: string, categories: [string])".to_string()
                    )
                }
            };
            let categories = match args.get(1) {
                Some(Value::Array(a)) => a
                    .borrow()
                    .iter()
                    .map(|v| match v {
                        Value::String(s) => Ok(s.to_string()),
                        other => Err(format!(
                            "classify() categories must all be strings, found `{}`",
                            other.type_name()
                        )),
                    })
                    .collect::<Result<Vec<_>, _>>()?,
                _ => {
                    return Err(
                        "classify() expects (text: string, categories: [string])".to_string()
                    )
                }
            };
            match ailang_ai_runtime::classify(&text, &categories) {
                Some(category) => Ok(Value::string(category)),
                None => Ok(Value::Null),
            }
        }
        "embed" => match args.first() {
            Some(Value::String(s)) => {
                let vector = ailang_ai_runtime::embed(s);
                Ok(Value::array(vector.into_iter().map(Value::Float).collect()))
            }
            Some(other) => Err(format!(
                "embed() expects a string, got `{}`",
                other.type_name()
            )),
            None => Err("embed() expects 1 argument".to_string()),
        },
        // ---- v1.0 standard library expansion ----
        "push" => match args.first() {
            Some(Value::Array(a)) => {
                let value = args.get(1).cloned().unwrap_or(Value::Null);
                a.borrow_mut().push(value);
                Ok(Value::Int(a.borrow().len() as i64))
            }
            Some(other) => Err(format!(
                "push() expects an array, got `{}`",
                other.type_name()
            )),
            None => Err("push() expects (array, value)".to_string()),
        },
        "pop" => match args.first() {
            Some(Value::Array(a)) => Ok(a.borrow_mut().pop().unwrap_or(Value::Null)),
            Some(other) => Err(format!(
                "pop() expects an array, got `{}`",
                other.type_name()
            )),
            None => Err("pop() expects 1 argument".to_string()),
        },
        "reverse" => match args.first() {
            Some(Value::Array(a)) => {
                a.borrow_mut().reverse();
                Ok(Value::Void)
            }
            Some(other) => Err(format!(
                "reverse() expects an array, got `{}`",
                other.type_name()
            )),
            None => Err("reverse() expects 1 argument".to_string()),
        },
        "sort" => match args.first() {
            Some(Value::Array(a)) => {
                sort_values(&mut a.borrow_mut())?;
                Ok(Value::Void)
            }
            Some(other) => Err(format!(
                "sort() expects an array, got `{}`",
                other.type_name()
            )),
            None => Err("sort() expects 1 argument".to_string()),
        },
        "json_encode" => match args.first() {
            Some(v) => value_to_json(v).map(|j| Value::string(j.to_string())),
            None => Err("json_encode() expects 1 argument".to_string()),
        },
        "json_decode" => match args.first() {
            Some(Value::String(s)) => serde_json::from_str::<serde_json::Value>(s)
                .map_err(|e| format!("invalid JSON: {e}"))
                .map(|j| json_to_value(&j)),
            Some(other) => Err(format!(
                "json_decode() expects a string, got `{}`",
                other.type_name()
            )),
            None => Err("json_decode() expects 1 argument".to_string()),
        },
        "read_file" => match args.first() {
            Some(Value::String(path)) => std::fs::read_to_string(path.as_str())
                .map(Value::string)
                .map_err(|e| format!("could not read `{path}`: {e}")),
            Some(other) => Err(format!(
                "read_file() expects a string path, got `{}`",
                other.type_name()
            )),
            None => Err("read_file() expects 1 argument".to_string()),
        },
        "write_file" => {
            let path = match args.first() {
                Some(Value::String(s)) => s.to_string(),
                _ => return Err("write_file() expects (path: string, content: string)".to_string()),
            };
            let content = match args.get(1) {
                Some(Value::String(s)) => s.to_string(),
                _ => return Err("write_file() expects (path: string, content: string)".to_string()),
            };
            std::fs::write(&path, content)
                .map(|_| Value::Void)
                .map_err(|e| format!("could not write `{path}`: {e}"))
        }
        "http_get" => match args.first() {
            Some(Value::String(url)) => ailang_ai_runtime::HttpModelProvider::new(url.as_str())
                .get()
                .map(Value::string),
            Some(other) => Err(format!(
                "http_get() expects a URL string, got `{}`",
                other.type_name()
            )),
            None => Err("http_get() expects 1 argument".to_string()),
        },
        "http_post" => {
            let url = match args.first() {
                Some(Value::String(s)) => s.to_string(),
                _ => return Err("http_post() expects (url: string, body: string)".to_string()),
            };
            let body = match args.get(1) {
                Some(Value::String(s)) => s.to_string(),
                _ => return Err("http_post() expects (url: string, body: string)".to_string()),
            };
            ailang_ai_runtime::HttpModelProvider::new(url)
                .post(&body)
                .map(Value::string)
        }
        "hash" => match args.first() {
            Some(v) => Ok(Value::string(fnv1a_hex(&v.to_string()))),
            None => Err("hash() expects 1 argument".to_string()),
        },
        "sleep" => match args.first() {
            Some(Value::Int(secs)) => {
                std::thread::sleep(std::time::Duration::from_secs_f64((*secs).max(0) as f64));
                Ok(Value::Void)
            }
            Some(Value::Float(secs)) => {
                std::thread::sleep(std::time::Duration::from_secs_f64(secs.max(0.0)));
                Ok(Value::Void)
            }
            Some(other) => Err(format!(
                "sleep() expects a number of seconds, got `{}`",
                other.type_name()
            )),
            None => Err("sleep() expects 1 argument".to_string()),
        },
        other => Err(format!("unknown builtin function `{other}`")),
    }
}

/// In-place sort for arrays of a single comparable type (`int`, `float`,
/// or `string`). Mixed-type or non-comparable-element arrays (arrays of
/// arrays, maps, functions, ...) return a clear error rather than an
/// arbitrary/unstable ordering.
fn sort_values(values: &mut [Value]) -> Result<(), String> {
    let mut err = None;
    values.sort_by(|a, b| {
        use std::cmp::Ordering;
        match (a, b) {
            (Value::Int(x), Value::Int(y)) => x.cmp(y),
            (Value::Float(x), Value::Float(y)) => x.partial_cmp(y).unwrap_or(Ordering::Equal),
            (Value::Int(x), Value::Float(y)) => {
                (*x as f64).partial_cmp(y).unwrap_or(Ordering::Equal)
            }
            (Value::Float(x), Value::Int(y)) => {
                x.partial_cmp(&(*y as f64)).unwrap_or(Ordering::Equal)
            }
            (Value::String(x), Value::String(y)) => x.cmp(y),
            (x, y) => {
                if err.is_none() {
                    err = Some(format!(
                        "sort() cannot compare `{}` and `{}`",
                        x.type_name(),
                        y.type_name()
                    ));
                }
                Ordering::Equal
            }
        }
    });
    match err {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

/// Converts an AILang `Value` to JSON. Functions, natives, and every
/// AI-native value kind (agent, model, tool, ...) have no meaningful JSON
/// representation and return a clear error rather than silently producing
/// `null` or a lossy stand-in.
fn value_to_json(value: &Value) -> Result<serde_json::Value, String> {
    use serde_json::Value as J;
    Ok(match value {
        Value::Int(n) => J::Number((*n).into()),
        Value::Float(n) => serde_json::Number::from_f64(*n)
            .map(J::Number)
            .unwrap_or(J::Null),
        Value::Bool(b) => J::Bool(*b),
        Value::String(s) => J::String(s.to_string()),
        Value::Char(c) => J::String(c.to_string()),
        Value::Null | Value::Void => J::Null,
        Value::Array(a) => J::Array(
            a.borrow()
                .iter()
                .map(value_to_json)
                .collect::<Result<Vec<_>, _>>()?,
        ),
        Value::Map(m) => {
            let mut obj = serde_json::Map::new();
            for (k, v) in m.borrow().iter() {
                obj.insert(k.to_string(), value_to_json(v)?);
            }
            J::Object(obj)
        }
        other => {
            return Err(format!(
                "json_encode() cannot represent a `{}`",
                other.type_name()
            ))
        }
    })
}

/// Converts JSON into an AILang `Value`: objects become `map`s keyed by
/// string, arrays become `array`s, numbers become `int` when they have no
/// fractional part and `float` otherwise.
fn json_to_value(json: &serde_json::Value) -> Value {
    match json {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(b) => Value::Bool(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::Int(i)
            } else {
                Value::Float(n.as_f64().unwrap_or(0.0))
            }
        }
        serde_json::Value::String(s) => Value::string(s.clone()),
        serde_json::Value::Array(items) => Value::array(items.iter().map(json_to_value).collect()),
        serde_json::Value::Object(obj) => Value::map(
            obj.iter()
                .map(|(k, v)| (Value::string(k.clone()), json_to_value(v)))
                .collect(),
        ),
    }
}

/// A non-cryptographic FNV-1a hash rendered as hex. Explicitly **not**
/// suitable for security-sensitive uses (password hashing, signatures,
/// integrity against a malicious adversary) — it's here for fast,
/// deterministic fingerprinting (cache keys, dedup, quick integrity
/// checks against accidental corruption), the same honest scope as
/// `ailang-pm`'s lockfile fingerprint, which uses the identical algorithm.
fn fnv1a_hex(s: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in s.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

/// A real, deterministic summarization algorithm: return the first
/// sentence (up to the first `.`, `!`, or `?`), or the first 15 words if
/// no sentence terminator is found. Documented as a placeholder for a
/// real model-backed summarizer (see `ailang_ai_runtime::MockProvider`'s
/// doc comment) but genuinely implemented and genuinely deterministic —
/// not a stand-in that returns the input unchanged.
fn summarize_text(text: &str) -> String {
    let trimmed = text.trim();
    if let Some(end) = trimmed.find(['.', '!', '?']) {
        return trimmed[..=end].trim().to_string();
    }
    let words: Vec<&str> = trimmed.split_whitespace().take(15).collect();
    let summary = words.join(" ");
    if trimmed.split_whitespace().count() > 15 {
        format!("{summary}...")
    } else {
        summary
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn len_of_string_and_array() {
        let mut out = Vec::new();
        assert_eq!(
            call("len", &[Value::string("hello")], &mut out).unwrap(),
            Value::Int(5)
        );
        assert_eq!(
            call(
                "len",
                &[Value::array(vec![Value::Int(1), Value::Int(2)])],
                &mut out
            )
            .unwrap(),
            Value::Int(2)
        );
    }

    #[test]
    #[allow(clippy::approx_constant)]
    fn parse_int_and_float() {
        let mut out = Vec::new();
        assert_eq!(
            call("parse_int", &[Value::string("42")], &mut out).unwrap(),
            Value::Int(42)
        );
        assert_eq!(
            call("parse_float", &[Value::string("3.14")], &mut out).unwrap(),
            Value::Float(3.14)
        );
    }

    #[test]
    fn assert_failure_returns_error() {
        let mut out = Vec::new();
        assert!(call("assert", &[Value::Bool(false)], &mut out).is_err());
    }

    #[test]
    fn print_writes_to_sink() {
        let mut out = Vec::new();
        call("print", &[Value::string("hi")], &mut out).unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), "hi");
    }

    // -----------------------------------------------------------------
    // v1.0 standard library expansion
    // -----------------------------------------------------------------

    #[test]
    fn push_appends_and_returns_new_length() {
        let mut out = Vec::new();
        let arr = Value::array(vec![Value::Int(1), Value::Int(2)]);
        let result = call("push", &[arr.clone(), Value::Int(3)], &mut out).unwrap();
        assert_eq!(result, Value::Int(3));
        if let Value::Array(a) = arr {
            assert_eq!(
                *a.borrow(),
                vec![Value::Int(1), Value::Int(2), Value::Int(3)]
            );
        }
    }

    #[test]
    fn pop_removes_and_returns_the_last_element() {
        let mut out = Vec::new();
        let arr = Value::array(vec![Value::Int(1), Value::Int(2)]);
        let result = call("pop", &[arr], &mut out).unwrap();
        assert_eq!(result, Value::Int(2));
    }

    #[test]
    fn pop_on_empty_array_returns_null() {
        let mut out = Vec::new();
        let arr = Value::array(vec![]);
        assert_eq!(call("pop", &[arr], &mut out).unwrap(), Value::Null);
    }

    #[test]
    fn sort_orders_integers_ascending() {
        let mut out = Vec::new();
        let arr = Value::array(vec![Value::Int(3), Value::Int(1), Value::Int(2)]);
        call("sort", &[arr.clone()], &mut out).unwrap();
        if let Value::Array(a) = arr {
            assert_eq!(
                *a.borrow(),
                vec![Value::Int(1), Value::Int(2), Value::Int(3)]
            );
        }
    }

    #[test]
    fn sort_orders_strings_lexically() {
        let mut out = Vec::new();
        let arr = Value::array(vec![Value::string("banana"), Value::string("apple")]);
        call("sort", &[arr.clone()], &mut out).unwrap();
        if let Value::Array(a) = arr {
            assert_eq!(
                *a.borrow(),
                vec![Value::string("apple"), Value::string("banana")]
            );
        }
    }

    #[test]
    fn sort_rejects_mixed_incomparable_types() {
        let mut out = Vec::new();
        let arr = Value::array(vec![Value::Int(1), Value::string("x")]);
        assert!(call("sort", &[arr], &mut out).is_err());
    }

    #[test]
    fn reverse_reverses_in_place() {
        let mut out = Vec::new();
        let arr = Value::array(vec![Value::Int(1), Value::Int(2), Value::Int(3)]);
        call("reverse", &[arr.clone()], &mut out).unwrap();
        if let Value::Array(a) = arr {
            assert_eq!(
                *a.borrow(),
                vec![Value::Int(3), Value::Int(2), Value::Int(1)]
            );
        }
    }

    #[test]
    fn json_encode_and_decode_round_trip_a_map() {
        let mut out = Vec::new();
        let original = Value::map(vec![
            (Value::string("name"), Value::string("Ada")),
            (Value::string("age"), Value::Int(30)),
        ]);
        let encoded = call("json_encode", &[original], &mut out).unwrap();
        let decoded = call("json_decode", &[encoded], &mut out).unwrap();
        if let Value::Map(m) = decoded {
            let entries = m.borrow();
            assert!(entries.contains(&(Value::string("name"), Value::string("Ada"))));
            assert!(entries.contains(&(Value::string("age"), Value::Int(30))));
        } else {
            panic!("expected a map");
        }
    }

    #[test]
    fn json_encode_rejects_unrepresentable_values() {
        let mut out = Vec::new();
        let func_like = Value::native("print");
        assert!(call("json_encode", &[func_like], &mut out).is_err());
    }

    #[test]
    fn json_decode_rejects_invalid_json() {
        let mut out = Vec::new();
        assert!(call("json_decode", &[Value::string("{not valid json")], &mut out).is_err());
    }

    #[test]
    fn write_file_and_read_file_round_trip() {
        let mut out = Vec::new();
        let path =
            std::env::temp_dir().join(format!("ailang-stdlib-test-{}.txt", std::process::id()));
        let path_str = path.to_str().unwrap();
        call(
            "write_file",
            &[Value::string(path_str), Value::string("hello disk")],
            &mut out,
        )
        .unwrap();
        let content = call("read_file", &[Value::string(path_str)], &mut out).unwrap();
        assert_eq!(content, Value::string("hello disk"));
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn read_file_reports_a_missing_file_clearly() {
        let mut out = Vec::new();
        assert!(call(
            "read_file",
            &[Value::string("/nonexistent/path/xyz")],
            &mut out
        )
        .is_err());
    }

    #[test]
    fn hash_is_deterministic() {
        let mut out = Vec::new();
        let a = call("hash", &[Value::string("hello")], &mut out).unwrap();
        let b = call("hash", &[Value::string("hello")], &mut out).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn hash_differs_for_different_input() {
        let mut out = Vec::new();
        let a = call("hash", &[Value::string("hello")], &mut out).unwrap();
        let b = call("hash", &[Value::string("world")], &mut out).unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn sleep_actually_sleeps_for_roughly_the_requested_duration() {
        let mut out = Vec::new();
        let start = std::time::Instant::now();
        call("sleep", &[Value::Float(0.05)], &mut out).unwrap();
        assert!(start.elapsed().as_secs_f64() >= 0.04);
    }
}
