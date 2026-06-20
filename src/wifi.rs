//! WiFi + network stack module for ESP32-C3.
//!
//! Provides WiFi connection, DHCP IP configuration, DNS resolution, and TCP
//! socket management using `esp-wifi` 0.13 and `smoltcp` 0.12.
//!
//! # Architecture
//!
//! Unlike the previous implementation which used the removed `WifiStack`
//! helper, this module manually drives a `smoltcp::iface::Interface` backed
//! by a `WifiDevice` (which implements `smoltcp::phy::Device`).
//!
//! The [`NetworkStack`] struct owns:
//! - A `WifiController` for WiFi connection management
//! - A `WifiDevice` as the smoltcp physical-layer device
//! - A `smoltcp::iface::Interface` for the IP stack
//! - A `smoltcp::iface::SocketSet` holding DHCP, DNS, and TCP sockets
//!
//! Call [`NetworkStack::poll`] regularly in the main loop to process packets
//! and maintain the connection.

#[cfg(target_arch = "riscv32")]
mod inner {
    use core::sync::atomic::Ordering;

    use esp_hal::rng::Rng;
    use esp_hal::timer::timg::TimerGroup;
    use esp_wifi::wifi::{
        ClientConfiguration, Configuration, WifiController, WifiDevice, WifiStaDevice,
    };
    use esp_wifi::{wifi, EspWifiController};
    use heapless::String as HString;
    use portable_atomic::AtomicBool;
    use smoltcp::iface::{Config as IfaceConfig, Interface, SocketHandle, SocketSet, SocketStorage};
    use managed::ManagedSlice;
    use smoltcp::socket::dhcpv4::{Socket as Dhcpv4Socket, Event as DhcpEvent};
    use smoltcp::socket::dns;
    use smoltcp::socket::tcp;
    use smoltcp::time::Instant;
    use smoltcp::wire::{
        EthernetAddress, HardwareAddress, IpAddress, IpCidr, Ipv4Address,
    };

    /// Errors that can occur during WiFi/network operations.
    #[derive(Debug, Clone, PartialEq)]
    pub enum WifiError {
        InitFailed,
        ConnectionFailed,
        Disconnected,
        ConfigError,
        NotReady,
        DnsFailed,
        TcpFailed,
    }

    /// Static storage for smoltcp sockets (DHCP + DNS + TCP).
    static mut SOCKET_STORAGE: [SocketStorage<'static>; 8] =
        [const { SocketStorage::EMPTY }; 8];

    /// Static storage for DNS query slots.
    static mut DNS_QUERIES: [Option<dns::DnsQuery>; 2] = [const { None }; 2];

    /// Static storage for the TCP socket buffers.
    static mut TCP_RX_BUF: [u8; 16384] = [0; 16384];
    static mut TCP_TX_BUF: [u8; 4096] = [0; 4096];

    /// Guards exclusive access to the static `TCP_RX_BUF` / `TCP_TX_BUF`
    /// buffers. Because the buffers are `static mut`, lending them to a
    /// `tcp::Socket` while another socket already holds them would create
    /// aliased mutable references — undefined behavior. This lock ensures at
    /// most one TCP connection is active at a time.
    static TCP_IN_USE: AtomicBool = AtomicBool::new(false);

    /// Returns the current time as a `smoltcp::time::Instant`.
    ///
    /// Uses the RISC-V 64-bit cycle counter. On RV32 the counter is split
    /// across the `mcycle` (low) and `mcycleh` (high) CSRs; reading only
    /// `mcycle` would wrap every ~26.8s at 160 MHz and break smoltcp's
    /// internal timers (TCP retransmits, DHCP lease renewals). The carry
    /// loop re-reads `mcycleh` before and after `mcycle` and retries if the
    /// high word changed, handling the race where `mcycle` overflows between
    /// the two reads. The ESP32-C3 runs at 160 MHz when `CpuClock::max()` is
    /// selected (as `main.rs` does).
    #[allow(clippy::unused_assignments, unused_assignments, unused_variables)]
    fn current_instant() -> Instant {
        let lo: u32;
        let hi: u32;
        let hi2: u32;
        unsafe {
            core::arch::asm!(
                "1: csrr {hi}, mcycleh
                   csrr {lo}, mcycle
                   csrr {hi2}, mcycleh
                   bne {hi}, {hi2}, 1b",
                hi = out(reg) hi,
                lo = out(reg) lo,
                hi2 = out(reg) hi2,
                options(nostack, preserves_flags),
            );
        }
        // `hi2` is read inside the asm by the `bne` but the compiler can't
        // see that, hence the function-level allow above.
        let cycles: u64 = ((hi as u64) << 32) | (lo as u64);
        Instant::from_micros((cycles / 160) as i64)
    }

    /// Manages the full WiFi + smoltcp network stack for ESP32-C3.
    ///
    /// Owns the WiFi controller, network interface, and socket set.
    /// Call [`poll`](Self::poll) regularly to drive the stack.
    pub struct NetworkStack<'d> {
        controller: WifiController<'d>,
        device: WifiDevice<'d, WifiStaDevice>,
        iface: Interface,
        sockets: SocketSet<'static>,
        dhcp_handle: SocketHandle,
        dns_handle: Option<SocketHandle>,
        ip_address: Option<Ipv4Address>,
        dns_server: Option<IpAddress>,
        connected: bool,
        network_ready: bool,
    }

