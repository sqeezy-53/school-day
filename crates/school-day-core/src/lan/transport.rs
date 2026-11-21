use super::{Error, Result};
use crate::lessons::RecordId;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use ring::{rand::SecureRandom, signature::Ed25519KeyPair};
use rustls::client::{
    AlwaysResolvesClientRawPublicKeys, Resumption,
    danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
};
use rustls::crypto::{WebPkiSupportedAlgorithms, verify_tls13_signature_with_raw_key};
use rustls::pki_types::{
    CertificateDer, PrivatePkcs8KeyDer, ServerName, SubjectPublicKeyInfoDer, UnixTime,
};
use rustls::server::{
    AlwaysResolvesServerRawPublicKeys, NoServerSessionStorage,
    danger::{ClientCertVerified, ClientCertVerifier},
};
use rustls::sign::CertifiedKey;
use rustls::{
    ClientConfig, ClientConnection, DigitallySignedStruct, DistinguishedName, ServerConfig,
    ServerConnection, SignatureScheme, StreamOwned,
};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, SocketAddrV4, TcpListener, TcpStream};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};
use subtle::ConstantTimeEq;

const ALPN: &[u8] = b"school-day-lan/3";
const CONTROL: usize = 1024;
const HANDSHAKE: Duration = Duration::from_secs(6);
const SESSION: Duration = Duration::from_secs(180);
const INVITE_TTL: Duration = Duration::from_secs(120);
const MAX_TRIES: usize = 6;
const SPKI_PREFIX: &[u8] = &[
    0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00,
];

#[derive(Clone, Default)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
    pub fn stop(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }

    pub(crate) fn check(&self) -> Result<()> {
        if self.is_cancelled() {
            Err(Error::Cancelled)
        } else {
            Ok(())
        }
    }
}

pub struct Identity {
    id: RecordId,
    key: Arc<CertifiedKey>,
}

impl Identity {
    /// The caller must persist the PKCS #8 key using OS-protected local storage.
    pub fn generate() -> Result<(Self, PrivatePkcs8KeyDer<'static>)> {
        let key = Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new())
            .map_err(|_| Error::Crypto)?;
        let private_key = PrivatePkcs8KeyDer::from(key.as_ref().to_vec());
        let identity = Self::load(RecordId::new_v4(), private_key.clone_key())?;
        Ok((identity, private_key))
    }

    pub fn load(id: RecordId, private_key: PrivatePkcs8KeyDer<'static>) -> Result<Self> {
        if id.is_nil() {
            return Err(Error::InvalidMessage);
        }
        let provider = rustls::crypto::ring::default_provider();
        let key = provider
            .key_provider
            .load_private_key(private_key.into())
            .map_err(|_| Error::Crypto)?;
        let spki = key.public_key().ok_or(Error::Crypto)?.as_ref().to_vec();
        validate_spki(&spki)?;
        Ok(Self {
            id,
            key: Arc::new(CertifiedKey::new(vec![CertificateDer::from(spki)], key)),
        })
    }

    pub fn id(&self) -> RecordId {
        self.id
    }
    fn public(&self) -> &[u8] {
        self.key.cert[0].as_ref()
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Peer {
    id: RecordId,
    endpoint: SocketAddrV4,
    key: Vec<u8>,
}

impl Peer {
    pub fn id(&self) -> RecordId {
        self.id
    }
    pub fn endpoint(&self) -> SocketAddrV4 {
        self.endpoint
    }

    pub(crate) fn set_address(&mut self, address: std::net::Ipv4Addr) -> Result<()> {
        let endpoint = SocketAddrV4::new(address, self.endpoint.port());
        validate_endpoint(endpoint)?;
        self.endpoint = endpoint;
        Ok(())
    }

    pub(crate) fn validate(&self, own: &Identity) -> Result<()> {
        validate_endpoint(self.endpoint)?;
        validate_spki(&self.key)?;
        if self.id.is_nil() || self.id == own.id || self.key == own.public() {
            return Err(Error::UntrustedDevice);
        }
        Ok(())
    }
}

pub struct Invitation {
    host: Peer,
    token: [u8; 32],
}

impl Invitation {
    /// A bearer secret for QR/clipboard access only. Never include it in logs.
    pub fn encode(&self) -> String {
        format!(
            "SD1/{}/{}/{}/{}",
            self.host.endpoint,
            self.host.id,
            URL_SAFE_NO_PAD.encode(&self.host.key),
            URL_SAFE_NO_PAD.encode(self.token)
        )
    }

    pub fn decode(code: &str) -> Result<Self> {
        let code = code.trim();
        if code.len() > 240 {
            return Err(Error::InvalidMessage);
        }
        let mut fields = code.split('/');
        if fields.next() != Some("SD1") {
            return Err(Error::InvalidMessage);
        }
        let mut next = || fields.next().ok_or(Error::InvalidMessage);
        let endpoint = next()?.parse().map_err(|_| Error::InvalidAddress)?;
        let id: RecordId = next()?.parse().map_err(|_| Error::InvalidMessage)?;
        let key = URL_SAFE_NO_PAD
            .decode(next()?)
            .map_err(|_| Error::InvalidMessage)?;
        let token = URL_SAFE_NO_PAD
            .decode(next()?)
            .map_err(|_| Error::InvalidMessage)?;
        let token: [u8; 32] = token.try_into().map_err(|_| Error::InvalidMessage)?;
        if fields.next().is_some() || id.is_nil() {
            return Err(Error::InvalidMessage);
        }
        validate_endpoint(endpoint)?;
        validate_spki(&key)?;
        Ok(Self {
            host: Peer { endpoint, id, key },
            token,
        })
    }
}

fn validate_spki(spki: &[u8]) -> Result<()> {
    if spki.len() != 44 || !spki.starts_with(SPKI_PREFIX) {
        Err(Error::UntrustedDevice)
    } else {
        Ok(())
    }
}

fn validate_endpoint(endpoint: SocketAddrV4) -> Result<()> {
    let ip = endpoint.ip();
    if !(ip.is_private() || ip.is_link_local() || ip.is_loopback()) || endpoint.port() == 0 {
        Err(Error::InvalidAddress)
    } else {
        Ok(())
    }
}

struct RawKeyVerifier {
    pin: Option<Vec<u8>>,
    signatures: WebPkiSupportedAlgorithms,
}

impl fmt::Debug for RawKeyVerifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RawKeyVerifier")
    }
}

