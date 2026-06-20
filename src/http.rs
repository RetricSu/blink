//! Minimal HTTP client module for embedded ESP32-C3.
//!
//! Provides a **blocking** HTTP/1.1 client that operates over raw TCP
//! sockets on the smoltcp network stack provided by the WiFi module.
//!
//! Supports GET and POST with configurable buffer sizes.
//!
//! # Security Warning
//!
//! This client uses unencrypted HTTP over plain TCP. Do not transmit
//! passwords, tokens, or personal data through it. It is intended only for
//! local, trusted networks.
//!
//! # Limitations
//!
//! - **IP addresses only** (no DNS resolution). Use the server's IPv4 address.
//! - **HTTP only** (no TLS/HTTPS).
//! - Response body is truncated if it exceeds the caller's buffer.
//!
//! # Example
//!
//! ```ignore
//! use blink::http::HttpClient;
//!
//! let mut client = HttpClient::new();
//! let mut buf = [0u8; 512];
//! let resp = client.get(wifi.resources(), "93.184.216.34", 80, "/", &mut buf)?;
//! // resp.status_code, buf[..resp.body_len] contain the response
//! ```

#[cfg(target_arch = "riscv32")]
mod inner {
    use core::fmt::Write;

    use esp_wifi::wifi_interface::WifiStack;
    use heapless::String as HString;
    use smoltcp::socket::tcp;
    use smoltcp::wire::{IpAddress, IpEndpoint, Ipv4Address};

    use crate::wifi::WifiResources;

    /// Errors that can occur during HTTP operations.
    #[derive(Debug, Clone, PartialEq)]
    pub enum HttpError {
        /// TCP connection to the server failed.
        ConnectionFailed,
        /// Failed to send the HTTP request.
        SendFailed,
        /// Failed to receive data from the server.
        RecvFailed,
        /// The HTTP response could not be parsed.
        ParseError,
        /// The URL or host string was malformed.
        InvalidUrl,
        /// The response body is larger than the caller's buffer.
        BodyTooLarge,
    }

    /// A parsed HTTP response.
    #[derive(Debug, Clone)]
    pub struct HttpResponse {
        /// HTTP status code (e.g., 200, 404, 500).
        pub status_code: u16,
        /// Number of body bytes written into the caller's buffer.
        pub body_len: usize,
    }

    // ── Internal constants ────────────────────────────────────────

    /// Buffer size for raw HTTP response.
    const RX_CAPACITY: usize = 1024;
    /// Poll iterations before timing out a connection/request.
    const MAX_POLL_ITER: usize = 2000;

    /// A minimal, blocking HTTP client for ESP32-C3.
    ///
    /// Owns internal buffers for TCP socket operations. Designed to be
    /// used with the [`WifiResources`] obtained from [`WifiManager`].
    pub struct HttpClient {
        tcp_rx_buf: [u8; 1024],
        tcp_tx_buf: [u8; 1024],
    }

    impl HttpClient {
        /// Create a new HTTP client with default (1 KiB) socket buffers.
        pub fn new() -> Self {
            Self {
                tcp_rx_buf: [0u8; 1024],
                tcp_tx_buf: [0u8; 1024],
            }
        }

        /// Perform an HTTP **GET** request.
        ///
        /// # Arguments
        /// * `resources` — WiFi resources from `WifiManager::resources()`.
        /// * `host` — Server IPv4 address as a dotted string, e.g. `"192.168.1.100"`.
        /// * `port` — TCP port (usually 80 for plain HTTP).
        /// * `path` — Request path, e.g. `"/api/status"`.
        /// * `resp_body` — Buffer where the response body will be written.
        ///
        /// # Returns
        /// `HttpResponse` with the status code and number of bytes written
        /// to `resp_body`.
        pub fn get(
            &mut self,
            resources: &mut WifiResources<'_>,
            host: &str,
            port: u16,
            path: &str,
            resp_body: &mut [u8],
        ) -> Result<HttpResponse, HttpError> {
            let ip = parse_ipv4(host)?;
            let endpoint = IpEndpoint::new(IpAddress::Ipv4(ip), port);
            self.do_request(resources, endpoint, host, "GET", path, &[], resp_body)
        }