    /// Guard that provides `embedded_io` access to a TCP socket.
    ///
    /// Borrows the `Interface`, `WifiDevice`, and `SocketSet` from a
    /// [`NetworkStack`] for the lifetime of the guard.  While this guard
    /// exists the `NetworkStack` cannot be accessed — the guard handles
    /// polling internally.
    pub struct TcpIo<'a, 'd> {
        iface: &'a mut Interface,
        device: &'a mut WifiDevice<'d, WifiStaDevice>,
        sockets: &'a mut SocketSet<'static>,
        handle: SocketHandle,
    }

    /// I/O error for the TCP bridge.
    #[derive(Debug)]
    pub struct TcpIoError;

    impl core::fmt::Display for TcpIoError {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            write!(f, "TCP I/O error")
        }
    }

    impl core::error::Error for TcpIoError {}

    impl embedded_io::Error for TcpIoError {
        fn kind(&self) -> embedded_io::ErrorKind {
            embedded_io::ErrorKind::Other
        }
    }

    impl<'a, 'd> embedded_io::ErrorType for TcpIo<'a, 'd> {
        type Error = TcpIoError;
    }

    impl<'a, 'd> embedded_io::Read for TcpIo<'a, 'd> {
        fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
            if buf.is_empty() {
                return Ok(0);
            }
            // Bound the blocking read so a silently-dropped connection or a
            // server that stops sending without closing cannot hang the
            // firmware forever.
            let start = current_instant();
            let timeout = smoltcp::time::Duration::from_millis(10000);
            loop {
                let now = current_instant();
                if now - start > timeout {
                    return Err(TcpIoError);
                }
                self.iface.poll(now, self.device, self.sockets);
                let sock = self.sockets.get_mut::<tcp::Socket>(self.handle);
                if !sock.may_recv() {
                    return Ok(0); // peer closed or not connected
                }
                if sock.can_recv() {
                    match sock.recv_slice(buf) {
                        Ok(0) => continue,
                        Ok(n) => return Ok(n),
                        Err(tcp::RecvError::InvalidState) => continue,
                        Err(_) => return Err(TcpIoError),
                    }
                }
            }
        }
    }

    impl<'a, 'd> embedded_io::Write for TcpIo<'a, 'd> {
        fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
            if buf.is_empty() {
                return Ok(0);
            }
            let mut total = 0;
            // Same rationale as `read`: bound the blocking write so a full
            // send buffer or a lost connection cannot hang the firmware.
            let start = current_instant();
            let timeout = smoltcp::time::Duration::from_millis(10000);
            while total < buf.len() {
                let now = current_instant();
                if now - start > timeout {
                    return Err(TcpIoError);
                }
                self.iface.poll(now, self.device, self.sockets);
                let sock = self.sockets.get_mut::<tcp::Socket>(self.handle);
                if !sock.may_send() {
                    return Err(TcpIoError);
                }
                if sock.can_send() {
                    match sock.send_slice(&buf[total..]) {
                        Ok(0) => continue,
                        Ok(n) => total += n,
                        Err(tcp::SendError::InvalidState) => continue,
                    }
                }
            }
            Ok(total)
        }

        fn flush(&mut self) -> Result<(), Self::Error> {
            Ok(())
        }
    }

    /// Static storage for the esp-wifi controller. It must outlive the
    /// `WifiDevice` and `WifiController` which borrow from it.
    static mut ESP_WIFI_INIT: Option<EspWifiController<'static>> = None;

    impl<'d> NetworkStack<'d> {
        /// Initialize the WiFi hardware and smoltcp network stack.
        ///
        /// Takes only the peripherals needed for WiFi (TIMG0, RNG, RADIO_CLK,
        /// WIFI) so the caller can retain I2C/GPIO for other uses.
        pub fn init(
            timg0_per: esp_hal::peripherals::TIMG0,
            rng_per: esp_hal::peripherals::RNG,
            radio_clk: esp_hal::peripherals::RADIO_CLK,
            wifi_per: esp_hal::peripherals::WIFI,
        ) -> Result<Self, WifiError> {
            static INITED: AtomicBool = AtomicBool::new(false);
            if INITED.swap(true, Ordering::SeqCst) {
                return Err(WifiError::InitFailed);
            }

            // ── esp-wifi initialization ──────────────────────────
            let timg0 = TimerGroup::new(timg0_per);
            let rng = Rng::new(rng_per);

            let init = esp_wifi::init(timg0.timer0, rng, radio_clk)
                .map_err(|_| {
                    INITED.store(false, Ordering::SeqCst);
                    WifiError::InitFailed
                })?;

            // Store the EspWifiController in a static so it lives for the
            // entire program. The lifetime in EspWifiController<'d> is phantom
            // (PhantomData<&'d ()>), so transmuting to 'static is sound.
            let init_ref: &'static EspWifiController<'static> = unsafe {
                let ptr = core::ptr::addr_of_mut!(ESP_WIFI_INIT);
                ESP_WIFI_INIT = Some(core::mem::transmute::<
                    EspWifiController<'_>,
                    EspWifiController<'static>,
                >(init));
                (*ptr).as_ref().unwrap()
            };

            // ── WiFi device + controller ─────────────────────────
            let (mut device, controller) =
                wifi::new_with_mode(init_ref, wifi_per, WifiStaDevice).map_err(|_| {
                    INITED.store(false, Ordering::SeqCst);
                    WifiError::InitFailed
                })?;

            // ── smoltcp interface ────────────────────────────────
            let mac = device.mac_address();
            let hw_addr =
                HardwareAddress::Ethernet(EthernetAddress::from_bytes(&mac));
            let config = IfaceConfig::new(hw_addr);
            let now = current_instant();
            let iface = Interface::new(config, &mut device, now);

            // ── Socket set with DHCP ─────────────────────────────
            let mut sockets =
                SocketSet::new(ManagedSlice::Borrowed(unsafe { &mut SOCKET_STORAGE[..] }));
            let dhcp = Dhcpv4Socket::new();
            let dhcp_handle = sockets.add(dhcp);

            Ok(Self {
                controller,
                device,
                iface,
                sockets,
                dhcp_handle,
                dns_handle: None,
                ip_address: None,
                dns_server: None,
                connected: false,
                network_ready: false,
            })
        }

        /// Connect to a WiFi access point (WPA2-Personal).
        ///
        /// Blocking call — returns once the WiFi link is up.
        pub fn connect(&mut self, ssid: &str, password: &str) -> Result<(), WifiError> {
            if ssid.len() > 32 || password.len() > 64 {
                return Err(WifiError::ConfigError);
            }

            let mut ssid_h: HString<32> = HString::new();
            ssid_h.push_str(ssid).map_err(|_| WifiError::ConfigError)?;

            let mut pwd_h: HString<64> = HString::new();
            pwd_h.push_str(password).map_err(|_| WifiError::ConfigError)?;

            let client_cfg = ClientConfiguration {
                ssid: ssid_h,
                password: pwd_h,
                ..Default::default()
            };

            self.controller
                .set_configuration(&Configuration::Client(client_cfg))
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

        /// Drive the network stack — call regularly in the main loop.
        ///
        /// Polls the smoltcp interface and processes DHCP events (IP
        /// address assignment, DNS server discovery).
        pub fn poll(&mut self) {
            let now = current_instant();
            self.iface.poll(now, &mut self.device, &mut self.sockets);
            self.handle_dhcp();
        }

        /// Process DHCP events and update interface configuration.
        fn handle_dhcp(&mut self) {
            // Extract DHCP event data without holding borrows on sockets.
            let event = {
                let dhcp =
                    self.sockets.get_mut::<Dhcpv4Socket>(self.dhcp_handle);
                match dhcp.poll() {
                    Some(DhcpEvent::Configured(config)) => {
                        let dns = config.dns_servers.first().copied();
                        Some((true, Some(config.address), config.router, dns))
                    }
                    Some(DhcpEvent::Deconfigured) => Some((false, None, None, None)),
                    None => None,
                }
            };

            if let Some((configured, address, router, dns)) = event {
                if configured {
                    if let Some(cidr) = address {
                        self.ip_address = Some(cidr.address());
                        self.iface.update_ip_addrs(|addrs| {
                            // Replace any existing IPv4 address.
                            addrs.clear();
                            addrs.push(IpCidr::Ipv4(cidr)).ok();
                        });
                    }
                    if let Some(router) = router {
                        self.iface
                            .routes_mut()
                            .add_default_ipv4_route(router)
                            .ok();
                    }
                    if let Some(dns) = dns {
                        self.dns_server = Some(IpAddress::Ipv4(dns));
                        if self.dns_handle.is_none() {
                            let servers = [IpAddress::Ipv4(dns)];
                            let dns_socket = dns::Socket::new(
                                &servers,
                                ManagedSlice::Borrowed(unsafe { &mut DNS_QUERIES[..] }),
                            );
                            self.dns_handle = Some(self.sockets.add(dns_socket));
                        }
                    }
                    self.network_ready = self.ip_address.is_some();
                } else {
                    // Deconfigured — lost IP
                    self.ip_address = None;
                    self.dns_server = None;
                    self.network_ready = false;
                    if let Some(handle) = self.dns_handle.take() {
                        self.sockets.remove(handle);
                    }
                }
            }
        }

        /// Returns `true` when WiFi is associated and DHCP has provided an IP.
        pub fn is_network_ready(&self) -> bool {
            self.network_ready
        }

        /// Returns `true` when the WiFi link is up.
        pub fn is_connected(&self) -> bool {
            self.connected && self.controller.is_connected().unwrap_or(false)
        }

        /// Resolve a hostname to an IP address via DNS.
        ///
        /// Blocking — polls the interface until the query completes or times
        /// out. Requires DHCP to have completed (DNS server must be known).
        pub fn resolve_dns(&mut self, hostname: &str) -> Result<IpAddress, WifiError> {
            let dns_handle = self.dns_handle.ok_or(WifiError::NotReady)?;

            // Start the DNS query — smoltcp 0.12 start_query takes
            // (Context, name, query_type). The DNS server is already
            // configured on the socket.
            let query_handle = {
                let mut cx = self.iface.context();
                let dns_sock =
                    self.sockets.get_mut::<dns::Socket>(dns_handle);
                dns_sock
                    .start_query(&mut cx, hostname, smoltcp::wire::DnsQueryType::A)
                    .map_err(|_| WifiError::DnsFailed)?
            };

            // Poll until the query completes or times out. A fixed iteration
            // count would expire in microseconds on a 160 MHz core, far too
            // short for a real DNS round-trip — use wall-clock time instead.
            let start = current_instant();
            let timeout = smoltcp::time::Duration::from_millis(5000);
            loop {
                let now = current_instant();
                if now - start > timeout {
                    return Err(WifiError::DnsFailed);
                }
                self.iface.poll(now, &mut self.device, &mut self.sockets);

                let dns_sock =
                    self.sockets.get_mut::<dns::Socket>(dns_handle);
                match dns_sock.get_query_result(query_handle) {
                    Ok(addrs) => {
                        let addr = addrs.first().copied().ok_or(WifiError::DnsFailed)?;
                        return Ok(addr);
                    }
                    Err(dns::GetQueryResultError::Pending) => continue,
                    Err(_) => return Err(WifiError::DnsFailed),
                }
            }
        }

        /// Open a TCP connection to `addr:port`.
        ///
        /// Returns a `SocketHandle` that can be used with [`tcp_io`](Self::tcp_io)
        /// and [`tcp_close`](Self::tcp_close). Uses static internal buffers, so
        /// only one TCP connection can be active at a time.
        pub fn tcp_connect(
            &mut self,
            addr: IpAddress,
            port: u16,
        ) -> Result<SocketHandle, WifiError> {
            if !self.is_network_ready() {
                return Err(WifiError::NotReady);
            }

            // The TCP socket buffers are `static mut`; lending them to more
            // than one socket at a time would create aliased mutable
            // references (undefined behavior). Acquire the lock first.
            if TCP_IN_USE.swap(true, Ordering::SeqCst) {
                return Err(WifiError::TcpFailed);
            }

            let socket = tcp::Socket::new(
                tcp::SocketBuffer::new(unsafe { &mut TCP_RX_BUF[..] }),
                tcp::SocketBuffer::new(unsafe { &mut TCP_TX_BUF[..] }),
            );
            let handle = self.sockets.add(socket);

            // Connect — smoltcp 0.12 takes (cx, remote, local).
            let local_port = 4096u16; // arbitrary ephemeral port
            {
                let mut cx = self.iface.context();
                let sock =
                    self.sockets.get_mut::<tcp::Socket>(handle);
                let remote = smoltcp::wire::IpEndpoint::new(addr, port);
                let local = smoltcp::wire::IpListenEndpoint {
                    addr: None,
                    port: local_port,
                };
                if sock.connect(&mut cx, remote, local).is_err() {
                    self.sockets.remove(handle);
                    TCP_IN_USE.store(false, Ordering::SeqCst);
                    return Err(WifiError::TcpFailed);
                }
            }

            // Wait for the TCP handshake to complete. A fixed iteration
            // count would expire in microseconds on a 160 MHz core, before
            // a real handshake can finish — use wall-clock time instead.
            let start = current_instant();
            let timeout = smoltcp::time::Duration::from_millis(5000);
            loop {
                let now = current_instant();
                if now - start > timeout {
                    self.sockets.remove(handle);
                    TCP_IN_USE.store(false, Ordering::SeqCst);
                    return Err(WifiError::TcpFailed);
                }
                self.iface.poll(now, &mut self.device, &mut self.sockets);
                let sock =
                    self.sockets.get_mut::<tcp::Socket>(handle);
                if !sock.is_open() {
                    self.sockets.remove(handle);
                    TCP_IN_USE.store(false, Ordering::SeqCst);
                    return Err(WifiError::TcpFailed);
                }
                if sock.may_send() {
                    return Ok(handle);
                }
            }
        }

        /// Create an `embedded_io` bridge for a TCP socket.
        ///
        /// While the returned guard is alive, the `NetworkStack` cannot be
        /// accessed — the guard handles interface polling internally.
        pub fn tcp_io(&mut self, handle: SocketHandle) -> TcpIo<'_, 'd> {
            TcpIo {
                iface: &mut self.iface,
                device: &mut self.device,
                sockets: &mut self.sockets,
                handle,
            }
        }

        /// Close and remove a TCP socket.
        pub fn tcp_close(&mut self, handle: SocketHandle) {
            self.sockets.remove(handle);
            TCP_IN_USE.store(false, Ordering::SeqCst);
        }
    }
}