impl RawKeyVerifier {
    fn check(
        &self,
        spki: &CertificateDer<'_>,
        chain: &[CertificateDer<'_>],
    ) -> std::result::Result<(), rustls::Error> {
        if !chain.is_empty()
            || validate_spki(spki.as_ref()).is_err()
            || self
                .pin
                .as_ref()
                .is_some_and(|pinned| pinned.as_slice() != spki.as_ref())
        {
            return Err(rustls::Error::InvalidCertificate(
                rustls::CertificateError::ApplicationVerificationFailure,
            ));
        }
        Ok(())
    }
}

macro_rules! raw_signatures {
    () => {
        fn requires_raw_public_keys(&self) -> bool {
            true
        }
        fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
            vec![SignatureScheme::ED25519]
        }
        fn verify_tls12_signature(
            &self,
            _: &[u8],
            _: &CertificateDer<'_>,
            _: &DigitallySignedStruct,
        ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
            Err(rustls::Error::General("Expected TLS 1.3".into()))
        }
        fn verify_tls13_signature(
            &self,
            message: &[u8],
            spki: &CertificateDer<'_>,
            sig: &DigitallySignedStruct,
        ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
            verify_tls13_signature_with_raw_key(
                message,
                &SubjectPublicKeyInfoDer::from(spki.as_ref()),
                sig,
                &self.signatures,
            )
        }
    };
}

impl ServerCertVerifier for RawKeyVerifier {
    fn verify_server_cert(
        &self,
        cert: &CertificateDer<'_>,
        chain: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        _: UnixTime,
    ) -> std::result::Result<ServerCertVerified, rustls::Error> {
        if self.pin.is_none() {
            return Err(rustls::Error::InvalidCertificate(
                rustls::CertificateError::UnknownIssuer,
            ));
        }
        self.check(cert, chain)?;
        Ok(ServerCertVerified::assertion())
    }
    raw_signatures!();
}

impl ClientCertVerifier for RawKeyVerifier {
    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        &[]
    }
    fn verify_client_cert(
        &self,
        cert: &CertificateDer<'_>,
        chain: &[CertificateDer<'_>],
        _: UnixTime,
    ) -> std::result::Result<ClientCertVerified, rustls::Error> {
        self.check(cert, chain)?;
        Ok(ClientCertVerified::assertion())
    }
    raw_signatures!();
}

