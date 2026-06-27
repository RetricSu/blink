//! Asset price module for blink.
//!
//! Supports BTC, CKB, and Gold (via PAXGUSDT proxy on Binance) for a basic
//! price-display mode. The module builds Binance API paths, parses the
//! standard ticker response, and provides a deterministic simulation fallback
//! because the current HTTP client is HTTP-only while Binance requires HTTPS.

use core::fmt::Write;
use heapless::String as HString;
#[cfg(all(feature = "network", target_arch = "riscv32"))]
use log::info;

/// Assets shown on the price display mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Asset {
    Btc,
    Ckb,
    Gold,
}

impl Asset {
    /// Position of this asset in [`ALL_ASSETS`].
    pub fn index(&self) -> usize {
        match self {
            Asset::Btc => 0,
            Asset::Ckb => 1,
            Asset::Gold => 2,
        }
    }

    /// Binance trading symbol used for the price endpoint.
    pub fn binance_symbol(&self) -> &'static str {
        match self {
            Asset::Btc => "BTCUSDT",
            Asset::Ckb => "CKBUSDT",
            Asset::Gold => "PAXGUSDT",
        }
    }

    /// Human-readable label shown on the OLED.
    pub fn display_name(&self) -> &'static str {
        match self {
            Asset::Btc => "BTC",
            Asset::Ckb => "CKB",
            Asset::Gold => "GOLD",
        }
    }
}

/// All assets cycled by the price mode, in order.
pub const ALL_ASSETS: [Asset; 3] = [Asset::Btc, Asset::Ckb, Asset::Gold];

/// Format a price for the 128x32 OLED.
///
/// Output is the numeric price rounded to two decimals, e.g. `"65432.10"`.
/// The asset label is drawn separately so the price can use a larger font.
pub fn format_price(price: f64) -> HString<128> {
    let mut s = HString::new();
    // The buffer (128 bytes) is far larger than the formatted output
    // (max two-decimal price), so write cannot fail.
    write!(&mut s, "{:.2}", price).unwrap();
    s
}

/// Format a price with asset-specific precision for the 128x32 OLED.
pub fn format_asset_price(asset: Asset, price: f64) -> HString<128> {
    let mut s = HString::new();
    match asset {
        Asset::Ckb => write!(&mut s, "{:.4}", price).unwrap(),
        Asset::Btc | Asset::Gold => write!(&mut s, "{:.2}", price).unwrap(),
    }
    s
}

/// Build the Binance `/api/v3/ticker/price` request path for a symbol.
pub fn binance_price_path(symbol: &str) -> HString<64> {
    let mut s = HString::new();
    // The path is bounded by the fixed prefix and a 20-character symbol.
    write!(&mut s, "/api/v3/ticker/price?symbol={}", symbol).unwrap();
    s
}

/// Parse the Binance ticker response body: `{"symbol":"...","price":"..."}`.
///
/// This is a small, no_std JSON object walker that looks for the `"price"`
/// string field. It tolerates whitespace and different field ordering, and
/// it will not be fooled by the substring `"price":"` appearing inside
/// another string value.
pub fn parse_price_json(body: &str) -> Option<f64> {
    let mut s = body.trim_start();
    if !s.starts_with('{') {
        return None;
    }
    s = &s[1..];

    loop {
        s = s.trim_start();
        if s.starts_with('}') || s.is_empty() {
            return None;
        }

        let key = parse_json_string(&mut s)?;
        s = s.trim_start();
        if !s.starts_with(':') {
            return None;
        }
        s = &s[1..];
        s = s.trim_start();

        if key == "price" {
            let value = parse_json_string(&mut s)?;
            return value.parse().ok();
        }

        skip_json_value(&mut s)?;

        s = s.trim_start();
        if s.starts_with(',') {
            s = &s[1..];
        } else if s.starts_with('}') {
            return None;
        } else {
            return None;
        }
    }
}

/// Parse a JSON string value (without the surrounding quotes) and advance `s`.
fn parse_json_string<'a>(s: &mut &'a str) -> Option<&'a str> {
    *s = s.trim_start();
    if !s.starts_with('"') {
        return None;
    }

    let bytes = s.as_bytes();
    let mut i = 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => {
                i = i.checked_add(2)?;
                if i > bytes.len() {
                    return None;
                }
            }
            b'"' => {
                let value = &s[1..i];
                *s = &s[i + 1..];
                return Some(value);
            }
            _ => i += 1,
        }
    }
    None
}

