use super::config::{IpNet, WgConfig};
use crate::{BoxedStream, DEFAULT_CONNECT_TIMEOUT, Transport, TransportError};
use async_trait::async_trait;
use boringtun::noise::errors::WireGuardError;
use boringtun::noise::{Tunn, TunnResult};
use boringtun::x25519::{PublicKey, StaticSecret};
use smoltcp::iface::{Config, Interface, SocketHandle, SocketSet};
use smoltcp::phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken};
use smoltcp::socket::dns::{self, GetQueryResultError};
use smoltcp::socket::{Socket, tcp};
use smoltcp::time::Instant as SmolInstant;
use smoltcp::wire::{DnsQueryType, HardwareAddress, IpAddress, IpCidr, Ipv4Address, Ipv6Address};
use std::collections::VecDeque;
use std::future::poll_fn;
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::UdpSocket;
use tokio::sync::{Notify, watch};

/// Empfangs- und Sendepuffer je TCP-Verbindung. Groß genug für zügige
/// Dateiübertragungen, klein genug für viele gleichzeitige Sitzungen.
const TCP_RX_BUFFER: usize = 512 * 1024;
const TCP_TX_BUFFER: usize = 256 * 1024;
const UDP_BUFFER: usize = 4 * 1024 * 1024;
/// WireGuard verwirft Sitzungen nach 180 Sekunden ohne neuen Handshake.
const HANDSHAKE_VALID: Duration = Duration::from_secs(180);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
const DNS_TIMEOUT: Duration = Duration::from_secs(5);
const TIMER_INTERVAL: Duration = Duration::from_millis(100);
const MAX_IDLE_SLEEP: Duration = Duration::from_millis(250);
/// Frist, in der eine ordentlich geschlossene Verbindung ausklingen darf.
const LINGER: Duration = Duration::from_secs(10);
const FIRST_EPHEMERAL_PORT: u16 = 49152;
/// Pause, bevor nach einem aufgegebenen Handshake erneut versucht wird.
const HANDSHAKE_RETRY: Duration = Duration::from_secs(10);
/// Kennung eines WireGuard-Datenpakets (erstes Byte).
const PACKET_TYPE_DATA: u8 = 4;
/// Höchstzahl UDP-Pakete, die am Stück verarbeitet werden, bevor der
/// TCP/IP-Stack wieder an die Reihe kommt.
const RECV_BATCH: usize = 64;

type Datagram = (Vec<u8>, SocketAddr);

fn smol_now() -> SmolInstant {
    static START: OnceLock<Instant> = OnceLock::new();
    SmolInstant::from_micros(START.get_or_init(Instant::now).elapsed().as_micros() as i64)
}

// ------------------------------------------------------------------ Virtuelles Gerät

/// Verbindet den TCP/IP-Stack mit WireGuard: zwei Warteschlangen mit IP-Paketen.
struct VirtualDevice {
    rx: VecDeque<Vec<u8>>,
    tx: VecDeque<Vec<u8>>,
    mtu: usize,
}

struct RxTok(Vec<u8>);
struct TxTok<'a>(&'a mut VecDeque<Vec<u8>>);

impl RxToken for RxTok {
    fn consume<R, F: FnOnce(&[u8]) -> R>(self, f: F) -> R {
        f(&self.0)
    }
}

impl TxToken for TxTok<'_> {
    fn consume<R, F: FnOnce(&mut [u8]) -> R>(self, len: usize, f: F) -> R {
        let mut packet = vec![0u8; len];
        let result = f(&mut packet);
        self.0.push_back(packet);
        result
    }
}

impl Device for VirtualDevice {
    type RxToken<'a> = RxTok;
    type TxToken<'a> = TxTok<'a>;

    fn receive(&mut self, _now: SmolInstant) -> Option<(RxTok, TxTok<'_>)> {
        let packet = self.rx.pop_front()?;
        Some((RxTok(packet), TxTok(&mut self.tx)))
    }

    fn transmit(&mut self, _now: SmolInstant) -> Option<TxTok<'_>> {
        Some(TxTok(&mut self.tx))
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut caps = DeviceCapabilities::default();
        caps.medium = Medium::Ip;
        caps.max_transmission_unit = self.mtu;
        caps
    }
}

// ------------------------------------------------------------------ Gemeinsamer Zustand

struct Peer {
    tunn: Tunn,
    endpoint: SocketAddr,
    allowed: Vec<IpNet>,
    /// Zeitpunkt des nächsten Verbindungsversuchs, nachdem WireGuard den
    /// Handshake aufgegeben hat.
    retry_at: Option<Instant>,
}

