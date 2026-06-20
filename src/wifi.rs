//! WiFi connection module for ESP32-C3.
//!
//! Provides initialization and management of WiFi connections using `esp-wifi`.
//! After connecting, exposes the network stack for use by higher-level modules
//! like the HTTP client.
//!
//! # Dependencies
//!
//! Designed for `esp-wifi` 0.13.x and `esp-hal` 0.23.x on ESP32-C3.
//! Minor API adjustments may be needed for other versions.
//!
//! # Example
//!
//! ```ignore
//! use blink::wifi::WifiManager;
//!
//! let peripherals = esp_hal::init(config);
//! let clocks = esp_hal::clock::CpuClock::max();
//! let mut wifi = WifiManager::init(peripherals, &clocks).unwrap();
//! wifi.connect("MyAP", "password123").unwrap();
//! // Ready for HTTP requests via blink::http
//! ```

#[cfg(target_arch = "riscv32")]
mod inner {
    use core::sync::atomic::{AtomicBool, Ordering};

    use esp_hal::clock::Clocks;
    use esp_hal::rng::Rng;
    use esp_hal::timer::timg::TimerGroup;
    use esp_wifi::wifi::{ClientConfiguration, Configuration, WifiController, WifiDevice};
    use esp_wifi::wifi_interface::WifiStack;
    use esp_wifi::{initialize, EspWifiInitFor};
    use heapless::String as HString;

    use smoltcp::iface::{Config as IfaceConfig, Interface, SocketSet};
    use smoltcp::time::Instant;
    use smoltcp::wire::{EthernetAddress, HardwareAddress};

    /// Errors that can occur during WiFi operations.
    #[derive(Debug, Clone, PartialEq)]
    pub enum WifiError {
        /// Failed to initialize the WiFi hardware/stack.
        InitFailed,
        /// Failed to connect to the access point (wrong credentials, out of range, etc.).
        ConnectionFailed,
        /// The WiFi connection was lost unexpectedly.
        Disconnected,
        /// Invalid configuration (e.g., SSID too long for the buffer).
        ConfigError,
    }

