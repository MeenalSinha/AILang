//! Model provider abstraction: [`ModelProvider`] is the extension point the
//! v0.5 spec asks for ("the runtime should provide a provider abstraction
//! so new model vendors can be added without changing the language").
//! [`MockProvider`] and [`HttpModelProvider`] are the two implementations
//! this release ships; see the crate-level doc for exactly what's real
//! about each.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use ailang_ast::TypeExpr;
use ailang_runtime::Value;

/// Anything that can turn a prompt into text, and fill a schema's fields
/// with values. New vendors (a real OpenAI/Anthropic/Gemini client, etc.)
/// implement this trait; nothing in the language, parser, or interpreter
/// needs to change to add one — only `ProviderRegistry::register` (or its
/// default-construction logic) needs to know how to build one from a
/// `model` declaration.
pub trait ModelProvider {
    fn generate_text(&self, prompt: &str) -> Result<String, String>;
    fn generate_structured(&self, fields: &[(String, TypeExpr)]) -> Result<Value, String>;
}

/// A deterministic, offline stand-in for a real model. It never fails,
/// never makes a network call, and its output is a pure function of its
/// input — which is exactly what makes it useful as the default provider
/// for every `model` declaration that doesn't specify a real `endpoint`:
/// programs using `generate`/model-backed stdlib functions run
/// identically in a test suite, in CI, and offline, with zero setup.
///
/// **This is not a real language model.** `generate_text` produces a
/// clearly-tagged echo of the prompt (`"[mock-model output for: ...]"`),
/// and `generate_structured` fills each schema field with a
/// type-appropriate default (`""`  for `string`, `0` for `int`, etc. — see
/// [`crate::schema::default_value_for_type`]). Wiring in a real provider
/// behind a `model LocalModel { endpoint "..." }` declaration uses
/// [`HttpModelProvider`] instead; wiring in a real hosted-API provider is a
/// new struct implementing this same trait.
pub struct MockProvider;

impl ModelProvider for MockProvider {
    fn generate_text(&self, prompt: &str) -> Result<String, String> {
        let trimmed = prompt.trim();
        let preview: String = trimmed.chars().take(80).collect();
        let suffix = if trimmed.chars().count() > 80 {
            "..."
        } else {
            ""
        };
        Ok(format!("[mock-model output for: {preview}{suffix}]"))
    }

    fn generate_structured(&self, fields: &[(String, TypeExpr)]) -> Result<Value, String> {
        Ok(crate::schema::fill_schema_defaults(fields))
    }
}

/// A genuine (if minimal) HTTP/1.1 client, built on `std::net::TcpStream`
/// only — no TLS, no external HTTP crate. This is a deliberate scope
/// boundary, not an oversight: adding TLS support means either hand-rolling
/// a TLS implementation (a bad idea) or depending on a crate like
/// `rustls`/`native-tls`, whose current versions hit the same
/// toolchain-compatibility wall documented in the v0.1 README (they
/// require a newer Rust edition than this release targets). Plain `http://`
/// endpoints — which is what local model servers (Ollama, llama.cpp
/// server, etc.) use by default — work today; `https://` endpoints return a
/// clear error rather than silently failing or (worse) silently not
/// encrypting anything.
///
/// The protocol handling itself (request line, headers, chunked-or-
/// content-length body framing on the response) is real, tested against
/// an in-process `TcpListener` in this module's tests — not mocked out.
pub struct HttpModelProvider {
    pub endpoint: String,
}