impl Peer {
    fn handshake_is_fresh(&self) -> bool {
        self.tunn.time_since_last_handshake().is_some_and(|age| age < HANDSHAKE_VALID)
    }
}

struct Inner {
    iface: Interface,
    device: VirtualDevice,
    sockets: SocketSet<'static>,
    peers: Vec<Peer>,
    dns: Option<SocketHandle>,
    next_port: u16,
    /// Abgebrochene Verbindungen; werden nach dem nächsten Durchlauf entfernt,
    /// damit die Gegenseite noch ein RST erhält.
    graveyard: Vec<SocketHandle>,
    /// Ordentlich geschlossene Verbindungen, die noch ausklingen.
    lingering: Vec<(SocketHandle, Instant)>,
    scratch: Vec<u8>,
    want_handshake: bool,
    closed: bool,
}

struct Shared {
    inner: Mutex<Inner>,
    /// Weckt die Tunnelschleife, wenn ein Strom Daten geschrieben oder gelesen hat.
    notify: Notify,
    /// Zählt authentifizierte Pakete der Gegenstelle; dient als Signal für
    /// „Handshake ist da".
    alive: watch::Sender<u64>,
    dns_servers: Vec<IpAddr>,
    has_ipv6: bool,
    description: String,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Beendet den Tunnel, sobald weder das Tunnelobjekt noch ein Strom mehr lebt.
struct Guard(Arc<Shared>);

impl Drop for Guard {
    fn drop(&mut self) {
        self.0.lock().closed = true;
        self.0.notify.notify_one();
    }
}

/// Zustand einer Gegenstelle für Statusanzeigen.
#[derive(Debug, Clone, PartialEq)]
pub struct PeerStatus {
    pub endpoint: SocketAddr,
    /// Zeit seit dem letzten erfolgreichen Handshake.
    pub last_handshake: Option<Duration>,
    pub tx_bytes: u64,
    pub rx_bytes: u64,
    /// Laufzeit des letzten Handshakes in Millisekunden.
    pub rtt_ms: Option<u32>,
}

/// Laufender WireGuard-Tunnel. Günstig zu klonen; der Tunnel endet, wenn der
/// letzte Klon und der letzte Strom verworfen wurden.
#[derive(Clone)]
pub struct WgTunnel {
    shared: Arc<Shared>,
    guard: Arc<Guard>,
    connect_timeout: Duration,
}

impl WgTunnel {
    /// Startet den Tunnel. Der Handshake läuft im Hintergrund an; Fehler in
    /// Schlüsseln oder Erreichbarkeit zeigen sich beim ersten Verbinden.
    pub async fn start(config: WgConfig) -> Result<Self, TransportError> {
        let mut peers = Vec::new();
        let mut family_v4: Option<bool> = None;
        for peer in &config.peers {
            let mut addrs: Vec<SocketAddr> = tokio::net::lookup_host(peer.endpoint.as_str())
                .await
                .map_err(|_| TransportError::Resolve(peer.endpoint.clone()))?
                .collect();
            addrs.sort_by_key(|a| !a.is_ipv4());
            let endpoint = *addrs.first().ok_or_else(|| TransportError::Resolve(peer.endpoint.clone()))?;
            match family_v4 {
                None => family_v4 = Some(endpoint.is_ipv4()),
                Some(v4) if v4 != endpoint.is_ipv4() => {
                    return Err(TransportError::Config(
                        "Endpoints mit IPv4 und IPv6 lassen sich in einem Profil nicht mischen".into(),
                    ));
                }
                Some(_) => {}
            }
            let index = u32::from_le_bytes(
                tephra_crypto::random_bytes::<4>().map_err(|e| TransportError::Config(e.to_string()))?,
            ) >> 8;
            let tunn = Tunn::new(
                StaticSecret::from(config.private_key),
                PublicKey::from(peer.public_key),
                peer.preshared_key,
                peer.keepalive,
                index,
                None,
            );
            peers.push(Peer { tunn, endpoint, allowed: peer.allowed_ips.clone(), retry_at: None });
        }
        let port = config.listen_port.unwrap_or(0);
        let bind: SocketAddr = if family_v4.unwrap_or(true) {
            (std::net::Ipv4Addr::UNSPECIFIED, port).into()
        } else {
            (std::net::Ipv6Addr::UNSPECIFIED, port).into()
        };
        let udp = bind_udp(bind)?;

        let mut device = VirtualDevice { rx: VecDeque::new(), tx: VecDeque::new(), mtu: usize::from(config.mtu) };
        let iface_seed =
            u64::from_le_bytes(tephra_crypto::random_bytes::<8>().map_err(|e| TransportError::Config(e.to_string()))?);
        let mut iface_config = Config::new(HardwareAddress::Ip);
        iface_config.random_seed = iface_seed;
        let mut iface = Interface::new(iface_config, &mut device, smol_now());
        let mut rejected = 0;
        iface.update_ip_addrs(|addrs| {
            for net in &config.addresses {
                if addrs.push(IpCidr::new(IpAddress::from(net.addr), net.prefix)).is_err() {
                    rejected += 1;
                }
            }
        });
        if rejected > 0 {
            return Err(TransportError::Config("Zu viele eigene Adressen im Profil (höchstens vier)".into()));
        }
        // Ziele außerhalb des eigenen Netzes brauchen eine Route. Das Gateway
        // ist ohne Bedeutung, weil die Gegenstelle anhand der AllowedIPs
        // gewählt wird.
        let _ = iface.routes_mut().add_default_ipv4_route(Ipv4Address::new(0, 0, 0, 1));
        let _ = iface.routes_mut().add_default_ipv6_route(Ipv6Address::new(0xfe80, 0, 0, 0, 0, 0, 0, 1));

        let description =
            format!("WireGuard {}", config.peers.iter().map(|p| p.endpoint.as_str()).collect::<Vec<_>>().join(", "));
        let (alive, _) = watch::channel(0u64);
        let shared = Arc::new(Shared {
            inner: Mutex::new(Inner {
                iface,
                device,
                sockets: SocketSet::new(Vec::new()),
                peers,
                dns: None,
                // Zufälliger Startwert, damit ein neu gestarteter Tunnel nicht
                // dieselben Ports wie sein Vorgänger verwendet.
                next_port: FIRST_EPHEMERAL_PORT + (iface_seed % u64::from(u16::MAX - FIRST_EPHEMERAL_PORT)) as u16,
                graveyard: Vec::new(),
                lingering: Vec::new(),
                scratch: vec![0u8; 65536],
                want_handshake: true,
                closed: false,
            }),
            notify: Notify::new(),
            alive,
            dns_servers: config.dns.clone(),
            has_ipv6: config.addresses.iter().any(|n| n.addr.is_ipv6()),
            description,
        });
        tokio::spawn(run(shared.clone(), udp, config.simulate_loss));
        Ok(Self { guard: Arc::new(Guard(shared.clone())), shared, connect_timeout: DEFAULT_CONNECT_TIMEOUT })
    }