/// Skip a single JSON value (string, number, object, array, true, false, null)
/// and advance `s`.
fn skip_json_value(s: &mut &str) -> Option<()> {
    *s = s.trim_start();
    if s.is_empty() {
        return None;
    }

    let c = s.as_bytes()[0];
    match c {
        b'"' => {
            parse_json_string(s)?;
        }
        b'{' => {
            *s = &s[1..];
            loop {
                *s = s.trim_start();
                if s.starts_with('}') {
                    *s = &s[1..];
                    break;
                }
                let _ = parse_json_string(s)?;
                *s = s.trim_start();
                if !s.starts_with(':') {
                    return None;
                }
                *s = &s[1..];
                skip_json_value(s)?;
                *s = s.trim_start();
                if s.starts_with(',') {
                    *s = &s[1..];
                } else if s.starts_with('}') {
                    *s = &s[1..];
                    break;
                } else {
                    return None;
                }
            }
        }
        b'[' => {
            *s = &s[1..];
            loop {
                *s = s.trim_start();
                if s.starts_with(']') {
                    *s = &s[1..];
                    break;
                }
                skip_json_value(s)?;
                *s = s.trim_start();
                if s.starts_with(',') {
                    *s = &s[1..];
                } else if s.starts_with(']') {
                    *s = &s[1..];
                    break;
                } else {
                    return None;
                }
            }
        }
        b't' => {
            if s.starts_with("true") {
                *s = &s[4..];
            } else {
                return None;
            }
        }
        b'f' => {
            if s.starts_with("false") {
                *s = &s[5..];
            } else {
                return None;
            }
        }
        b'n' => {
            if s.starts_with("null") {
                *s = &s[4..];
            } else {
                return None;
            }
        }
        _ => {
            // Number: consume until the next comma, closing brace, or bracket.
            let mut end = 0;
            for (idx, ch) in s.char_indices() {
                if ch == ',' || ch == '}' || ch == ']' {
                    break;
                }
                end = idx + ch.len_utf8();
            }
            if end == 0 {
                return None;
            }
            *s = &s[end..];
        }
    }
    Some(())
}

/// Simulated prices used while the device HTTP client is HTTP-only.
/// These values are deterministic stand-ins for real Binance ticker prices.
const SIMULATED_BTC_PRICE: f64 = 65432.10;
const SIMULATED_CKB_PRICE: f64 = 0.0123;
const SIMULATED_GOLD_PRICE: f64 = 2345.67;

/// Deterministic simulated price for the given asset.
///
/// Used in the basic version because the device's HTTP client cannot speak
/// HTTPS. Once TLS support is added, replace this with a real network call.
pub fn simulate_fetch_price(asset: Asset) -> f64 {
    match asset {
        Asset::Btc => SIMULATED_BTC_PRICE,
        Asset::Ckb => SIMULATED_CKB_PRICE,
        Asset::Gold => SIMULATED_GOLD_PRICE,
    }
}

/// Errors that can occur when fetching a real price from Binance.
#[cfg(all(feature = "network", target_arch = "riscv32"))]
#[derive(Debug)]
pub enum FetchError {
    Http(crate::http::HttpError),
    BadStatus(u16),
    ParseError,
}

#[cfg(all(feature = "network", target_arch = "riscv32"))]
#[derive(Clone, Copy)]
struct BinanceEndpoint {
    host: &'static str,
    tls: bool,
}

#[cfg(all(feature = "network", target_arch = "riscv32"))]
const BINANCE_API_ENDPOINTS: [BinanceEndpoint; 4] = [
    BinanceEndpoint {
        host: "data-api.binance.vision",
        tls: false,
    },
    BinanceEndpoint {
        host: "data-api.binance.vision",
        tls: true,
    },
    BinanceEndpoint {
        host: "api-gcp.binance.com",
        tls: true,
    },
    BinanceEndpoint {
        host: "api1.binance.com",
        tls: true,
    },
];