struct Socket {
    tcp: TcpStream,
    deadline: Instant,
    cancel: Cancel,
}

impl Socket {
    fn new(tcp: TcpStream, deadline: Instant, cancel: Cancel) -> io::Result<Self> {
        tcp.set_nodelay(true)?;
        Ok(Self {
            tcp,
            deadline,
            cancel,
        })
    }

    fn timeout(&self) -> io::Result<Duration> {
        if self.cancel.is_cancelled() {
            // Interrupted means "retry" to Read::read_exact, so use ConnectionAborted.
            return Err(io::ErrorKind::ConnectionAborted.into());
        }
        let left = self.deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(io::ErrorKind::TimedOut.into());
        }
        Ok(left.min(Duration::from_millis(400)))
    }
}

impl Read for Socket {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        loop {
            self.tcp.set_read_timeout(Some(self.timeout()?))?;
            match self.tcp.read(buffer) {
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) => {}
                result => return result,
            }
        }
    }
}

impl Write for Socket {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        loop {
            self.tcp.set_write_timeout(Some(self.timeout()?))?;
            match self.tcp.write(buffer) {
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) => {}
                result => return result,
            }
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        self.tcp.flush()
    }
}

enum Tls {
    Server(Box<StreamOwned<ServerConnection, Socket>>),
    Client(Box<StreamOwned<ClientConnection, Socket>>),
}

impl Tls {
    fn server(
        socket: TcpStream,
        identity: &Identity,
        pinned: Option<&Peer>,
        cancel: Cancel,
        limit: Instant,
    ) -> Result<Self> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let verifier = Arc::new(RawKeyVerifier {
            pin: pinned.map(|peer| peer.key.clone()),
            signatures: provider.signature_verification_algorithms,
        });
        let mut config = ServerConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&rustls::version::TLS13])
            .map_err(|_| Error::Crypto)?
            .with_client_cert_verifier(verifier)
            .with_cert_resolver(Arc::new(AlwaysResolvesServerRawPublicKeys::new(
                identity.key.clone(),
            )));
        config.alpn_protocols = vec![ALPN.to_vec()];
        config.session_storage = Arc::new(NoServerSessionStorage {});
        config.send_tls13_tickets = 0;
        let mut conn = ServerConnection::new(Arc::new(config)).map_err(|_| Error::Crypto)?;
        let mut io = Socket::new(socket, limit.min(Instant::now() + HANDSHAKE), cancel)?;
        while conn.is_handshaking() {
            conn.complete_io(&mut io)
                .map_err(|_| Error::UntrustedDevice)?;
        }
        if conn.alpn_protocol() != Some(ALPN) {
            return Err(Error::InvalidMessage);
        }
        io.deadline = limit;
        Ok(Tls::Server(Box::new(StreamOwned::new(conn, io))))
    }

    fn client(identity: &Identity, peer: &Peer, cancel: Cancel, limit: Instant) -> Result<Self> {
        cancel.check()?;
        peer.validate(identity)?;
        let tcp = TcpStream::connect_timeout(&peer.endpoint.into(), HANDSHAKE)?;
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let verifier = Arc::new(RawKeyVerifier {
            pin: Some(peer.key.clone()),
            signatures: provider.signature_verification_algorithms,
        });
        let mut config = ClientConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&rustls::version::TLS13])
            .map_err(|_| Error::Crypto)?
            .dangerous()
            .with_custom_certificate_verifier(verifier)
            .with_client_cert_resolver(Arc::new(AlwaysResolvesClientRawPublicKeys::new(
                identity.key.clone(),
            )));
        config.alpn_protocols = vec![ALPN.to_vec()];
        config.resumption = Resumption::disabled();
        let name = ServerName::try_from("school-day.local").map_err(|_| Error::Crypto)?;
        let mut conn = ClientConnection::new(Arc::new(config), name).map_err(|_| Error::Crypto)?;
        let mut io = Socket::new(tcp, limit.min(Instant::now() + HANDSHAKE), cancel)?;
        while conn.is_handshaking() {
            conn.complete_io(&mut io)
                .map_err(|_| Error::UntrustedDevice)?;
        }
        if conn.alpn_protocol() != Some(ALPN) {
            return Err(Error::InvalidMessage);
        }
        io.deadline = limit;
        Ok(Tls::Client(Box::new(StreamOwned::new(conn, io))))
    }

    fn channel(&mut self) -> &mut dyn Stream {
        match self {
            Self::Server(io) => io,
            Self::Client(io) => io,
        }
    }

    fn peer_key(&self) -> Result<Vec<u8>> {
        let chain = match self {
            Self::Server(io) => io.conn.peer_certificates(),
            Self::Client(io) => io.conn.peer_certificates(),
        }
        .ok_or(Error::UntrustedDevice)?;
        match chain {
            [key] => Ok(key.as_ref().to_vec()),
            _ => Err(Error::UntrustedDevice),
        }
    }

    fn code(&self) -> Result<String> {
        let label = b"EXPORTER-School-Day-pair-confirmation";
        let seed = [0u8; 4];
        let bytes = match self {
            Self::Server(io) => io.conn.export_keying_material(seed, label, None),
            Self::Client(io) => io.conn.export_keying_material(seed, label, None),
        }
        .map_err(|_| Error::Crypto)?;
        Ok(format!("{:08}", u32::from_be_bytes(bytes) % 100_000_000))
    }
}