    pub fn with_connect_timeout(mut self, timeout: Duration) -> Self {
        self.connect_timeout = timeout;
        self
    }

    /// Anzahl der Tunnelobjekte und Ströme, die diesen Tunnel gerade halten.
    pub fn holders(&self) -> usize {
        Arc::strong_count(&self.guard)
    }

    /// Zustand aller Gegenstellen.
    pub fn status(&self) -> Vec<PeerStatus> {
        self.shared
            .lock()
            .peers
            .iter()
            .map(|peer| {
                let (last_handshake, tx, rx, _loss, rtt_ms) = peer.tunn.stats();
                PeerStatus { endpoint: peer.endpoint, last_handshake, tx_bytes: tx as u64, rx_bytes: rx as u64, rtt_ms }
            })
            .collect()
    }

    /// Wartet, bis mit der für `target` zuständigen Gegenstelle ein gültiger
    /// Handshake besteht. Liefert damit eine verständliche Fehlermeldung,
    /// statt später nur in eine TCP-Zeitüberschreitung zu laufen.
    pub async fn wait_ready(&self, target: IpAddr, timeout: Duration) -> Result<(), TransportError> {
        let mut alive = self.shared.alive.subscribe();
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let endpoint = {
                let mut inner = self.shared.lock();
                if inner.closed {
                    return Err(TransportError::TunnelClosed);
                }
                let index = route(&inner.peers, target).ok_or_else(|| TransportError::NoRoute(target.to_string()))?;
                if inner.peers[index].handshake_is_fresh() {
                    return Ok(());
                }
                inner.want_handshake = true;
                inner.peers[index].endpoint
            };
            self.shared.notify.notify_one();
            match tokio::time::timeout_at(deadline, alive.changed()).await {
                Ok(Ok(())) => {}
                Ok(Err(_)) => return Err(TransportError::TunnelClosed),
                Err(_) => return Err(TransportError::NoHandshake(endpoint.to_string())),
            }
        }
    }