#[cfg(target_arch = "riscv32")]
pub use inner::*;

// ── Host-side stubs (for `cargo test` on the host) ──────────────

#[cfg(not(target_arch = "riscv32"))]
mod inner {
    use core::marker::PhantomData;
    use smoltcp::wire::IpAddress;

    #[derive(Debug, Clone, PartialEq)]
    pub enum WifiError {
        InitFailed,
        ConnectionFailed,
        Disconnected,
        ConfigError,
        NotReady,
        DnsFailed,
        TcpFailed,
    }

    /// Stub network stack for host-side compilation.
    pub struct NetworkStack<'d> {
        _phantom: PhantomData<&'d ()>,
    }

    impl<'d> NetworkStack<'d> {
        pub fn init() -> Result<Self, WifiError> {
            Err(WifiError::InitFailed)
        }

        pub fn connect(&mut self, _ssid: &str, _password: &str) -> Result<(), WifiError> {
            Err(WifiError::ConnectionFailed)
        }

        pub fn poll(&mut self) {}

        pub fn is_connected(&self) -> bool {
            false
        }

        pub fn is_network_ready(&self) -> bool {
            false
        }

        pub fn resolve_dns(&mut self, _hostname: &str) -> Result<IpAddress, WifiError> {
            Err(WifiError::DnsFailed)
        }
    }
}

#[cfg(not(target_arch = "riscv32"))]
pub use inner::*;