trait Stream: Read + Write {}
impl<T: Read + Write> Stream for T {}

pub(crate) struct Channel(Tls);

impl Channel {
    pub(crate) fn connect(identity: &Identity, peer: &Peer, cancel: Cancel) -> Result<Self> {
        Tls::client(identity, peer, cancel, Instant::now() + SESSION).map(Self)
    }

    pub(crate) fn accept(
        listener: TcpListener,
        identity: &Identity,
        peer: &Peer,
        cancel: Cancel,
        poll_interval: Duration,
    ) -> Result<Self> {
        peer.validate(identity)?;
        let until = Instant::now() + SESSION;
        listener.set_nonblocking(true)?;
        for _ in 0..MAX_TRIES {
            let (tcp, _) = accept_socket(&listener, until, &cancel, poll_interval)?;
            if let Ok(stream) = Tls::server(tcp, identity, Some(peer), cancel.clone(), until) {
                return Ok(Self(stream));
            }
        }
        Err(Error::UntrustedDevice)
    }

    pub(crate) fn send(&mut self, kind: u8, body: &[u8], limit: usize) -> Result<()> {
        let len = u32::try_from(body.len()).map_err(|_| Error::InvalidMessage)?;
        if body.len() > limit {
            return Err(Error::InvalidMessage);
        }
        let io = self.0.channel();
        io.write_all(&[kind])?;
        io.write_all(&len.to_be_bytes())?;
        io.write_all(body)?;
        io.flush()?;
        Ok(())
    }

    pub(crate) fn receive(&mut self, kind: u8, limit: usize) -> Result<Vec<u8>> {
        let io = self.0.channel();
        let mut header = [0; 5];
        io.read_exact(&mut header)?;
        let len = u32::from_be_bytes([header[1], header[2], header[3], header[4]]) as usize;
        if header[0] != kind || len > limit {
            return Err(Error::InvalidMessage);
        }
        let mut data = vec![0; len];
        io.read_exact(&mut data)?;
        Ok(data)
    }

    pub(crate) fn send_control(&mut self, kind: u8, value: &impl Serialize) -> Result<()> {
        let bytes = serde_json::to_vec(value).map_err(|_| Error::InvalidMessage)?;
        self.send(kind, &bytes, CONTROL)
    }

    pub(crate) fn receive_control<T: serde::de::DeserializeOwned>(
        &mut self,
        kind: u8,
    ) -> Result<T> {
        serde_json::from_slice(&self.receive(kind, CONTROL)?).map_err(|_| Error::InvalidMessage)
    }
}

impl Drop for Channel {
    fn drop(&mut self) {
        // Let the authenticated application acknowledgements, not a close alert,
        // determine success. Do not extend a cancelled or expired connection.
        match &mut self.0 {
            Tls::Server(io) => {
                io.conn.send_close_notify();
                io.sock.deadline = io
                    .sock
                    .deadline
                    .min(Instant::now() + Duration::from_millis(200));
                let _ = io.flush();
            }
            Tls::Client(io) => {
                io.conn.send_close_notify();
                io.sock.deadline = io
                    .sock
                    .deadline
                    .min(Instant::now() + Duration::from_millis(200));
                let _ = io.flush();
            }
        }
    }
}