    /// Öffnet eine TCP-Verbindung durch den Tunnel.
    pub async fn connect(&self, addr: SocketAddr) -> Result<TunnelStream, TransportError> {
        let target = addr.to_string();
        let handle = {
            let mut inner = self.shared.lock();
            if inner.closed {
                return Err(TransportError::TunnelClosed);
            }
            if route(&inner.peers, addr.ip()).is_none() {
                return Err(TransportError::NoRoute(addr.ip().to_string()));
            }
            let mut socket = new_tcp_socket();
            let port = inner.next_local_port();
            socket
                .connect(inner.iface.context(), addr, port)
                .map_err(|e| TransportError::Connect { target: target.clone(), reason: e.to_string() })?;
            inner.sockets.add(socket)
        };
        self.shared.notify.notify_one();
        // Ab hier räumt der Strom auf, falls der Aufbau scheitert oder abgebrochen wird.
        let stream = TunnelStream { shared: self.shared.clone(), _guard: self.guard.clone(), handle, shutdown: false };

        let shared = &self.shared;
        let established = poll_fn(|cx| {
            let mut inner = shared.lock();
            if inner.closed {
                return Poll::Ready(Err(TransportError::TunnelClosed));
            }
            let socket = inner.sockets.get_mut::<tcp::Socket>(handle);
            match socket.state() {
                tcp::State::Established => Poll::Ready(Ok(())),
                tcp::State::Closed => Poll::Ready(Err(TransportError::Connect {
                    target: target.clone(),
                    reason: "Verbindung abgelehnt".into(),
                })),
                _ => {
                    socket.register_send_waker(cx.waker());
                    Poll::Pending
                }
            }
        });
        match tokio::time::timeout(self.connect_timeout, established).await {
            Ok(Ok(())) => Ok(stream),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(TransportError::Timeout(addr.to_string())),
        }
    }

    /// Nimmt TCP-Verbindungen an, die durch den Tunnel auf `port` eingehen.
    pub fn listen(&self, port: u16) -> Result<TunnelListener, TransportError> {
        let mut inner = self.shared.lock();
        if inner.closed {
            return Err(TransportError::TunnelClosed);
        }
        let handle = add_listener(&mut inner, port)?;
        Ok(TunnelListener { shared: self.shared.clone(), guard: self.guard.clone(), port, current: handle })
    }

    /// Löst einen Namen über die DNS-Server des Profils auf – durch den Tunnel.
    pub async fn resolve(&self, name: &str) -> Result<Vec<IpAddr>, TransportError> {
        if self.shared.dns_servers.is_empty() {
            return Err(TransportError::Resolve(format!("{name} (im VPN-Profil ist kein DNS-Server eingetragen)")));
        }
        for server in &self.shared.dns_servers {
            self.wait_ready(*server, HANDSHAKE_TIMEOUT).await?;
        }
        let mut found = self.query(name, DnsQueryType::A).await.unwrap_or_default();
        if found.is_empty() && self.shared.has_ipv6 {
            found = self.query(name, DnsQueryType::Aaaa).await.unwrap_or_default();
        }
        if found.is_empty() {
            return Err(TransportError::Resolve(name.to_string()));
        }
        Ok(found)
    }

    async fn query(&self, name: &str, kind: DnsQueryType) -> Result<Vec<IpAddr>, TransportError> {
        let fail = || TransportError::Resolve(name.to_string());
        let (socket_handle, query) = {
            let mut inner = self.shared.lock();
            if inner.closed {
                return Err(TransportError::TunnelClosed);
            }
            let socket_handle = match inner.dns {
                Some(handle) => handle,
                None => {
                    let servers: Vec<IpAddress> =
                        self.shared.dns_servers.iter().copied().map(IpAddress::from).collect();
                    let handle = inner.sockets.add(dns::Socket::new(&servers, Vec::new()));
                    inner.dns = Some(handle);
                    handle
                }
            };
            let Inner { iface, sockets, .. } = &mut *inner;
            let query = sockets
                .get_mut::<dns::Socket>(socket_handle)
                .start_query(iface.context(), name, kind)
                .map_err(|_| fail())?;
            (socket_handle, query)
        };
        self.shared.notify.notify_one();

        // Gibt den Abfrageplatz frei, falls die Abfrage abgebrochen wird.
        struct Cancel<'a> {
            shared: &'a Shared,
            socket: SocketHandle,
            query: Option<dns::QueryHandle>,
        }
        impl Drop for Cancel<'_> {
            fn drop(&mut self) {
                if let Some(query) = self.query.take() {
                    self.shared.lock().sockets.get_mut::<dns::Socket>(self.socket).cancel_query(query);
                }
            }
        }
        let mut cancel = Cancel { shared: &self.shared, socket: socket_handle, query: Some(query) };

        let shared = &self.shared;
        let answer = poll_fn(|cx| {
            let mut inner = shared.lock();
            let socket = inner.sockets.get_mut::<dns::Socket>(socket_handle);
            match socket.get_query_result(query) {
                Ok(addrs) => Poll::Ready(Ok(addrs.into_iter().map(IpAddr::from).collect::<Vec<_>>())),
                Err(GetQueryResultError::Pending) => {
                    socket.register_query_waker(query, cx.waker());
                    Poll::Pending
                }
                Err(GetQueryResultError::Failed) => Poll::Ready(Err(())),
            }
        });
        match tokio::time::timeout(DNS_TIMEOUT, answer).await {
            Ok(result) => {
                // Der Stack hat den Platz bereits selbst freigegeben.
                cancel.query = None;
                result.map_err(|_| fail())
            }
            Err(_) => Err(fail()),
        }
    }
}