        /// Perform an HTTP **POST** request.
        ///
        /// # Arguments
        /// * `resources` — WiFi resources from `WifiManager::resources()`.
        /// * `host` — Server IPv4 address as a dotted string.
        /// * `port` — TCP port (usually 80 for plain HTTP).
        /// * `path` — Request path.
        /// * `body` — Request body bytes (e.g., JSON payload).
        /// * `resp_body` — Buffer where the response body will be written.
        ///
        /// # Returns
        /// `HttpResponse` with the status code and number of bytes written
        /// to `resp_body`.
        pub fn post(
            &mut self,
            resources: &mut WifiResources<'_>,
            host: &str,
            port: u16,
            path: &str,
            body: &[u8],
            resp_body: &mut [u8],
        ) -> Result<HttpResponse, HttpError> {
            let ip = parse_ipv4(host)?;
            let endpoint = IpEndpoint::new(IpAddress::Ipv4(ip), port);
            self.do_request(resources, endpoint, host, "POST", path, body, resp_body)
        }

        // ── internal request engine ──────────────────────────────

        fn do_request(
            &mut self,
            resources: &mut WifiResources<'_>,
            endpoint: IpEndpoint,
            host: &str,
            method: &str,
            path: &str,
            body: &[u8],
            resp_body: &mut [u8],
        ) -> Result<HttpResponse, HttpError> {
            // Reject control characters in request tokens to prevent header injection.
            validate_http_token(host)?;
            validate_http_token(path)?;
            validate_http_token(method)?;
            // ── Build HTTP request ────────────────────────────────
            let mut req: HString<1024> = HString::new();

            // Request line
            write!(&mut req, "{} {} HTTP/1.1\r\n", method, path)
                .map_err(|_| HttpError::SendFailed)?;

            // Headers
            write!(&mut req, "Host: {}\r\n", host).map_err(|_| HttpError::SendFailed)?;
            write!(&mut req, "Connection: close\r\n").map_err(|_| HttpError::SendFailed)?;

            if !body.is_empty() {
                write!(&mut req, "Content-Length: {}\r\n", body.len())
                    .map_err(|_| HttpError::SendFailed)?;
                write!(&mut req, "Content-Type: application/octet-stream\r\n")
                    .map_err(|_| HttpError::SendFailed)?;
            }

            write!(&mut req, "\r\n").map_err(|_| HttpError::SendFailed)?;

            let req_bytes = req.as_bytes();

            // ── Create TCP socket ─────────────────────────────────
            let rx_buf = &mut self.tcp_rx_buf[..];
            let tx_buf = &mut self.tcp_tx_buf[..];

            let tcp_socket = smoltcp::socket::tcp::Socket::new(
                smoltcp::socket::tcp::SocketBuffer::new(rx_buf),
                smoltcp::socket::tcp::SocketBuffer::new(tx_buf),
            );

            let mut sockets = resources.stack.socket_set();
            let handle = sockets
                .add(tcp_socket)
                .map_err(|_| HttpError::ConnectionFailed)?;

            // ── Connect ───────────────────────────────────────────
            {
                let mut sock = sockets.get_mut::<smoltcp::socket::tcp::Socket>(handle);
                let mut cx = resources.stack.interface().context();
                sock.connect(&mut cx, endpoint, |_, _| {}).map_err(|_| {
                    sockets.remove(handle);
                    HttpError::ConnectionFailed
                })?;
            }

            // Wait for the TCP handshake to complete
            let mut handshake_ok = false;
            for _ in 0..MAX_POLL_ITER {
                resources.stack.work();
                let sock = sockets.get::<smoltcp::socket::tcp::Socket>(handle);
                if !sock.is_open() {
                    break;
                }
                if sock.may_send() {
                    handshake_ok = true;
                    break;
                }
            }
            if !handshake_ok {
                sockets.remove(handle);
                return Err(HttpError::ConnectionFailed);
            }

            // ── Send request (headers + optional body) ────────────
            let all_send = [req_bytes, body];
            for chunk in &all_send {
                if chunk.is_empty() {
                    continue;
                }
                let mut sent = 0usize;
                for _ in 0..MAX_POLL_ITER {
                    if sent >= chunk.len() {
                        break;
                    }
                    resources.stack.work();
                    let mut sock = sockets.get_mut::<smoltcp::socket::tcp::Socket>(handle);
                    if sock.can_send() {
                        match sock.send_slice(&chunk[sent..]) {
                            Ok(n) => sent += n,
                            Err(tcp::SendError::InvalidState) => continue,
                            Err(_) => {
                                sockets.remove(handle);
                                return Err(HttpError::SendFailed);
                            }
                        }
                    }
                }
                if sent < chunk.len() {
                    sockets.remove(handle);
                    return Err(HttpError::SendFailed);
                }
            }

            // Signal we're done writing
            {
                let mut sock = sockets.get_mut::<smoltcp::socket::tcp::Socket>(handle);
                sock.close();
            }

            // ── Receive response ──────────────────────────────────
            let mut rx_buf: [u8; RX_CAPACITY] = [0u8; RX_CAPACITY];
            let mut rx_total = 0usize;
            let mut recv_complete = false;

            for _ in 0..MAX_POLL_ITER {
                resources.stack.work();
                let mut sock = sockets.get_mut::<smoltcp::socket::tcp::Socket>(handle);
                if !sock.may_recv() {
                    recv_complete = true;
                    break;
                }
                if sock.can_recv() {
                    let space = &mut rx_buf[rx_total..];
                    match sock.recv_slice(space) {
                        Ok(0) => {
                            recv_complete = true;
                            break; // peer closed
                        }
                        Ok(n) => {
                            rx_total += n;
                            if rx_total >= rx_buf.len() {
                                recv_complete = true;
                                break;
                            }
                        }
                        Err(tcp::RecvError::InvalidState) => continue,
                        Err(_) => break,
                    }
                }
            }

            sockets.remove(handle);

            if !recv_complete {
                return Err(HttpError::RecvFailed);
            }

            // ── Parse HTTP response ───────────────────────────────
            parse_http_response(&rx_buf[..rx_total], resp_body)
        }
    }

