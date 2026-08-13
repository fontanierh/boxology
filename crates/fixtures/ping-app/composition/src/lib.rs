//! A small composition that exposes the `ping` fixture in-process and over HTTP.

#![deny(missing_docs)]
#![forbid(unsafe_code)]

use std::net::SocketAddr;
use std::sync::Arc;

use boxology_contract::ExposureLevel;
use boxology_http::{HttpServerBinding, HttpServerConfig};
use boxology_runtime::{AssemblyErrors, Composition, CompositionBuilder};
use ping_implementation::{PingService, generated};

/// A running `ping` composition with its ordinary typed box handle.
pub struct PingApp {
    composition: Composition,
    address: SocketAddr,
    ping: ping_contract::PingHandle,
}

impl PingApp {
    /// Returns the generated handle for the composed `ping` box.
    pub fn ping(&self) -> &ping_contract::PingHandle {
        &self.ping
    }

    /// Returns the bound HTTP address.
    pub fn http_address(&self) -> SocketAddr {
        self.address
    }

    /// Gracefully shuts down the composition.
    pub async fn shutdown(
        self,
        timeout: std::time::Duration,
    ) -> Result<(), boxology_contract::ErasedCallError> {
        self.composition.shutdown(timeout).await
    }
}

/// Starts the live `ping-app` composition.
pub fn start() -> Result<PingApp, AssemblyErrors> {
    let http = Arc::new(HttpServerBinding::new(HttpServerConfig::new(
        "127.0.0.1:0".parse().expect("loopback address is valid"),
    )));

    let mut builder = CompositionBuilder::new();
    let ping_box = generated::register(&mut builder, PingService);
    let ping = builder.handle::<ping_contract::PingHandle>(&ping_box);
    builder.expose_all(&ping_box, http.clone(), ExposureLevel::External);

    let composition = builder.start()?;
    let address = http.local_addr().expect("HTTP binding bound during start");
    Ok(PingApp {
        composition,
        address,
        ping,
    })
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use boxology_contract::{CallContext, Caller, CancelToken, TraceContext};
    use boxology_manifest::{CrateRole, Exposure, Kind, Manifest, RelativePath, Transport};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    use super::start;

    const MANIFEST: &str = include_str!("../../boxology.toml");

    fn context() -> CallContext {
        CallContext::new(
            Caller::Anonymous,
            None,
            CancelToken::new(),
            TraceContext::empty(),
            None,
        )
    }

    #[test]
    fn manifest_is_the_exact_checked_in_composition_shape() {
        assert_eq!(MANIFEST.lines().next(), Some("schema = 1"));
        let manifest = Manifest::parse(
            RelativePath::new("boxology.toml").expect("manifest path is valid"),
            MANIFEST.as_bytes(),
        )
        .expect("checked-in composition manifest is valid");

        assert_eq!(manifest.id().as_str(), "ping-app");
        assert_eq!(manifest.kind(), Kind::Composition);
        assert_eq!(
            manifest
                .owned()
                .iter()
                .map(|pattern| pattern.as_str())
                .collect::<Vec<_>>(),
            vec!["boxology.toml", "composition/**"]
        );
        assert_eq!(
            manifest.quality_commands(),
            &[
                "cargo test -p ping-app tests::assembled_ping_answers_in_process_and_over_real_http -- --exact"
            ]
        );
        assert_eq!(manifest.crates().len(), 1);
        let crate_entry = &manifest.crates()[0];
        assert_eq!(crate_entry.cargo_package(), "ping-app");
        assert_eq!(crate_entry.path().as_str(), "composition");
        assert_eq!(crate_entry.role(), CrateRole::Composition);

        let composition = manifest.composition().expect("composition section exists");
        assert_eq!(
            composition
                .boxes()
                .iter()
                .map(|box_id| box_id.as_str())
                .collect::<Vec<_>>(),
            vec!["ping"]
        );
        assert_eq!(composition.bindings().len(), 2);
        let bindings = composition.bindings();
        assert_eq!(bindings[0].capability().to_string(), "ping.*");
        assert_eq!(bindings[0].transport(), Transport::InProcess);
        assert_eq!(bindings[0].exposure(), None);
        assert_eq!(bindings[1].capability().to_string(), "ping.*");
        assert_eq!(bindings[1].transport(), Transport::Http);
        assert_eq!(bindings[1].exposure(), Some(Exposure::External));
    }

    #[tokio::test]
    async fn assembled_ping_answers_in_process_and_over_real_http() {
        let app = start().expect("ping composition starts");
        let address = app.http_address();
        let first = app
            .ping()
            .ping(context(), 17)
            .await
            .expect("first in-process call succeeds");
        let second = app
            .ping()
            .ping(context(), 9_001)
            .await
            .expect("second in-process call succeeds");
        assert_ne!(first, second);
        assert_eq!((first, second), (17, 9_001));

        for nonce in [31_u64, 7_777_u64] {
            let request = format!("\"{nonce}\"");
            let response = post(address, "ping", request.as_bytes()).await;
            let expected = format!(r#"{{"result":{{"value":"{nonce}"}}}}"#);
            assert_canonical_response(&response, "HTTP/1.1 200 OK", expected.as_bytes());
            assert_eq!(decode_http_nonce(&response.body), nonce);
        }

        let malformed = post(address, "ping", b"not-a-u64").await;
        assert_canonical_response(
            &malformed,
            "HTTP/1.1 400 Bad Request",
            br#"{"error":{"kind":"call","code":"invalid_request","message":"invalid request"}}"#,
        );

        if std::env::var_os("BOXOLOGY_REQUIRE_GREET").is_some() {
            let response = post(address, "greet", br#""Grace""#).await;
            assert_canonical_response(
                &response,
                "HTTP/1.1 200 OK",
                br#"{"result":{"value":"Hello, Grace!"}}"#,
            );
        }

        app.shutdown(Duration::from_secs(1))
            .await
            .expect("composition shutdown succeeds");
        assert!(TcpStream::connect(address).await.is_err());
    }

    struct HttpResponse {
        status_line: String,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
    }

    async fn post(address: std::net::SocketAddr, capability: &str, body: &[u8]) -> HttpResponse {
        let mut stream = TcpStream::connect(address)
            .await
            .expect("HTTP listener accepts");
        let head = format!(
            "POST /rpc/ping/{capability} HTTP/1.1\r\nHost: boxology\r\n\
             Content-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n",
            body.len()
        );
        stream
            .write_all(head.as_bytes())
            .await
            .expect("request head writes");
        stream.write_all(body).await.expect("request body writes");
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).await.expect("response reads");
        split_response(&raw)
    }

    fn split_response(raw: &[u8]) -> HttpResponse {
        let boundary = raw
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .expect("response has a header/body boundary");
        let mut lines = raw[..boundary + 2]
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty());
        let status_line = header_line(lines.next().expect("response has a status line"));
        let headers = lines
            .map(|line| {
                let line = header_line(line);
                let (name, value) = line.split_once(':').expect("response header has a colon");
                (name.to_ascii_lowercase(), value.trim_start().to_owned())
            })
            .collect();
        HttpResponse {
            status_line,
            headers,
            body: raw[boundary + 4..].to_vec(),
        }
    }

    fn header_line(line: &[u8]) -> String {
        let line = line.strip_suffix(b"\r").expect("HTTP line ends with CRLF");
        std::str::from_utf8(line)
            .expect("HTTP response is UTF-8")
            .to_owned()
    }

    fn header_value<'a>(response: &'a HttpResponse, name: &str) -> &'a str {
        let mut matches = response.headers.iter().filter(|(header, _)| header == name);
        let (_, value) = matches.next().expect("required response header is present");
        assert!(
            matches.next().is_none(),
            "required response header occurs once"
        );
        value
    }

    fn assert_canonical_response(response: &HttpResponse, status_line: &str, body: &[u8]) {
        assert_eq!(response.status_line, status_line);
        assert_eq!(header_value(response, "content-type"), "application/json");
        assert_eq!(header_value(response, "connection"), "close");
        let content_length = header_value(response, "content-length")
            .parse::<usize>()
            .expect("content length is decimal");
        assert_eq!(content_length, body.len());
        assert!(
            !response
                .headers
                .iter()
                .any(|(name, _)| name == "transfer-encoding"),
            "canonical responses use Content-Length framing"
        );
        assert_eq!(response.body, body);
    }

    fn decode_http_nonce(body: &[u8]) -> u64 {
        let body = std::str::from_utf8(body).expect("HTTP body is UTF-8");
        let value = body
            .strip_prefix("{\"result\":{\"value\":\"")
            .and_then(|body| body.strip_suffix("\"}}"))
            .expect("HTTP body is the result envelope");
        value.parse().expect("HTTP result value is a u64")
    }
}