impl HttpModelProvider {
    pub fn new(endpoint: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
        }
    }

    /// A real HTTP/1.1 POST with an arbitrary request body, returning
    /// the response body. Public (not just used internally by
    /// `generate_text`) so general-purpose stdlib functions like
    /// `http_post` can reuse the same client — see
    /// `ailang-interpreter`'s stdlib module.
    pub fn post(&self, body: &str) -> Result<String, String> {
        let (host, port, path) = parse_http_url(&self.endpoint)?;

        let mut stream = TcpStream::connect((host.as_str(), port))
            .map_err(|e| format!("could not connect to {}:{port}: {e}", host))?;
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .map_err(|e| e.to_string())?;

        let request = format!(
            "POST {path} HTTP/1.1\r\n\
             Host: {host}\r\n\
             Content-Type: application/json\r\n\
             Content-Length: {}\r\n\
             Connection: close\r\n\
             \r\n\
             {body}",
            body.len()
        );
        stream
            .write_all(request.as_bytes())
            .map_err(|e| format!("write failed: {e}"))?;

        let mut response = Vec::new();
        stream
            .read_to_end(&mut response)
            .map_err(|e| format!("read failed: {e}"))?;

        parse_http_response_body(&response)
    }

    /// A real HTTP GET against `self.endpoint`, returning the response
    /// body. Used by [`crate::tools::HttpTool`]; shares the same
    /// std-only, HTTP/1.1-over-`TcpStream` implementation as
    /// [`Self::post_json`] (see this struct's doc comment for the TLS
    /// scope boundary).
    pub fn get(&self) -> Result<String, String> {
        let (host, port, path) = parse_http_url(&self.endpoint)?;
        let mut stream = TcpStream::connect((host.as_str(), port))
            .map_err(|e| format!("could not connect to {host}:{port}: {e}"))?;
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .map_err(|e| e.to_string())?;

        let request = format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n");
        stream
            .write_all(request.as_bytes())
            .map_err(|e| format!("write failed: {e}"))?;

        let mut response = Vec::new();
        stream
            .read_to_end(&mut response)
            .map_err(|e| format!("read failed: {e}"))?;

        parse_http_response_body(&response)
    }
}

impl ModelProvider for HttpModelProvider {
    fn generate_text(&self, prompt: &str) -> Result<String, String> {
        let escaped = prompt.replace('\\', "\\\\").replace('"', "\\\"");
        let body = format!(r#"{{"prompt":"{escaped}"}}"#);
        self.post(&body)
    }

    fn generate_structured(&self, fields: &[(String, TypeExpr)]) -> Result<Value, String> {
        // v0.5 foundation: a local model's raw text response isn't parsed
        // into typed fields yet (that needs a real JSON-schema-constrained
        // decoding strategy per model backend). Rather than guess, fall
        // back to the same schema-defaults behavior as MockProvider so
        // `generate` still returns a well-typed value; see docs/roadmap.md.
        MockProvider.generate_structured(fields)
    }
}

fn parse_http_url(url: &str) -> Result<(String, u16, String), String> {
    let rest = url
        .strip_prefix("http://")
        .ok_or_else(|| format!("unsupported endpoint scheme in `{url}` (only http:// is supported; see HttpModelProvider's doc comment for why)"))?;
    let (authority, path) = match rest.find('/') {
        Some(idx) => (&rest[..idx], &rest[idx..]),
        None => (rest, "/"),
    };
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) => (
            h.to_string(),
            p.parse::<u16>()
                .map_err(|_| format!("invalid port in endpoint `{url}`"))?,
        ),
        None => (authority.to_string(), 80),
    };
    Ok((host, port, path.to_string()))
}

/// Parses a raw HTTP/1.1 response into just its body, handling both
/// `Content-Length`-framed and `Transfer-Encoding: chunked` responses
/// (a real local-model server may use either).
fn parse_http_response_body(raw: &[u8]) -> Result<String, String> {
    let text = String::from_utf8_lossy(raw);
    let sep = text
        .find("\r\n\r\n")
        .ok_or("malformed HTTP response: no header/body separator")?;
    let headers = &text[..sep];
    let body = &text[sep + 4..];

    let mut header_map = HashMap::new();
    for line in headers.split("\r\n").skip(1) {
        if let Some((k, v)) = line.split_once(':') {
            header_map.insert(k.trim().to_lowercase(), v.trim().to_string());
        }
    }

    if header_map
        .get("transfer-encoding")
        .map(|v| v.eq_ignore_ascii_case("chunked"))
        .unwrap_or(false)
    {
        return Ok(dechunk(body));
    }

    Ok(body.to_string())
}