fn accept_socket(
    listener: &TcpListener,
    until: Instant,
    cancel: &Cancel,
    poll_interval: Duration,
) -> Result<(TcpStream, SocketAddr)> {
    loop {
        cancel.check()?;
        if Instant::now() >= until {
            return Err(Error::Deadline);
        }
        match listener.accept() {
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => std::thread::sleep(poll_interval),
            result => return result.map_err(Into::into),
        }
    }
}

pub struct Pairing {
    listener: TcpListener,
    invitation: Invitation,
    until: Instant,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PairRequest {
    id: RecordId,
    token: [u8; 32],
}

impl Pairing {
    pub fn open(identity: &Identity, address: SocketAddrV4) -> Result<Self> {
        validate_endpoint(SocketAddrV4::new(*address.ip(), 1))?;
        let listener = TcpListener::bind(address)?;
        listener.set_nonblocking(true)?;
        let SocketAddr::V4(endpoint) = listener.local_addr()? else {
            return Err(Error::InvalidAddress);
        };
        let mut token = [0; 32];
        ring::rand::SystemRandom::new()
            .fill(&mut token)
            .map_err(|_| Error::Crypto)?;
        let host = Peer {
            id: identity.id,
            endpoint,
            key: identity.public().to_vec(),
        };
        Ok(Self {
            listener,
            invitation: Invitation { host, token },
            until: Instant::now() + INVITE_TTL,
        })
    }

    pub fn invitation(&self) -> &Invitation {
        &self.invitation
    }

