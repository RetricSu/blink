//! Asset price module for blink.
//!
//! Supports BTC, CKB, and Gold (via PAXGUSDT proxy on Binance) for a basic
//! price-display mode. The module builds Binance API paths, parses the
//! standard ticker response, and provides a deterministic simulation fallback
//! because the current HTTP client is HTTP-only while Binance requires HTTPS.

use core::fmt::Write;
use heapless::String as HString;

/// Assets shown on the price display mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Asset {
    Btc,
    Ckb,
    Gold,
}

impl Asset {
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
/// Output looks like `"BTC: 65432.10"` and fits on a single 6x10 line.
pub fn format_price(asset: Asset, price: f64) -> HString<128> {
    let mut s = HString::new();
    let _ = write!(&mut s, "{}: {:.2}", asset.display_name(), price);
    s
}

/// Build the Binance `/api/v3/ticker/price` request path for a symbol.
pub fn binance_price_path(symbol: &str) -> HString<64> {
    let mut s = HString::new();
    let _ = write!(&mut s, "/api/v3/ticker/price?symbol={}", symbol);
    s
}

/// Parse the Binance ticker response body: `{"symbol":"...","price":"..."}`.
pub fn parse_price_json(body: &str) -> Option<f64> {
    // Minimal JSON parser: find the quoted "price" value.
    let key = "\"price\":\"";
    let start = body.find(key)? + key.len();
    let end = body[start..].find('"')?;
    body[start..start + end].parse().ok()
}

/// Deterministic simulated price for the given asset.
///
/// Used in the basic version because the device's HTTP client cannot speak
/// HTTPS. Once TLS support is added, replace this with a real network call.
pub fn simulate_fetch_price(asset: Asset) -> f64 {
    match asset {
        Asset::Btc => 65432.10,
        Asset::Ckb => 0.0123,
        Asset::Gold => 2345.67,
    }
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
        let s = format_price(Asset::Btc, 65432.1234);
        assert_eq!(s.as_str(), "BTC: 65432.12");
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
    fn simulation_returns_nonzero_prices() {
        assert!(simulate_fetch_price(Asset::Btc) > 0.0);
        assert!(simulate_fetch_price(Asset::Ckb) > 0.0);
        assert!(simulate_fetch_price(Asset::Gold) > 0.0);
    }
}