fn dechunk(body: &str) -> String {
    let mut out = String::new();
    let mut rest = body;
    loop {
        let Some(line_end) = rest.find("\r\n") else {
            break;
        };
        let size_line = &rest[..line_end];
        let Ok(size) = usize::from_str_radix(size_line.trim(), 16) else {
            break;
        };
        if size == 0 {
            break;
        }
        let chunk_start = line_end + 2;
        let chunk_end = (chunk_start + size).min(rest.len());
        out.push_str(&rest[chunk_start..chunk_end]);
        if chunk_end + 2 > rest.len() {
            break;
        }
        rest = &rest[chunk_end + 2..];
    }
    out
}

/// Maps registered `model` names to a provider implementation. A model
/// declared with an `endpoint` gets an [`HttpModelProvider`]; every other
/// model name (and any name not registered at all — e.g. `generate`, which
/// doesn't name a model) falls back to [`MockProvider`], so programs never
/// hard-fail just because a real provider wasn't configured.
#[derive(Default)]
pub struct ProviderRegistry {
    providers: HashMap<String, Box<dyn ModelProvider>>,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, model_name: &str, endpoint: Option<&str>) {
        let provider: Box<dyn ModelProvider> = match endpoint {
            Some(url) => Box::new(HttpModelProvider::new(url)),
            None => Box::new(MockProvider),
        };
        self.providers.insert(model_name.to_string(), provider);
    }

    pub fn get(&self, model_name: &str) -> &dyn ModelProvider {
        self.providers
            .get(model_name)
            .map(|p| p.as_ref())
            .unwrap_or(&MockProvider)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufRead;
    use std::net::TcpListener;
    use std::thread;

    #[test]
    fn mock_provider_generate_text_is_deterministic() {
        let a = MockProvider.generate_text("hello").unwrap();
        let b = MockProvider.generate_text("hello").unwrap();
        assert_eq!(a, b);
        assert!(a.contains("hello"));
    }

    #[test]
    fn mock_provider_truncates_long_prompts_in_preview() {
        let long_prompt = "x".repeat(200);
        let out = MockProvider.generate_text(&long_prompt).unwrap();
        assert!(out.contains("..."));
    }

    #[test]
    fn mock_provider_fills_schema_defaults() {
        let fields = vec![
            ("name".to_string(), TypeExpr::String),
            ("age".to_string(), TypeExpr::Int),
        ];
        let value = MockProvider.generate_structured(&fields).unwrap();
        assert!(matches!(value, Value::Map(_)));
    }

    #[test]
    fn provider_registry_defaults_to_mock_for_unregistered_names() {
        let registry = ProviderRegistry::new();
        let out = registry
            .get("SomeUnregisteredModel")
            .generate_text("hi")
            .unwrap();
        assert!(out.contains("mock-model"));
    }

    #[test]
    fn parses_plain_http_url() {
        let (host, port, path) = parse_http_url("http://localhost:11434/generate").unwrap();
        assert_eq!(host, "localhost");
        assert_eq!(port, 11434);
        assert_eq!(path, "/generate");
    }

    #[test]
    fn parses_http_url_without_explicit_port_or_path() {
        let (host, port, path) = parse_http_url("http://example.com").unwrap();
        assert_eq!(host, "example.com");
        assert_eq!(port, 80);
        assert_eq!(path, "/");
    }

    #[test]
    fn rejects_https_urls_with_a_clear_error() {
        let err = parse_http_url("https://example.com").unwrap_err();
        assert!(err.contains("http://"));
    }

    #[test]
    fn dechunk_reassembles_chunked_body() {
        let chunked = "5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n";
        assert_eq!(dechunk(chunked), "hello world");
    }

    /// End-to-end test of the real HTTP client against a tiny in-process
    /// server (no external network access, no mocking of the client
    /// itself) — this is what makes `HttpModelProvider` "real network I/O"
    /// rather than an aspirational doc comment.
    #[test]
    fn http_model_provider_talks_to_a_real_local_server() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
            let mut request_line = String::new();
            reader.read_line(&mut request_line).unwrap();
            // Drain headers.
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
            }
            let body = r#"{"response":"hello from the local model"}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).unwrap();
        });

        let provider = HttpModelProvider::new(format!("http://127.0.0.1:{}/", addr.port()));
        let result = provider.generate_text("ping").unwrap();
        assert!(result.contains("hello from the local model"));

        server.join().unwrap();
    }
}