#[async_trait]
impl Transport for WgTunnel {
    async fn connect_tcp(&self, host: &str, port: u16) -> Result<BoxedStream, TransportError> {
        let ip = match host.parse::<IpAddr>() {
            Ok(ip) => ip,
            // Namen werden ausschließlich durch den Tunnel aufgelöst. Ein
            // Rückfall auf die Namensauflösung des Geräts würde interne
            // Hostnamen an fremde DNS-Server verraten.
            Err(_) => *self.resolve(host).await?.first().ok_or_else(|| TransportError::Resolve(host.to_string()))?,
        };
        self.wait_ready(ip, HANDSHAKE_TIMEOUT).await?;
        Ok(Box::new(self.connect(SocketAddr::new(ip, port)).await?))
    }

    fn describe(&self) -> String {
        self.shared.description.clone()
    }
}

/// Öffnet den UDP-Socket mit großzügigen Puffern. Die Standardgröße reicht
/// für wenige Dutzend Pakete; bei Dateiübertragungen gingen sonst Pakete
/// schon im eigenen Betriebssystem verloren.
fn bind_udp(addr: SocketAddr) -> io::Result<UdpSocket> {
    use socket2::{Domain, Protocol, Socket, Type};
    let socket = Socket::new(Domain::for_address(addr), Type::DGRAM, Some(Protocol::UDP))?;
    // Das Betriebssystem darf kleinere Puffer vergeben; das ist kein Fehler.
    let _ = socket.set_recv_buffer_size(UDP_BUFFER);
    let _ = socket.set_send_buffer_size(UDP_BUFFER);
    socket.set_nonblocking(true)?;
    socket.bind(&addr.into())?;
    UdpSocket::from_std(socket.into())
}

fn new_tcp_socket() -> tcp::Socket<'static> {
    let mut socket = tcp::Socket::new(
        tcp::SocketBuffer::new(vec![0u8; TCP_RX_BUFFER]),
        tcp::SocketBuffer::new(vec![0u8; TCP_TX_BUFFER]),
    );
    // Tastendrücke sollen sofort gesendet werden.
    socket.set_nagle_enabled(false);
    // Ohne Staukontrolle würde der Stack bei Paketverlust das ganze Fenster
    // auf einmal nachsenden und sich damit selbst ausbremsen.
    socket.set_congestion_control(tcp::CongestionControl::Cubic);
    // Sofort bestätigen: Die Gegenseite erkennt Lücken dann an wiederholten
    // Bestätigungen und sendet nach, ohne auf ihren Zeitgeber zu warten.
    socket.set_ack_delay(None);
    socket
}

fn add_listener(inner: &mut Inner, port: u16) -> Result<SocketHandle, TransportError> {
    let mut socket = new_tcp_socket();
    socket
        .listen(port)
        .map_err(|e| TransportError::Connect { target: format!("Port {port}"), reason: e.to_string() })?;
    Ok(inner.sockets.add(socket))
}

/// Wählt die Gegenstelle mit dem genauesten passenden Eintrag in AllowedIPs.
fn route(peers: &[Peer], dst: IpAddr) -> Option<usize> {
    peers
        .iter()
        .enumerate()
        .filter_map(|(index, peer)| {
            peer.allowed
                .iter()
                .filter(|net| net.contains(dst))
                .map(|net| net.prefix)
                .max()
                .map(|prefix| (prefix, index))
        })
        .max_by_key(|(prefix, _)| *prefix)
        .map(|(_, index)| index)
}