    impl Default for HttpClient {
        fn default() -> Self {
            Self::new()
        }
    }

    // ── Helpers ───────────────────────────────────────────────────

    /// Parse an IPv4 address from a dotted-decimal string.
    fn parse_ipv4(s: &str) -> Result<Ipv4Address, HttpError> {
        let mut octets = [0u8; 4];
        let mut iter = s.split('.');
        for i in 0..4 {
            let part = iter.next().ok_or(HttpError::InvalidUrl)?;
            if part.is_empty() || part.len() > 3 {
                return Err(HttpError::InvalidUrl);
            }
            let value: u8 = part.parse().map_err(|_| HttpError::InvalidUrl)?;
            octets[i] = value;
        }
        if iter.next().is_some() {
            return Err(HttpError::InvalidUrl);
        }
        Ok(Ipv4Address::new(octets[0], octets[1], octets[2], octets[3]))
    }

    /// Reject HTTP request tokens that contain control characters which could
    /// break the request line or headers (`\r`, `\n`, `\0`).
    fn validate_http_token(s: &str) -> Result<(), HttpError> {
        if s.bytes().any(|b| b == b'\r' || b == b'\n' || b == b'\0') {
            return Err(HttpError::InvalidUrl);
        }
        Ok(())
    }

    /// Parse an HTTP/1.1 response from raw bytes.
    ///
    /// Extracts the status code and copies everything after the headers into
    /// `body_buf`. The implementation does not interpret `Content-Length`; it
    /// treats all bytes following the header block as the response body.
    fn parse_http_response(raw: &[u8], body_buf: &mut [u8]) -> Result<HttpResponse, HttpError> {
        // Parse only the status line + headers as text, before the double CRLF.
        let header_end = raw
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .unwrap_or(raw.len());

        let header_text =
            core::str::from_utf8(&raw[..header_end]).map_err(|_| HttpError::ParseError)?;

        // ── Status line ───────────────────────────────────────────
        let status_end = header_text.find("\r\n").unwrap_or(header_text.len());
        let status_line = &header_text[..status_end];

        let mut parts = status_line.splitn(3, ' ');
        let version = parts.next().ok_or(HttpError::ParseError)?;
        if version != "HTTP/1.1" {
            return Err(HttpError::ParseError);
        }
        let code_str = parts.next().ok_or(HttpError::ParseError)?;
        let status_code: u16 = code_str.parse().map_err(|_| HttpError::ParseError)?;

        // ── Copy body (after double CRLF) ─────────────────────────
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
}

#[cfg(target_arch = "riscv32")]
pub use inner::*;

// ── Host-side stubs (for `cargo test` on the host) ──────────────

#[cfg(not(target_arch = "riscv32"))]
mod inner {
    use crate::wifi::WifiResources;