    /// Consumes the listener and invite on approval or rejection, so the QR cannot be reused.
    pub(crate) fn accept(
        self,
        identity: &Identity,
        cancel: Cancel,
        approve: impl FnOnce(&str) -> bool,
        persist: impl FnOnce(&Peer) -> Result<()>,
    ) -> Result<()> {
        if self.invitation.host.key != identity.public() || self.invitation.host.id != identity.id {
            return Err(Error::UntrustedDevice);
        }
        for _ in 0..MAX_TRIES {
            let (socket, _) = accept_socket(
                &self.listener,
                self.until,
                &cancel,
                Duration::from_millis(35),
            )
            .map_err(|error| match error {
                Error::Deadline => Error::InvitationExpired,
                other => other,
            })?;
            let request_by = self.until.min(Instant::now() + HANDSHAKE);
            let Ok(tls) = Tls::server(socket, identity, None, cancel.clone(), request_by) else {
                continue;
            };
            let mut link = Channel(tls);
            let Ok(request) = link.receive_control::<PairRequest>(1) else {
                continue;
            };
            if !bool::from(self.invitation.token.ct_eq(&request.token)) {
                continue;
            }

            let peer = Peer {
                id: request.id,
                endpoint: self.invitation.host.endpoint,
                key: link.0.peer_key()?,
            };
            peer.validate(identity)?;
            if let Tls::Server(io) = &mut link.0 {
                io.sock.deadline = self.until;
            }
            let code = link.0.code()?;
            if !approve(&code) {
                return Err(Error::Rejected);
            }
            cancel.check()?;
            if Instant::now() >= self.until {
                return Err(Error::InvitationExpired);
            }
            link.send_control(2, &identity.id)?;
            let agreed: RecordId = link.receive_control(3)?;
            if agreed != peer.id {
                return Err(Error::UntrustedDevice);
            }
            persist(&peer)?;
            link.send_control(4, &identity.id)?;
            return Ok(());
        }
        Err(Error::InvitationExpired)
    }
}

pub(crate) fn pair_client(
    identity: &Identity,
    invitation: Invitation,
    cancel: Cancel,
    approve: impl FnOnce(&str) -> bool,
    persist: impl FnOnce(&Peer) -> Result<()>,
) -> Result<()> {
    let peer = invitation.host;
    let mut link = Channel::connect(identity, &peer, cancel.clone())?;
    link.send_control(
        1,
        &PairRequest {
            id: identity.id,
            token: invitation.token,
        },
    )?;
    let code = link.0.code()?;
    if !approve(&code) {
        return Err(Error::Rejected);
    }
    cancel.check()?;
    let server_id: RecordId = link.receive_control(2)?;
    if server_id != peer.id {
        return Err(Error::UntrustedDevice);
    }
    persist(&peer)?;
    link.send_control(3, &identity.id)?;
    let stored: RecordId = link.receive_control(4)?;
    if stored != peer.id {
        return Err(Error::UntrustedDevice);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expired_qr_offer_skips_confirmation_and_reports_expiration() {
        let (identity, _) = Identity::generate().unwrap();
        let mut offer = Pairing::open(&identity, "127.0.0.1:0".parse().unwrap()).unwrap();
        offer.until = Instant::now();
        let result = offer.accept(
            &identity,
            Cancel::default(),
            |_| panic!("Expired offers must not display a confirmation"),
            |_| panic!("Expired offers must not store trust"),
        );
        assert!(matches!(result, Err(Error::InvitationExpired)));
    }

    #[test]
    fn malformed_and_consumed_qr_offers_cannot_pair_a_second_device() {
        let (desktop, _) = Identity::generate().unwrap();
        let (phone, _) = Identity::generate().unwrap();
        let (other, _) = Identity::generate().unwrap();
        let offer = Pairing::open(&desktop, "127.0.0.1:0".parse().unwrap()).unwrap();
        let code = offer.invitation().encode();
        assert!(Invitation::decode(&format!("{code}/extra")).is_err());
        assert!(Invitation::decode(&code.replace("127.0.0.1", "192.0.2.1")).is_err());
        assert!(Invitation::decode(&"A".repeat(241)).is_err());
        let first = Invitation::decode(&code).unwrap();
        let reused = Invitation::decode(&code).unwrap();
        std::thread::scope(|scope| {
            let accept = scope.spawn(|| {
                offer.accept(
                    &desktop,
                    Cancel::default(),
                    |_| true,
                    |peer| {
                        assert_eq!(peer.id, phone.id());
                        Ok(())
                    },
                )
            });
            pair_client(&phone, first, Cancel::default(), |_| true, |_| Ok(())).unwrap();
            accept.join().unwrap().unwrap();
        });
        assert!(
            pair_client(
                &other,
                reused,
                Cancel::default(),
                |_| panic!("A consumed offer must not display a confirmation"),
                |_| panic!("A consumed offer must not store trust"),
            )
            .is_err()
        );
    }

    #[test]
    fn a_valid_but_untrusted_client_key_cannot_open_the_exchange() {
        let (desktop, _) = Identity::generate().unwrap();
        let (allowed, _) = Identity::generate().unwrap();
        let (other, _) = Identity::generate().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let SocketAddr::V4(endpoint) = listener.local_addr().unwrap() else {
            unreachable!()
        };
        let desk = Peer {
            id: desktop.id,
            endpoint,
            key: desktop.public().to_vec(),
        };
        let pinned_phone = Peer {
            id: allowed.id,
            endpoint,
            key: allowed.public().to_vec(),
        };
        std::thread::scope(|scope| {
            let accept = scope.spawn(|| {
                let (socket, _) = listener.accept().unwrap();
                Tls::server(
                    socket,
                    &desktop,
                    Some(&pinned_phone),
                    Cancel::default(),
                    Instant::now() + Duration::from_secs(4),
                )
            });
            if let Ok(tls) = Tls::client(
                &other,
                &desk,
                Cancel::default(),
                Instant::now() + Duration::from_secs(4),
            ) {
                let mut channel = Channel(tls);
                assert!(channel.receive(10, 32).is_err());
            }
            assert!(matches!(
                accept.join().unwrap(),
                Err(Error::UntrustedDevice)
            ));
        });
    }

    #[test]
    fn oversized_frame_is_rejected_before_reading_a_payload() {
        let (desktop, _) = Identity::generate().unwrap();
        let (phone, _) = Identity::generate().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let SocketAddr::V4(endpoint) = listener.local_addr().unwrap() else {
            unreachable!()
        };
        let desk = Peer {
            id: desktop.id,
            endpoint,
            key: desktop.public().to_vec(),
        };
        let client = Peer {
            id: phone.id,
            endpoint,
            key: phone.public().to_vec(),
        };
        std::thread::scope(|scope| {
            let server = scope.spawn(|| {
                let (socket, _) = listener.accept().unwrap();
                let mut channel = Channel(
                    Tls::server(
                        socket,
                        &desktop,
                        Some(&client),
                        Cancel::default(),
                        Instant::now() + Duration::from_secs(4),
                    )
                    .unwrap(),
                );
                assert!(matches!(
                    channel.receive(10, 256),
                    Err(Error::InvalidMessage)
                ));
            });
            let mut channel = Channel::connect(&phone, &desk, Cancel::default()).unwrap();
            channel.0.channel().write_all(&[10, 0, 0, 1, 1]).unwrap();
            channel.0.channel().flush().unwrap();
            server.join().unwrap();
        });
    }
}