impl Inner {
    fn next_local_port(&mut self) -> u16 {
        let port = self.next_port;
        self.next_port = if port == u16::MAX { FIRST_EPHEMERAL_PORT } else { port + 1 };
        port
    }

    /// Lässt den TCP/IP-Stack arbeiten und verpackt alle ausgehenden Pakete.
    /// Gibt zurück, wann der Stack spätestens wieder an die Reihe kommen will.
    fn poll(&mut self, out: &mut Vec<Datagram>) -> Option<Duration> {
        let now = smol_now();
        self.iface.poll(now, &mut self.device, &mut self.sockets);

        while let Some(packet) = self.device.tx.pop_front() {
            let Some(dst) = Tunn::dst_address(&packet) else { continue };
            let Some(index) = route(&self.peers, dst) else { continue };
            let peer = &mut self.peers[index];
            let needed = (packet.len() + 32).max(148);
            if self.scratch.len() < needed {
                self.scratch.resize(needed, 0);
            }
            match peer.tunn.encapsulate(&packet, &mut self.scratch) {
                TunnResult::WriteToNetwork(datagram) => out.push((datagram.to_vec(), peer.endpoint)),
                TunnResult::Err(e) => tracing::debug!(error = ?e, "Paket konnte nicht verpackt werden"),
                _ => {}
            }
        }

        for handle in self.graveyard.drain(..) {
            self.sockets.remove(handle);
        }
        let mut index = 0;
        while index < self.lingering.len() {
            let (handle, deadline) = self.lingering[index];
            let socket = self.sockets.get_mut::<tcp::Socket>(handle);
            if socket.state() == tcp::State::Closed {
                self.sockets.remove(handle);
                self.lingering.swap_remove(index);
            } else if Instant::now() >= deadline {
                socket.abort();
                self.graveyard.push(handle);
                self.lingering.swap_remove(index);
            } else {
                index += 1;
            }
        }

        self.iface.poll_delay(now, &self.sockets).map(|d| Duration::from_micros(d.total_micros()))
    }

    /// Verarbeitet ein UDP-Paket der Gegenstelle. Gibt zurück, ob es echt war.
    fn ingest(&mut self, datagram: &[u8], from: SocketAddr, out: &mut Vec<Datagram>) -> bool {
        // Zuerst die Gegenstelle mit passender Absenderadresse versuchen; die
        // übrigen nur, falls sie ihre Adresse gewechselt hat.
        let mut order: Vec<usize> = (0..self.peers.len()).collect();
        order.sort_by_key(|&index| self.peers[index].endpoint != from);

        for index in order {
            let peer = &mut self.peers[index];
            let handshake_before = peer.tunn.time_since_last_handshake();
            let authentic = match peer.tunn.decapsulate(Some(from.ip()), datagram, &mut self.scratch) {
                TunnResult::Err(_) => continue,
                // Ohne Nutzlast echt ist nur ein Datenpaket (Lebenszeichen);
                // eine Cookie-Antwort ließe sich von Dritten nachbilden.
                TunnResult::Done => datagram.first() == Some(&PACKET_TYPE_DATA),
                TunnResult::WriteToNetwork(reply) => {
                    out.push((reply.to_vec(), from));
                    // Pakete nachsenden, die auf den Handshake gewartet haben.
                    while let TunnResult::WriteToNetwork(queued) = peer.tunn.decapsulate(None, &[], &mut self.scratch) {
                        out.push((queued.to_vec(), from));
                    }
                    // Eine Antwort allein beweist nichts: Unter Last
                    // beantwortet WireGuard auch unauthentifizierte Pakete
                    // (Cookie). Echt war das Paket nur, wenn es einen
                    // Handshake abgeschlossen hat.
                    match (handshake_before, peer.tunn.time_since_last_handshake()) {
                        (None, Some(_)) => true,
                        (Some(before), Some(after)) => after < before,
                        _ => false,
                    }
                }
                TunnResult::WriteToTunnelV4(packet, source) => {
                    if peer.allowed.iter().any(|net| net.contains(IpAddr::V4(source))) {
                        self.device.rx.push_back(packet.to_vec());
                    }
                    true
                }
                TunnResult::WriteToTunnelV6(packet, source) => {
                    if peer.allowed.iter().any(|net| net.contains(IpAddr::V6(source))) {
                        self.device.rx.push_back(packet.to_vec());
                    }
                    true
                }
            };
            if authentic {
                // Die Gegenstelle darf ihre Adresse wechseln (Roaming) –
                // aber nur ein authentifiziertes Paket ändert das Ziel.
                peer.endpoint = from;
                peer.retry_at = None;
            }
            return authentic;
        }
        false
    }