/// Fetch the live price of `asset` from the Binance HTTPS API.
///
/// Requires a [`NetworkStack`](crate::wifi::NetworkStack) with WiFi connected
/// and DHCP configured. On error, the caller should fall back to
/// [`simulate_fetch_price`].
#[cfg(all(feature = "network", target_arch = "riscv32"))]
pub fn fetch_price(
    stack: &mut crate::wifi::NetworkStack<'_>,
    asset: Asset,
) -> Result<f64, FetchError> {
    use crate::http::HttpClient;
    use log::info;

    info!("Price: fetching {} price", asset.display_name());

    let path = binance_price_path(asset.binance_symbol());
    let mut last_error = FetchError::ParseError;

    for endpoint in BINANCE_API_ENDPOINTS {
        let scheme = if endpoint.tls { "https" } else { "http" };
        info!("Price: fetching from {}://{}", scheme, endpoint.host);

        let mut client = HttpClient::new();
        let mut body_buf = [0u8; 256];
        let result = if endpoint.tls {
            client.get_https(stack, endpoint.host, &path, &mut body_buf)
        } else {
            client.get_http(stack, endpoint.host, &path, &mut body_buf)
        };

        let resp = match result {
            Ok(resp) => resp,
            Err(e) => {
                info!(
                    "Price: failed to fetch {} price from {}://{}: {:?}",
                    asset.display_name(),
                    scheme,
                    endpoint.host,
                    e
                );
                last_error = FetchError::Http(e);
                continue;
            }
        };

        if resp.status_code != 200 {
            info!(
                "Price: failed to fetch {} price from {}://{}, status {}",
                asset.display_name(),
                scheme,
                endpoint.host,
                resp.status_code
            );
            last_error = FetchError::BadStatus(resp.status_code);
            continue;
        }

        info!(
            "Price: received {} price from {}://{}, status {}",
            asset.display_name(),
            scheme,
            endpoint.host,
            resp.status_code
        );

        let body = match core::str::from_utf8(&body_buf[..resp.body_len]) {
            Ok(body) => body,
            Err(_) => {
                last_error = FetchError::ParseError;
                continue;
            }
        };

        match parse_price_json(body) {
            Some(price) => return Ok(price),
            None => last_error = FetchError::ParseError,
        }
    }

    Err(last_error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn btc_symbol_and_name() {
        assert_eq!(Asset::Btc.binance_symbol(), "BTCUSDT");
        assert_eq!(Asset::Btc.display_name(), "BTC");
    }

    #[test]
    fn ckb_symbol_and_name() {
        assert_eq!(Asset::Ckb.binance_symbol(), "CKBUSDT");
        assert_eq!(Asset::Ckb.display_name(), "CKB");
    }

    #[test]
    fn gold_uses_paxg_proxy() {
        assert_eq!(Asset::Gold.binance_symbol(), "PAXGUSDT");
        assert_eq!(Asset::Gold.display_name(), "GOLD");
    }

    #[test]
    fn format_price_rounds_to_two_decimals() {
        let s = format_price(65432.1234);
        assert_eq!(s.as_str(), "65432.12");
    }

    #[test]
    fn format_asset_price_keeps_small_ckb_values_visible() {
        let s = format_asset_price(Asset::Ckb, 0.0042);
        assert_eq!(s.as_str(), "0.0042");
    }

    #[test]
    fn binance_price_path_includes_symbol() {
        let path = binance_price_path("BTCUSDT");
        assert_eq!(path.as_str(), "/api/v3/ticker/price?symbol=BTCUSDT");
    }

    #[test]
    fn parse_valid_binance_response() {
        let body = r#"{"symbol":"BTCUSDT","price":"65432.10"}"#;
        assert_eq!(parse_price_json(body), Some(65432.10));
    }

    #[test]
    fn parse_invalid_response_returns_none() {
        assert_eq!(parse_price_json("not json"), None);
    }

    #[test]
    fn parse_reversed_field_order() {
        let body = r#"{"price":"65432.10","symbol":"BTCUSDT"}"#;
        assert_eq!(parse_price_json(body), Some(65432.10));
    }

    #[test]
    fn parse_tolerates_whitespace() {
        let body = r#"{ "symbol" : "BTCUSDT", "price" : "65432.10" }"#;
        assert_eq!(parse_price_json(body), Some(65432.10));
    }

    #[test]
    fn parse_ignores_price_substring_in_other_string() {
        // The literal "price":" must not match inside the symbol value.
        let body = r#"{"symbol":"PRICEUSDT","price":"123.45"}"#;
        assert_eq!(parse_price_json(body), Some(123.45));
    }

    #[test]
    fn parse_unclosed_string_in_non_price_field_returns_none() {
        // A malformed non-price string value should cause the parser to fail
        // instead of silently skipping it.
        let body = r#"{"symbol":"BTCUSDT"#;
        assert_eq!(parse_price_json(body), None);
    }

    #[test]
    fn parse_unclosed_string_as_price_value_returns_none() {
        let body = r#"{"price":"123.45"#;
        assert_eq!(parse_price_json(body), None);
    }

    #[test]
    fn asset_index_matches_all_assets_order() {
        assert_eq!(Asset::Btc.index(), 0);
        assert_eq!(Asset::Ckb.index(), 1);
        assert_eq!(Asset::Gold.index(), 2);
    }

    #[test]
    fn simulation_returns_nonzero_prices() {
        assert!(simulate_fetch_price(Asset::Btc) > 0.0);
        assert!(simulate_fetch_price(Asset::Ckb) > 0.0);
        assert!(simulate_fetch_price(Asset::Gold) > 0.0);
    }
}
