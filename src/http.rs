//! HTTP/HTTPS client module for embedded ESP32-C3.
//!
//! Provides a **blocking** HTTP/1.1 client that can operate over TLS (HTTPS)
//! using `embedded-tls` 0.19 (TLS 1.3, no_std, no allocator).
//!
//! # Security Note
//!
//! TLS certificate verification is currently **disabled** (`UnsecureProvider`/
//! `NoVerify`) because `embedded-tls`'s `webpki` verifier requires `alloc`,
//! which this no_std firmware does not enable.  The connection is still
//! encrypted (TLS 1.3 with AES-128-GCM) and resistant to passive
//! eavesdropping, but is **not** protected against active man-in-the-middle
//! attacks.  This is acceptable for fetching public price data; for anything
//! sensitive, enable `alloc` and use a real `CertVerifier`.
//!
//! # Limitations
//!
//! - HTTPS only (plain HTTP not supported by the high-level API — the device
//!   should always use TLS for external APIs).
//! - Response body is truncated if it exceeds the caller's buffer.
//! - DNS resolution is performed via the smoltcp DNS socket.

// ── Shared parsing helpers (compiled on host AND target) ────────

use core::fmt::Write as _;
use heapless::String as HString;

/// Errors that can occur during HTTP operations.
#[derive(Debug, Clone, PartialEq)]
pub enum HttpError {
    ConnectionFailed,
    SendFailed,
    RecvFailed,
    ParseError,
    InvalidUrl,
    BodyTooLarge,
    DnsFailed,
    TlsFailed,
}

/// A parsed HTTP response.
#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status_code: u16,
    pub body_len: usize,
}

/// Reject HTTP request tokens that contain control characters which could
/// break the request line or headers (`\r`, `\n`, `\0`).
fn validate_http_token(s: &str) -> Result<(), HttpError> {
    if s.bytes().any(|b| b == b'\r' || b == b'\n' || b == b'\0') {
        return Err(HttpError::InvalidUrl);
    }
    Ok(())
}

/// Build an HTTP/1.1 GET request string.
///
/// Shared between host and target so the request format can be tested on host.
pub fn build_get_request(host: &str, path: &str) -> Result<HString<512>, HttpError> {
    validate_http_token(host)?;
    validate_http_token(path)?;
    let mut req = HString::<512>::new();
    write!(&mut req, "GET {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nUser-Agent: blink/0.1\r\n\r\n", path, host)
        .map_err(|_| HttpError::SendFailed)?;
    Ok(req)
}

/// Parse an HTTP/1.1 response from raw bytes.
///
/// Extracts the status code and copies everything after the headers into
/// `body_buf`.  Shared between host and target so it can be tested on host.
pub fn parse_http_response(raw: &[u8], body_buf: &mut [u8]) -> Result<HttpResponse, HttpError> {
    let header_end = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .unwrap_or(raw.len());

    let header_text =
        core::str::from_utf8(&raw[..header_end]).map_err(|_| HttpError::ParseError)?;

    let status_end = header_text.find("\r\n").unwrap_or(header_text.len());
    let status_line = &header_text[..status_end];

    let mut parts = status_line.splitn(3, ' ');
    let version = parts.next().ok_or(HttpError::ParseError)?;
    if version != "HTTP/1.1" && version != "HTTP/1.0" {
        return Err(HttpError::ParseError);
    }
    let code_str = parts.next().ok_or(HttpError::ParseError)?;
    let status_code: u16 = code_str.parse().map_err(|_| HttpError::ParseError)?;

    let body_start = if header_end < raw.len() {
        header_end + 4
    } else {
        raw.len()
    };
    let body = &raw[body_start..];

    let copy_len = body.len().min(body_buf.len());
    body_buf[..copy_len].copy_from_slice(&body[..copy_len]);

    Ok(HttpResponse {
        status_code,
        body_len: copy_len,
    })
}

// ── Target implementation (riscv32 + network feature) ──────────

#[cfg(all(target_arch = "riscv32", feature = "network"))]
mod inner {
    use super::{build_get_request, parse_http_response, HttpError, HttpResponse};
    use crate::wifi::NetworkStack;
    use embedded_tls::blocking::{TlsConfig, TlsConnection, TlsContext, UnsecureProvider};
    use embedded_tls::Aes128GcmSha256;
    use esp_hal::rng::Rng;
    use rand_core::{CryptoRng, RngCore};

    /// Static buffers for TLS records (too large for the stack).
    /// 16 KiB read buffer is required for the TLS handshake (server certificate).
    static mut TLS_READ_BUF: [u8; 16384] = [0; 16384];
    static mut TLS_WRITE_BUF: [u8; 4096] = [0; 4096];