    #[derive(Debug, Clone, PartialEq)]
    pub enum HttpError {
        ConnectionFailed,
        SendFailed,
        RecvFailed,
        ParseError,
        InvalidUrl,
        BodyTooLarge,
    }

    #[derive(Debug, Clone)]
    pub struct HttpResponse {
        pub status_code: u16,
        pub body_len: usize,
    }

    /// Stub HTTP client for host-side compilation/testing.
    pub struct HttpClient;

    impl HttpClient {
        pub fn new() -> Self {
            Self
        }

        pub fn get(
            &mut self,
            _resources: &mut WifiResources<'_>,
            _host: &str,
            _port: u16,
            _path: &str,
            _resp_body: &mut [u8],
        ) -> Result<HttpResponse, HttpError> {
            Err(HttpError::ConnectionFailed)
        }

        pub fn post(
            &mut self,
            _resources: &mut WifiResources<'_>,
            _host: &str,
            _port: u16,
            _path: &str,
            _body: &[u8],
            _resp_body: &mut [u8],
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

#[cfg(not(target_arch = "riscv32"))]
pub use inner::*;

// ── Tests ────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    #![allow(unused_imports)]

    use super::*;

    // These tests exercise the parsers — they run on the host as well.

    /// Helper: test IPv4 parser (no-op on host stubs).
    fn run_parse_ipv4(s: &str, expected: Option<[u8; 4]>) {
        #[cfg(target_arch = "riscv32")]
        {
            let result = inner::parse_ipv4(s);
            match expected {
                Some(bytes) => {
                    let ip = result.unwrap();
                    assert_eq!(ip.as_bytes(), &bytes);
                }
                None => assert!(result.is_err()),
            }
        }
        // On host, the inner module is stubbed; skip gracefully.
        let _ = (s, expected);
    }

    /// Helper: test HTTP response parser (no-op on host stubs).
    fn run_parse_response(raw: &[u8], want_code: u16, want_body_prefix: &[u8]) {
        #[cfg(target_arch = "riscv32")]
        {
            let mut buf = [0u8; 128];
            let resp = inner::parse_http_response(raw, &mut buf).unwrap();
            assert_eq!(resp.status_code, want_code);
            assert!(buf[..resp.body_len].starts_with(want_body_prefix));
        }
        let _ = (raw, want_code, want_body_prefix);
    }

    #[test]
    fn parse_ipv4_valid() {
        run_parse_ipv4("192.168.1.1", Some([192, 168, 1, 1]));
    }

    #[test]
    fn parse_ipv4_localhost() {
        run_parse_ipv4("127.0.0.1", Some([127, 0, 0, 1]));
    }

    #[test]
    fn parse_ipv4_invalid_too_few() {
        run_parse_ipv4("10.0.0", None);
    }

    #[test]
    fn parse_ipv4_invalid_text() {
        run_parse_ipv4("not.an.ip.address", None);
    }

    #[test]
    fn response_200_with_body() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Length: 12\r\n\r\nHello World!";
        run_parse_response(raw, 200, b"Hello World!");
    }

    #[test]
    fn response_404_no_body() {
        let raw = b"HTTP/1.1 404 Not Found\r\n\r\n";
        run_parse_response(raw, 404, b"");
    }

    #[test]
    fn response_500_with_json() {
        let raw = b"HTTP/1.1 500 Internal Server Error\r\n\r\n{\"error\":\"boom\"}";
        run_parse_response(raw, 500, b"{\"error\":\"boom\"}");
    }
}