    /// Lässt die WireGuard-Zeitgeber laufen (Keepalive, Schlüsselerneuerung).
    fn tick(&mut self, out: &mut Vec<Datagram>) {
        let want_handshake = std::mem::take(&mut self.want_handshake);
        let now = Instant::now();
        for peer in &mut self.peers {
            match peer.tunn.update_timers(&mut self.scratch) {
                TunnResult::WriteToNetwork(datagram) => out.push((datagram.to_vec(), peer.endpoint)),
                // WireGuard gibt nach längerem Ausfall der Gegenstelle auf.
                // Damit der Tunnel sich von selbst erholt, wird in Abständen
                // ein neuer Handshake versucht.
                TunnResult::Err(WireGuardError::ConnectionExpired) => {
                    peer.retry_at.get_or_insert(now + HANDSHAKE_RETRY);
                }
                _ => {}
            }
            let retry_due = peer.retry_at.is_some_and(|at| now >= at);
            if retry_due {
                peer.retry_at = None;
            }
            if (want_handshake || retry_due)
                && !peer.handshake_is_fresh()
                && let TunnResult::WriteToNetwork(datagram) =
                    peer.tunn.format_handshake_initiation(&mut self.scratch, false)
            {
                out.push((datagram.to_vec(), peer.endpoint));
            }
        }
    }
}

/// Tunnelschleife: vermittelt zwischen UDP-Socket, WireGuard und TCP/IP-Stack.
/// Sorgt dafür, dass kein Strom ewig wartet, falls die Tunnelschleife
/// unerwartet endet: markiert den Tunnel als beendet und weckt alle Wartenden.
struct AbortOnExit(Arc<Shared>);

impl Drop for AbortOnExit {
    fn drop(&mut self) {
        let mut inner = self.0.lock();
        if !inner.closed {
            inner.closed = true;
            for (_, socket) in inner.sockets.iter_mut() {
                if let Socket::Tcp(tcp) = socket {
                    tcp.abort();
                }
            }
        }
    }
}

async fn run(shared: Arc<Shared>, udp: UdpSocket, simulate_loss: Option<u32>) {
    let _abort = AbortOnExit(shared.clone());
    let mut closing_since: Option<Instant> = None;
    let mut sent: u32 = 0;
    let mut buffer = vec![0u8; 65536];
    let mut out: Vec<Datagram> = Vec::new();
    let mut last_tick = Instant::now() - TIMER_INTERVAL;

    loop {
        let (delay, finished) = {
            let mut inner = shared.lock();
            if inner.want_handshake || last_tick.elapsed() >= TIMER_INTERVAL {
                inner.tick(&mut out);
                last_tick = Instant::now();
            }
            let delay = inner.poll(&mut out);
            // Nach dem letzten Nutzer läuft die Schleife noch so lange, bis
            // ausstehende Daten und Verbindungsabschlüsse gesendet sind.
            let finished = inner.closed && {
                let since = *closing_since.get_or_insert_with(Instant::now);
                (inner.graveyard.is_empty() && inner.lingering.is_empty()) || since.elapsed() >= LINGER
            };
            (delay, finished)
        };
        for (datagram, addr) in out.drain(..) {
            sent = sent.wrapping_add(1);
            if simulate_loss.is_some_and(|every| every > 0 && sent.is_multiple_of(every)) {
                continue;
            }
            if let Err(e) = udp.send_to(&datagram, addr).await {
                tracing::debug!(error = %e, %addr, "UDP-Paket konnte nicht gesendet werden");
            }
        }
        if finished {
            break;
        }

        let sleep = delay.unwrap_or(MAX_IDLE_SLEEP).min(MAX_IDLE_SLEEP);
        tokio::select! {
            received = udp.recv_from(&mut buffer) => {
                let mut next = received;
                let mut authentic = false;
                let mut inner = shared.lock();
                for _ in 0..RECV_BATCH {
                    match next {
                        Ok((len, from)) => authentic |= inner.ingest(&buffer[..len], from, &mut out),
                        // Unter Windows meldet der Socket hier ICMP-Fehler
                        // („Port nicht erreichbar"); das ist kein Grund aufzuhören.
                        Err(_) => break,
                    }
                    next = match udp.try_recv_from(&mut buffer) {
                        Ok(packet) => Ok(packet),
                        Err(_) => break,
                    };
                }
                drop(inner);
                if authentic {
                    shared.alive.send_modify(|count| *count = count.wrapping_add(1));
                }
            }
            _ = shared.notify.notified() => {}
            _ = tokio::time::sleep(sleep) => {}
        }
    }
}