    /// Holds WiFi state accessible by other modules (HTTP client, etc.).
    pub struct WifiResources<'d> {
        /// The smoltcp network stack for TCP/UDP socket operations.
        pub stack: WifiStack<'d>,
    }

    /// Manages WiFi connection lifecycle for the ESP32-C3.
    ///
    /// Initializes the hardware, connects to access points, and provides
    /// access to the network stack for data transmission.
    pub struct WifiManager<'d> {
        controller: WifiController<'d>,
        resources: WifiResources<'d>,
        connected: bool,
    }

    impl<'d> WifiManager<'d> {
        /// Initialize WiFi hardware and the smoltcp network stack.
        ///
        /// This consumes the ESP32-C3 `Peripherals` to take ownership of the
        /// RADIO, RNG, and TIMG0 hardware blocks. Returns a `WifiManager` that
        /// is ready for connection.
        pub fn init(
            peripherals: esp_hal::peripherals::Peripherals,
            clocks: &Clocks<'_>,
        ) -> Result<Self, WifiError> {
            // The socket set lives in a static mut buffer. Creating a second
            // `WifiManager` would produce a second `&mut` to that buffer,
            // which is undefined behavior. Guard against double initialization.
            static INITED: AtomicBool = AtomicBool::new(false);
            if INITED.swap(true, Ordering::SeqCst) {
                return Err(WifiError::InitFailed);
            }

            // ── esp-wifi initialization ──────────────────────────
            let timg0 = TimerGroup::new(peripherals.TIMG0, clocks);
            let rng = Rng::new(peripherals.RNG);
            let radio_clk = peripherals.RADIO_CLK;

            let init = initialize(EspWifiInitFor::Wifi, timg0.timer0, rng, radio_clk, clocks)
                .map_err(|_| {
                    INITED.store(false, Ordering::SeqCst);
                    WifiError::InitFailed
                })?;

            // The RADIO peripheral is split into WiFi and BLE halves.
            // We only need the WiFi half.
            let (wifi_antenna, _ble) = peripherals.RADIO.split();

            let (wifi_device, controller) = esp_wifi::wifi::new_with_mode(init, wifi_antenna)
                .map_err(|_| {
                    INITED.store(false, Ordering::SeqCst);
                    WifiError::InitFailed
                })?;

            // ── smoltcp network stack ────────────────────────────
            let iface_config =
                IfaceConfig::new(HardwareAddress::Ethernet(EthernetAddress::default()));

            // We use a static buffer for the socket set — in no_std there is no
            // global allocator, so a Vec cannot grow.
            static mut SOCKET_STORAGE: [Option<smoltcp::iface::SocketStorage<'static>>; 4] =
                [const { None }; 4];
            let socket_set = SocketSet::new(unsafe { &mut SOCKET_STORAGE[..] });

            // Clock function: esp-wifi uses this to timestamp packets.
            // The cycle counter runs at the CPU frequency.
            let cpu_mhz = clocks.cpu_clock.to_Hz() / 1_000_000;
            let now = || -> Instant {
                let cycles = riscv::register::mcycle::read64();
                Instant::from_micros((cycles / cpu_mhz) as i64)
            };

            let stack = WifiStack::new(iface_config, wifi_device, socket_set, now);

            Ok(Self {
                controller,
                resources: WifiResources { stack },
                connected: false,
            })
        }

        /// Connect to a WiFi access point (WPA2-Personal).
        ///
        /// This is a blocking call — it will not return until the connection
        /// succeeds or fails. For async operation, use the controller directly.
        pub fn connect(&mut self, ssid: &str, password: &str) -> Result<(), WifiError> {
            if ssid.len() > 32 || password.len() > 64 {
                return Err(WifiError::ConfigError);
            }

            let mut ssid_h: HString<32> = HString::new();
            ssid_h.push_str(ssid).map_err(|_| WifiError::ConfigError)?;

            let mut pwd_h: HString<64> = HString::new();
            pwd_h
                .push_str(password)
                .map_err(|_| WifiError::ConfigError)?;

            let mut client_cfg = ClientConfiguration::default();
            client_cfg.ssid = ssid_h;
            client_cfg.password = pwd_h;

            let conf = Configuration::Client(client_cfg);

            self.controller
                .set_configuration(&conf)
                .map_err(|_| WifiError::ConnectionFailed)?;

            self.controller
                .start()
                .map_err(|_| WifiError::ConnectionFailed)?;

            self.controller
                .connect()
                .map_err(|_| WifiError::ConnectionFailed)?;

            self.connected = true;
            Ok(())
        }

        /// Disconnect from the current access point and stop WiFi.
        pub fn disconnect(&mut self) -> Result<(), WifiError> {
            self.controller
                .disconnect()
                .map_err(|_| WifiError::Disconnected)?;
            // Mark disconnected as soon as the controller reports it; if stop()
            // fails afterwards we still reflect the disconnected state.
            self.connected = false;
            self.controller
                .stop()
                .map_err(|_| WifiError::Disconnected)?;
            Ok(())
        }

        /// Returns `true` when the WiFi link is up.
        pub fn is_connected(&self) -> bool {
            self.connected && self.controller.is_connected().unwrap_or(false)
        }

        /// Get mutable access to WiFi resources (network stack, etc.).
        ///
        /// Pass this to the HTTP client or other networking modules.
        pub fn resources(&mut self) -> &mut WifiResources<'d> {
            &mut self.resources
        }

        /// Drive the network stack — call this regularly in the main loop
        /// to process incoming/outgoing packets and maintain the connection.
        pub fn poll(&mut self) {
            self.resources.stack.work();
        }
    }
}

#[cfg(target_arch = "riscv32")]
pub use inner::*;

// ── Host-side stubs (for `cargo test` on the host) ──────────────

#[cfg(not(target_arch = "riscv32"))]
mod inner {
    use core::marker::PhantomData;

    #[derive(Debug, Clone, PartialEq)]
    pub enum WifiError {
        InitFailed,
        ConnectionFailed,
        Disconnected,
        ConfigError,
    }

    /// Stub WiFi resources struct for host-side compilation.
    pub struct WifiResources<'d> {
        _phantom: PhantomData<&'d ()>,
    }

    /// Stub WiFi manager for host-side compilation.
    pub struct WifiManager<'d> {
        resources: WifiResources<'d>,
    }

    impl<'d> WifiManager<'d> {
        pub fn init(_peripherals: (), _clocks: &()) -> Result<Self, WifiError> {
            Err(WifiError::InitFailed)
        }

        pub fn connect(&mut self, _ssid: &str, _password: &str) -> Result<(), WifiError> {
            Err(WifiError::ConnectionFailed)
        }

        pub fn disconnect(&mut self) -> Result<(), WifiError> {
            Err(WifiError::Disconnected)
        }

        pub fn is_connected(&self) -> bool {
            false
        }

        pub fn resources(&mut self) -> &mut WifiResources<'d> {
            &mut self.resources
        }

        pub fn poll(&mut self) {}
    }
}

#[cfg(not(target_arch = "riscv32"))]
pub use inner::*;