    /// Wrapper around `esp_hal::rng::Rng` that also implements `CryptoRng`,
    /// which `embedded-tls` requires.
    ///
    /// The ESP32-C3 hardware RNG produces true random numbers when the RF
    /// (WiFi) subsystem is enabled, satisfying the `CryptoRng` contract.
    struct CryptoRngWrapper {
        rng: Rng,
    }

    impl CryptoRngWrapper {
        fn new() -> Self {
            // `esp_wifi::init` consumed the original `RNG` peripheral, but the
            // hardware register is still accessible.  Steal a new instance.
            let stolen = unsafe { esp_hal::peripherals::RNG::steal() };
            Self {
                rng: Rng::new(stolen),
            }
        }
    }

    impl RngCore for CryptoRngWrapper {
        fn next_u32(&mut self) -> u32 {
            self.rng.next_u32()
        }
        fn next_u64(&mut self) -> u64 {
            self.rng.next_u64()
        }
        fn fill_bytes(&mut self, dest: &mut [u8]) {
            self.rng.fill_bytes(dest)
        }
        fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand_core::Error> {
            self.rng.try_fill_bytes(dest)
        }
    }

    impl CryptoRng for CryptoRngWrapper {}

    /// A minimal, blocking HTTPS client for ESP32-C3.
    ///
    /// Uses `embedded-tls` 0.19 (TLS 1.3) over a smoltcp TCP socket provided
    /// by [`NetworkStack`].
    pub struct HttpClient;

    impl HttpClient {
        pub fn new() -> Self {
            Self
        }

        /// Perform an HTTPS **GET** request.
        ///
        /// # Arguments
        /// * `stack` — Network stack with WiFi connected and DHCP configured.
        /// * `host` — Server hostname (e.g. `"api.binance.com"`). DNS is resolved
        ///   automatically.
        /// * `path` — Request path (e.g. `"/api/v3/ticker/price?symbol=BTCUSDT"`).
        /// * `resp_body` — Buffer where the response body will be written.
        pub fn get_https(
            &mut self,
            stack: &mut NetworkStack<'_>,
            host: &str,
            path: &str,
            resp_body: &mut [u8],
        ) -> Result<HttpResponse, HttpError> {
            // 1. DNS resolve
            let ip = stack
                .resolve_dns(host)
                .map_err(|_| HttpError::DnsFailed)?;

            // 2. TCP connect to port 443
            let handle = stack
                .tcp_connect(ip, 443)
                .map_err(|_| HttpError::ConnectionFailed)?;

            // 3. Create embedded-io bridge + TLS connection
            let io = stack.tcp_io(handle);

            let mut tls = TlsConnection::new(
                io,
                unsafe { &mut *core::ptr::addr_of_mut!(TLS_READ_BUF) },
                unsafe { &mut *core::ptr::addr_of_mut!(TLS_WRITE_BUF) },
            );

            // 4. TLS handshake (no certificate verification — see module docs)
            let config = TlsConfig::new().with_server_name(host);
            let rng = CryptoRngWrapper::new();
            let context = TlsContext::new(&config, UnsecureProvider::new::<Aes128GcmSha256>(rng));

            tls.open(context).map_err(|_| HttpError::TlsFailed)?;

            // 5. Send HTTP request
            let request = build_get_request(host, path)?;
            tls.write(request.as_bytes())
                .map_err(|_| HttpError::SendFailed)?;
            tls.flush().ok();

            // 6. Read response — read until EOF (Connection: close) or buffer full
            let mut raw_buf: [u8; 2048] = [0; 2048];
            let mut total = 0usize;
            loop {
                if total >= raw_buf.len() {
                    break; // buffer full
                }
                let n = match tls.read(&mut raw_buf[total..]) {
                    Ok(0) => break, // EOF — server closed connection
                    Ok(n) => n,
                    Err(_) => {
                        // If we already have some data, try to parse it
                        if total > 0 {
                            break;
                        }
                        return Err(HttpError::RecvFailed);
                    }
                };
                total += n;
            }

            // 7. Drop TLS connection (releases borrow on stack)
            drop(tls);

            // 8. Close TCP socket
            stack.tcp_close(handle);

            if total == 0 {
                return Err(HttpError::RecvFailed);
            }

            // 9. Parse HTTP response
            parse_http_response(&raw_buf[..total], resp_body)
        }
    }

    impl Default for HttpClient {
        fn default() -> Self {
            Self::new()
        }
    }
}

// ── Host-side stubs (host or no network feature) ───────────────

#[cfg(not(all(target_arch = "riscv32", feature = "network")))]
mod inner {
    use super::{HttpError, HttpResponse};