// ------------------------------------------------------------------ Ströme

/// TCP-Verbindung durch den Tunnel.
pub struct TunnelStream {
    shared: Arc<Shared>,
    _guard: Arc<Guard>,
    handle: SocketHandle,
    shutdown: bool,
}

fn tunnel_closed() -> io::Error {
    io::Error::new(io::ErrorKind::BrokenPipe, "Tunnel wurde beendet")
}

impl AsyncRead for TunnelStream {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        let mut inner = self.shared.lock();
        if inner.closed {
            return Poll::Ready(Err(tunnel_closed()));
        }
        let socket = inner.sockets.get_mut::<tcp::Socket>(self.handle);
        if socket.can_recv() {
            let read = socket.recv_slice(buf.initialize_unfilled()).map_err(io::Error::other)?;
            buf.advance(read);
            drop(inner);
            // Der Stack soll der Gegenseite das frei gewordene Fenster melden.
            self.shared.notify.notify_one();
            return Poll::Ready(Ok(()));
        }
        if !socket.may_recv() {
            if socket.state() == tcp::State::Closed && !self.shutdown {
                return Poll::Ready(Err(io::ErrorKind::ConnectionReset.into()));
            }
            return Poll::Ready(Ok(()));
        }
        socket.register_recv_waker(cx.waker());
        Poll::Pending
    }
}

impl AsyncWrite for TunnelStream {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, data: &[u8]) -> Poll<io::Result<usize>> {
        let mut inner = self.shared.lock();
        if inner.closed {
            return Poll::Ready(Err(tunnel_closed()));
        }
        let socket = inner.sockets.get_mut::<tcp::Socket>(self.handle);
        if socket.can_send() {
            let written = socket.send_slice(data).map_err(io::Error::other)?;
            drop(inner);
            self.shared.notify.notify_one();
            return Poll::Ready(Ok(written));
        }
        if !socket.may_send() {
            return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()));
        }
        socket.register_send_waker(cx.waker());
        Poll::Pending
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.shutdown = true;
        self.shared.lock().sockets.get_mut::<tcp::Socket>(self.handle).close();
        self.shared.notify.notify_one();
        Poll::Ready(Ok(()))
    }
}

impl Drop for TunnelStream {
    fn drop(&mut self) {
        let mut inner = self.shared.lock();
        let socket = inner.sockets.get_mut::<tcp::Socket>(self.handle);
        if socket.state() == tcp::State::Closed {
            inner.sockets.remove(self.handle);
        } else if self.shutdown {
            inner.lingering.push((self.handle, Instant::now() + LINGER));
        } else {
            socket.abort();
            inner.graveyard.push(self.handle);
        }
        drop(inner);
        self.shared.notify.notify_one();
    }
}

/// Nimmt eingehende TCP-Verbindungen auf einem Port im Tunnel an.
pub struct TunnelListener {
    shared: Arc<Shared>,
    guard: Arc<Guard>,
    port: u16,
    current: SocketHandle,
}

impl TunnelListener {
    pub async fn accept(&mut self) -> Result<TunnelStream, TransportError> {
        let shared = self.shared.clone();
        let port = self.port;
        let current = &mut self.current;
        let handle = poll_fn(|cx| {
            let mut inner = shared.lock();
            if inner.closed {
                return Poll::Ready(Err(TransportError::TunnelClosed));
            }
            let socket = inner.sockets.get_mut::<tcp::Socket>(*current);
            match socket.state() {
                tcp::State::Established | tcp::State::CloseWait => {
                    let accepted = *current;
                    *current = add_listener(&mut inner, port)?;
                    Poll::Ready(Ok(accepted))
                }
                tcp::State::Closed => {
                    // Aufbau wurde abgebrochen; erneut lauschen.
                    socket.listen(port).map_err(|e| TransportError::Connect {
                        target: format!("Port {port}"),
                        reason: e.to_string(),
                    })?;
                    socket.register_recv_waker(cx.waker());
                    Poll::Pending
                }
                _ => {
                    socket.register_recv_waker(cx.waker());
                    socket.register_send_waker(cx.waker());
                    Poll::Pending
                }
            }
        })
        .await?;
        Ok(TunnelStream { shared: self.shared.clone(), _guard: self.guard.clone(), handle, shutdown: false })
    }
}

impl Drop for TunnelListener {
    fn drop(&mut self) {
        let mut inner = self.shared.lock();
        inner.sockets.get_mut::<tcp::Socket>(self.current).abort();
        inner.graveyard.push(self.current);
    }
}