    /// Stub HTTP client for host-side compilation/testing.
    pub struct HttpClient;

    impl HttpClient {
        pub fn new() -> Self {
            Self
        }

        pub fn get_https(
            &mut self,
            _resp_body: &mut [u8],
            _host: &str,
            _path: &str,
        ) -> Result<HttpResponse, HttpError> {
            Err(HttpError::ConnectionFailed)
        }
    }

    impl Default for HttpClient {
        fn default() -> Self {
            Self::new()
        }
    }
}

#[cfg(all(target_arch = "riscv32", feature = "network"))]
pub use inner::*;

#[cfg(not(all(target_arch = "riscv32", feature = "network")))]
pub use inner::*;

// ── Tests ────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_get_request_basic() {
        let req = build_get_request("api.binance.com", "/api/v3/ticker/price?symbol=BTCUSDT").unwrap();
        assert!(req.as_str().starts_with("GET /api/v3/ticker/price?symbol=BTCUSDT HTTP/1.1\r\n"));
        assert!(req.as_str().contains("Host: api.binance.com\r\n"));
        assert!(req.as_str().contains("Connection: close\r\n"));
        assert!(req.as_str().ends_with("\r\n\r\n"));
    }

    #[test]
    fn build_get_request_rejects_newline_in_host() {
        let result = build_get_request("evil.com\r\nInjected: yes", "/path");
        assert!(result.is_err());
    }

    #[test]
    fn build_get_request_rejects_newline_in_path() {
        let result = build_get_request("api.binance.com", "/path\r\nInjected: yes");
        assert!(result.is_err());
    }

    #[test]
    fn response_200_with_body() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Length: 12\r\n\r\nHello World!";
        let mut buf = [0u8; 128];
        let resp = parse_http_response(raw, &mut buf).unwrap();
        assert_eq!(resp.status_code, 200);
        assert!(buf[..resp.body_len].starts_with(b"Hello World!"));
    }

    #[test]
    fn response_404_no_body() {
        let raw = b"HTTP/1.1 404 Not Found\r\n\r\n";
        let mut buf = [0u8; 128];
        let resp = parse_http_response(raw, &mut buf).unwrap();
        assert_eq!(resp.status_code, 404);
        assert_eq!(resp.body_len, 0);
    }

    #[test]
    fn response_500_with_json() {
        let raw = b"HTTP/1.1 500 Internal Server Error\r\n\r\n{\"error\":\"boom\"}";
        let mut buf = [0u8; 128];
        let resp = parse_http_response(raw, &mut buf).unwrap();
        assert_eq!(resp.status_code, 500);
        assert!(buf[..resp.body_len].starts_with(b"{\"error\":\"boom\"}"));
    }

    #[test]
    fn response_with_binance_json() {
        // Real Binance API response format
        let raw = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 49\r\n\r\n{\"symbol\":\"BTCUSDT\",\"price\":\"63617.99000000\"}";
        let mut buf = [0u8; 256];
        let resp = parse_http_response(raw, &mut buf).unwrap();
        assert_eq!(resp.status_code, 200);
        let body = core::str::from_utf8(&buf[..resp.body_len]).unwrap();
        assert!(body.contains("\"price\":\"63617.99000000\""));
    }

    #[test]
    fn response_truncates_body_to_buffer() {
        let raw = b"HTTP/1.1 200 OK\r\n\r\nABCDEFGHIJKLMNOPQRSTUVWXYZ";
        let mut buf = [0u8; 5];
        let resp = parse_http_response(raw, &mut buf).unwrap();
        assert_eq!(resp.status_code, 200);
        assert_eq!(resp.body_len, 5);
        assert_eq!(&buf, b"ABCDE");
    }

    #[test]
    fn response_http_1_0_accepted() {
        let raw = b"HTTP/1.0 200 OK\r\n\r\nbody";
        let mut buf = [0u8; 64];
        let resp = parse_http_response(raw, &mut buf).unwrap();
        assert_eq!(resp.status_code, 200);
        assert_eq!(&buf[..resp.body_len], b"body");
    }

    #[test]
    fn response_invalid_version_rejected() {
        let raw = b"HTTP/2.0 200 OK\r\n\r\nbody";
        let mut buf = [0u8; 64];
        assert!(parse_http_response(raw, &mut buf).is_err());
    }

    #[test]
    fn response_empty_body() {
        let raw = b"HTTP/1.1 204 No Content\r\n\r\n";
        let mut buf = [0u8; 64];
        let resp = parse_http_response(raw, &mut buf).unwrap();
        assert_eq!(resp.status_code, 204);
        assert_eq!(resp.body_len, 0);
    }
}
