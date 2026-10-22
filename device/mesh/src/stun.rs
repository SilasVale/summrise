//! STUN (RFC 5389): asking a reflector what an address looks like from there.
//!
//! The mesh needs its public UDP address before it can publish a candidate or
//! punch towards a peer. A STUN binding request is twenty bytes and no
//! attributes; the reflector answers with `XOR-MAPPED-ADDRESS`, the source
//! address it observed. The standard is worth speaking because the tailnet's
//! exit node already runs a reflector on UDP 3478 (`scripts/DERP.md`) — the
//! address-discovery half needs no new server.
//!
//! **The XOR is not optional, and getting it wrong is a classic mistake.**
//! RFC 5389 §15.2: the port is XORed with the high 16 bits of the magic
//! cookie, an IPv4 address with the whole cookie, and an IPv6 address with
//! the cookie **followed by the transaction id**. A parser that forgets the
//! port mask produces a plausible-looking wrong port, and a wrong port is a
//! punch that lands on nothing — which looks exactly like a firewall.
//!
//! **The address is a hypothesis, not a fact.** It is true only for the path
//! to the reflector; a box's egress address can differ by destination and
//! over time. This module reports what the reflector saw; the punch is what
//! tests it.
//!
//! The module is the codec plus two thin socket drivers: [`query`] is the
//! async form, and [`query_blocking`] is the form a caller uses when it must
//! ask **before** handing the socket to another owner (quinn's endpoint).
//! Both send the request on the caller's socket and leave it with the caller.
//! The reflector must be asked on the **same socket** the punch uses, because
//! that is the NAT mapping whose public port the answer names.

use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;

/// The STUN magic cookie (RFC 5389 §6).
pub const MAGIC_COOKIE: u32 = 0x2112_A442;

/// A transaction id is 96 bits, and it is what ties a response to a request.
pub const TRANSACTION_ID_BYTES: usize = 12;

/// A STUN transaction id.
pub type TransactionId = [u8; TRANSACTION_ID_BYTES];

/// A binding request type.
const BINDING_REQUEST: u16 = 0x0001;

/// A binding success response type.
const BINDING_SUCCESS: u16 = 0x0101;

/// The `XOR-MAPPED-ADDRESS` attribute type.
const XOR_MAPPED_ADDRESS: u16 = 0x0020;

/// The header every STUN message starts with.
const HEADER_BYTES: usize = 20;

/// The success response class (RFC 5389 §6: request=0, indication=1,
/// success=2, error=3). The class bits are not adjacent.
const SUCCESS_CLASS: u8 = 2;

/// Why a response could not be used.
///
/// Every variant is a refusal to guess: a wrong transaction id, a wrong
/// cookie, a bad length, or a response that is not a binding success is an
/// error, never an address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StunError {
    /// The message is shorter than the 20-byte header.
    TooShort {
        /// How many bytes arrived.
        len: usize,
    },
    /// The magic cookie is not the STUN cookie.
    WrongCookie {
        /// The cookie the message carried.
        found: u32,
    },
    /// The message length field does not describe this datagram.
    BadLength {
        /// The length the header declared.
        declared: usize,
        /// The body bytes actually present.
        actual: usize,
    },
    /// The message is a STUN success response, but not to this request.
    WrongTransactionId,
    /// The class is not "success response".
    NotSuccess {
        /// The message type, for the error message.
        message_type: u16,
        /// The decoded class.
        class: u8,
    },
    /// A success response, but not for the binding method.
    WrongMethod {
        /// The message type, for the error message.
        message_type: u16,
    },
    /// An attribute's length runs past the end of the message.
    MalformedAttribute {
        /// Where the attribute header starts in the message.
        offset: usize,
    },
    /// `XOR-MAPPED-ADDRESS` is shorter than its family requires, or claims a
    /// length the family does not use.
    MalformedMappedAddress {
        /// The address family byte.
        family: u8,
        /// The attribute value's length.
        len: usize,
    },
    /// The address family is neither IPv4 nor IPv6.
    UnsupportedFamily {
        /// The family byte the reflector sent.
        family: u8,
    },
    /// No `XOR-MAPPED-ADDRESS` attribute was present.
    NoMappedAddress,
    /// The deadline passed before the reflector answered with our transaction
    /// id. A reflector that only sent another transaction's answer, or none
    /// at all, lands here.
    Timeout,
    /// Sending the request or receiving the answer failed at the socket.
    Io(std::io::ErrorKind),
}

impl std::fmt::Display for StunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooShort { len } => {
                write!(f, "a STUN message is at least {HEADER_BYTES} bytes; this one is {len}")
            }
            Self::WrongCookie { found } => write!(
                f,
                "the STUN magic cookie is {MAGIC_COOKIE:#010x}; this message carries {found:#010x}"
            ),
            Self::BadLength { declared, actual } => write!(
                f,
                "the STUN header declares a {declared}-byte body; {actual} bytes follow it"
            ),
            Self::WrongTransactionId => {
                f.write_str("the STUN response belongs to a different transaction")
            }
            Self::NotSuccess {
                message_type,
                class,
            } => write!(
                f,
                "the STUN message type {message_type:#06x} has class {class}, not a success response"
            ),
            Self::WrongMethod { message_type } => write!(
                f,
                "the STUN message type {message_type:#06x} is not a binding response"
            ),
            Self::MalformedAttribute { offset } => write!(
                f,
                "the STUN attribute at byte {offset} runs past the end of the message"
            ),
            Self::MalformedMappedAddress { family, len } => write!(
                f,
                "the XOR-MAPPED-ADDRESS family {family:#04x} has a {len}-byte value"
            ),
            Self::UnsupportedFamily { family } => {
                write!(f, "the XOR-MAPPED-ADDRESS family {family:#04x} is not IPv4 or IPv6")
            }
            Self::NoMappedAddress => {
                f.write_str("the STUN response has no XOR-MAPPED-ADDRESS attribute")
            }
            Self::Timeout => f.write_str("the STUN query timed out without a matching answer"),
            Self::Io(kind) => write!(f, "the STUN socket failed: {kind:?}"),
        }
    }
}

impl std::error::Error for StunError {}

/// Draw a fresh 96-bit transaction id from the OS CSPRNG.
pub fn transaction_id() -> TransactionId {
    let mut id = [0u8; TRANSACTION_ID_BYTES];
    rand::fill(&mut id);
    id
}

/// Build a binding request with a fresh transaction id.
///
/// The id is returned beside the bytes: a response is only usable if it
/// carries the same id, so the caller must keep it.
pub fn binding_request() -> (TransactionId, Vec<u8>) {
    let id = transaction_id();
    (id, binding_request_with(&id))
}

/// Build a binding request for a caller-supplied transaction id.
pub fn binding_request_with(transaction_id: &TransactionId) -> Vec<u8> {
    let mut request = Vec::with_capacity(HEADER_BYTES);
    request.extend_from_slice(&BINDING_REQUEST.to_be_bytes());
    request.extend_from_slice(&0u16.to_be_bytes()); // no attributes
    request.extend_from_slice(&MAGIC_COOKIE.to_be_bytes());
    request.extend_from_slice(transaction_id);
    request
}

/// Ask `server_addr` what this socket's public address looks like from there.
///
/// The socket is the caller's and must be the one the punch will use: the
/// answer names the NAT mapping that socket's traffic created. The request
/// carries a fresh transaction id; an answer with a different id is not an
/// answer and is ignored, so a stale or injected datagram can neither become
/// "our public address" nor end the wait early. The deadline is the only
/// clock: if no matching answer arrives before it, the result is
/// [`StunError::Timeout`].
///
/// A malformed answer *from the reflector* is a typed error, not a retry: it
/// says the reflector is not speaking this protocol.
pub async fn query(
    socket: &tokio::net::UdpSocket,
    server_addr: SocketAddr,
    timeout: Duration,
) -> Result<SocketAddr, StunError> {
    let (transaction_id, request) = binding_request();
    socket
        .send_to(&request, server_addr)
        .await
        .map_err(|error| StunError::Io(error.kind()))?;

    let deadline = tokio::time::Instant::now() + timeout;
    let mut buffer = [0u8; 1500];
    loop {
        let (len, from) =
            match tokio::time::timeout_at(deadline, socket.recv_from(&mut buffer)).await {
                Err(_) => return Err(StunError::Timeout),
                Ok(Err(error)) => return Err(StunError::Io(error.kind())),
                Ok(Ok(answer)) => answer,
            };
        // Only the reflector's own datagrams count. Anything else on this
        // socket is not an answer to this question.
        if from != server_addr {
            continue;
        }
        match parse_binding_response(&buffer[..len], &transaction_id) {
            Ok(address) => return Ok(address),
            Err(StunError::WrongTransactionId) => continue,
            Err(error) => return Err(error),
        }
    }
}

/// Ask `server_addr` what this socket's public address looks like from there,
/// blocking the calling thread.
///
/// This is [`query`] for the one caller that must ask **before** it hands the
/// socket to someone else: `MeshSession::join` binds the UDP socket, asks the
/// reflector on that same socket, and then gives it to quinn. The NAT mapping
/// the exchange creates belongs to this socket's 5-tuple, so a reflection
/// asked anywhere else would name a port QUIC does not own.
///
/// The deadline is the only clock: the socket's read timeout is set to what is
/// left of it before every receive, and a silent reflector ends in
/// [`StunError::Timeout`]. The socket must be in blocking mode; it is the
/// caller's, and the caller hands it on unchanged.
pub fn query_blocking(
    socket: &std::net::UdpSocket,
    server_addr: SocketAddr,
    timeout: Duration,
) -> Result<SocketAddr, StunError> {
    let (transaction_id, request) = binding_request();
    socket
        .send_to(&request, server_addr)
        .map_err(|error| StunError::Io(error.kind()))?;

    let deadline = std::time::Instant::now() + timeout;
    let mut buffer = [0u8; 1500];
    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return Err(StunError::Timeout);
        }
        socket
            .set_read_timeout(Some(remaining))
            .map_err(|error| StunError::Io(error.kind()))?;
        let (len, from) = match socket.recv_from(&mut buffer) {
            Ok(answer) => answer,
            // A read timeout is reported as `WouldBlock` on Unix and
            // `TimedOut` on Windows; both mean this deadline passed.
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                return Err(StunError::Timeout)
            }
            Err(error) => return Err(StunError::Io(error.kind())),
        };
        // Only the reflector's own datagrams count. Anything else on this
        // socket is not an answer to this question.
        if from != server_addr {
            continue;
        }
        match parse_binding_response(&buffer[..len], &transaction_id) {
            Ok(address) => return Ok(address),
            Err(StunError::WrongTransactionId) => continue,
            Err(error) => return Err(error),
        }
    }
}

/// Parse a binding success response into the address the reflector observed.
///
/// `expected` is the transaction id of the request this response claims to
/// answer. A response with any other id is refused: accepting it would let a
/// stale or injected datagram become "our public address".
pub fn parse_binding_response(
    bytes: &[u8],
    expected: &TransactionId,
) -> Result<SocketAddr, StunError> {
    if bytes.len() < HEADER_BYTES {
        return Err(StunError::TooShort { len: bytes.len() });
    }
    let message_type = u16::from_be_bytes([bytes[0], bytes[1]]);
    let declared = u16::from_be_bytes([bytes[2], bytes[3]]) as usize;
    let cookie = u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    let actual = bytes.len() - HEADER_BYTES;

    if cookie != MAGIC_COOKIE {
        return Err(StunError::WrongCookie { found: cookie });
    }
    // RFC 5389 §6: the length counts everything after the header and is a
    // multiple of four. A datagram is exactly one message, so both must
    // agree; a longer or shorter field is a mangled message, not slack.
    if declared != actual || !declared.is_multiple_of(4) {
        return Err(StunError::BadLength { declared, actual });
    }

    let class = ((message_type & 0x0100) >> 7 | (message_type & 0x0010) >> 4) as u8;
    if class != SUCCESS_CLASS {
        return Err(StunError::NotSuccess {
            message_type,
            class,
        });
    }
    if message_type != BINDING_SUCCESS {
        return Err(StunError::WrongMethod { message_type });
    }
    if bytes[8..HEADER_BYTES] != expected[..] {
        return Err(StunError::WrongTransactionId);
    }

    let end = HEADER_BYTES + declared;
    let mut offset = HEADER_BYTES;
    while offset < end {
        if end - offset < 4 {
            return Err(StunError::MalformedAttribute { offset });
        }
        let attribute_type = u16::from_be_bytes([bytes[offset], bytes[offset + 1]]);
        let attribute_len = u16::from_be_bytes([bytes[offset + 2], bytes[offset + 3]]) as usize;
        let value_start = offset + 4;
        // Padded to a four-byte boundary; the padding is counted in the
        // message length but not in the attribute length.
        let padded_len = (attribute_len + 3) & !3;
        let next = value_start + padded_len;
        if next > end {
            return Err(StunError::MalformedAttribute { offset });
        }
        if attribute_type == XOR_MAPPED_ADDRESS {
            return parse_xor_mapped_address(
                &bytes[value_start..value_start + attribute_len],
                expected,
            );
        }
        offset = next;
    }

    Err(StunError::NoMappedAddress)
}

/// Decode an `XOR-MAPPED-ADDRESS` value (RFC 5389 §15.2).
fn parse_xor_mapped_address(
    value: &[u8],
    transaction_id: &TransactionId,
) -> Result<SocketAddr, StunError> {
    if value.len() < 4 {
        return Err(StunError::MalformedMappedAddress {
            family: value.get(1).copied().unwrap_or(0),
            len: value.len(),
        });
    }
    let family = value[1];
    let port = u16::from_be_bytes([value[2], value[3]]) ^ (MAGIC_COOKIE >> 16) as u16;
    match family {
        0x01 => {
            if value.len() != 8 {
                return Err(StunError::MalformedMappedAddress {
                    family,
                    len: value.len(),
                });
            }
            let masked = u32::from_be_bytes([value[4], value[5], value[6], value[7]]);
            let address = masked ^ MAGIC_COOKIE;
            Ok(SocketAddr::from((Ipv4Addr::from(address), port)))
        }
        0x02 => {
            if value.len() != 20 {
                return Err(StunError::MalformedMappedAddress {
                    family,
                    len: value.len(),
                });
            }
            // The IPv6 mask is the magic cookie followed by the transaction
            // id — sixteen bytes, and not the four-byte IPv4 mask.
            let mut mask = [0u8; 16];
            mask[..4].copy_from_slice(&MAGIC_COOKIE.to_be_bytes());
            mask[4..].copy_from_slice(transaction_id);
            let mut octets = [0u8; 16];
            for (index, byte) in octets.iter_mut().enumerate() {
                *byte = value[4 + index] ^ mask[index];
            }
            Ok(SocketAddr::from((Ipv6Addr::from(octets), port)))
        }
        other => Err(StunError::UnsupportedFamily { family: other }),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::net::UdpSocket;

    use super::*;

    /// Build a binding success response for `mapped`, as a reflector would.
    fn binding_response_for(transaction_id: &TransactionId, mapped: SocketAddr) -> Vec<u8> {
        let (family, address, port) = match mapped {
            SocketAddr::V4(address) => (0x01u8, address.ip().octets().to_vec(), address.port()),
            SocketAddr::V6(address) => (0x02u8, address.ip().octets().to_vec(), address.port()),
        };
        let mut value = vec![0u8, family];
        value.extend_from_slice(&(port ^ (MAGIC_COOKIE >> 16) as u16).to_be_bytes());
        let mut mask = MAGIC_COOKIE.to_be_bytes().to_vec();
        if family == 0x02 {
            // The IPv6 mask is the cookie followed by the transaction id.
            mask.extend_from_slice(transaction_id);
        }
        for (byte, mask) in address.iter().zip(mask.iter()) {
            value.push(byte ^ mask);
        }

        let mut response = Vec::with_capacity(HEADER_BYTES + 4 + value.len());
        response.extend_from_slice(&BINDING_SUCCESS.to_be_bytes());
        response.extend_from_slice(&((4 + value.len()) as u16).to_be_bytes());
        response.extend_from_slice(&MAGIC_COOKIE.to_be_bytes());
        response.extend_from_slice(transaction_id);
        response.extend_from_slice(&XOR_MAPPED_ADDRESS.to_be_bytes());
        response.extend_from_slice(&(value.len() as u16).to_be_bytes());
        response.extend_from_slice(&value);
        response
    }

    #[tokio::test]
    async fn the_query_returns_the_address_the_reflector_reports() {
        let reflector = UdpSocket::bind("127.0.0.1:0").await.expect("a reflector");
        let server = reflector.local_addr().expect("its port");
        let expected: SocketAddr = "192.0.2.9:54321".parse().expect("an address");
        let responder = tokio::spawn(async move {
            let mut buffer = [0u8; 1500];
            let (len, peer) = reflector.recv_from(&mut buffer).await.expect("a request");
            assert_eq!(len, HEADER_BYTES);
            let transaction_id: TransactionId = buffer[8..20].try_into().expect("an id");
            let response = binding_response_for(&transaction_id, expected);
            reflector
                .send_to(&response, peer)
                .await
                .expect("a response");
        });

        let socket = UdpSocket::bind("127.0.0.1:0").await.expect("a socket");
        let answer = tokio::time::timeout(
            Duration::from_secs(2),
            query(&socket, server, Duration::from_secs(1)),
        )
        .await
        .expect("the query must not hang")
        .expect("the address the reflector reported");

        assert_eq!(answer, expected);
        responder.await.expect("the responder task");
    }

    #[test]
    fn a_blocking_query_returns_the_address_the_reflector_reports() {
        let reflector = std::net::UdpSocket::bind("127.0.0.1:0").expect("a reflector");
        let server = reflector.local_addr().expect("its port");
        let expected: SocketAddr = "192.0.2.9:54321".parse().expect("an address");
        let responder = std::thread::spawn(move || {
            let mut buffer = [0u8; 1500];
            let (len, peer) = reflector.recv_from(&mut buffer).expect("a request");
            assert_eq!(len, HEADER_BYTES);
            let transaction_id: TransactionId = buffer[8..20].try_into().expect("an id");
            let response = binding_response_for(&transaction_id, expected);
            reflector.send_to(&response, peer).expect("a response");
        });

        let socket = std::net::UdpSocket::bind("127.0.0.1:0").expect("a socket");
        let answer =
            query_blocking(&socket, server, Duration::from_secs(1)).expect("the reported address");

        assert_eq!(answer, expected);
        responder.join().expect("the responder thread");
    }

    #[test]
    fn a_blocking_query_times_out_on_a_silent_reflector() {
        let reflector = std::net::UdpSocket::bind("127.0.0.1:0").expect("a reflector");
        let server = reflector.local_addr().expect("its port");
        let socket = std::net::UdpSocket::bind("127.0.0.1:0").expect("a socket");

        let started = std::time::Instant::now();
        let result = query_blocking(&socket, server, Duration::from_millis(100));

        assert_eq!(result, Err(StunError::Timeout));
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "the blocking query must enforce its own timeout, took {:?}",
            started.elapsed()
        );
    }

    #[tokio::test]
    async fn a_response_with_a_wrong_transaction_id_times_out() {
        let reflector = UdpSocket::bind("127.0.0.1:0").await.expect("a reflector");
        let server = reflector.local_addr().expect("its port");
        let responder = tokio::spawn(async move {
            let mut buffer = [0u8; 1500];
            let (_, peer) = reflector.recv_from(&mut buffer).await.expect("a request");
            let mut wrong: TransactionId = buffer[8..20].try_into().expect("an id");
            wrong[0] ^= 0xff;
            let response =
                binding_response_for(&wrong, "192.0.2.9:54321".parse().expect("an address"));
            reflector
                .send_to(&response, peer)
                .await
                .expect("a response");
        });

        let socket = UdpSocket::bind("127.0.0.1:0").await.expect("a socket");
        let result = tokio::time::timeout(
            Duration::from_secs(2),
            query(&socket, server, Duration::from_millis(100)),
        )
        .await
        .expect("the query must enforce its own timeout");

        assert_eq!(result, Err(StunError::Timeout));
        responder.await.expect("the responder task");
    }

    /// Decode the hexadecimal test vectors from RFC 5769, which are written
    /// as byte pairs and whitespace.
    fn hex(text: &str) -> Vec<u8> {
        text.split_whitespace()
            .map(|pair| u8::from_str_radix(pair, 16).expect("a hexadecimal byte"))
            .collect()
    }

    fn transaction_id_of(text: &str) -> TransactionId {
        hex(text).try_into().expect("a 96-bit transaction id")
    }

    /// RFC 5769 §2.2, complete with SOFTWARE, MESSAGE-INTEGRITY and
    /// FINGERPRINT attributes this parser must skip.
    const IPV4_RESPONSE: &str = "
        01 01 00 3c 21 12 a4 42 b7 e7 a7 01 bc 34 d6 86 fa 87 df ae
        80 22 00 0b 74 65 73 74 20 76 65 63 74 6f 72 20
        00 20 00 08 00 01 a1 47 e1 12 a6 43
        00 08 00 14 2b 91 f5 99 fd 9e 90 c3 8c 74 89 f9 2a f9 ba 53 f0 6b e7 d7
        80 28 00 04 c0 7d 4c 96";

    /// RFC 5769 §2.3: the same shape with an IPv6 mapped address. The IPv6
    /// mask is the cookie plus the transaction id, so this vector catches a
    /// parser that XORs only the first four bytes.
    const IPV6_RESPONSE: &str = "
        01 01 00 48 21 12 a4 42 b7 e7 a7 01 bc 34 d6 86 fa 87 df ae
        80 22 00 0b 74 65 73 74 20 76 65 63 74 6f 72 20
        00 20 00 14 00 02 a1 47 01 13 a9 fa a5 d3 f1 79 bc 25 f4 b5 be d2 b9 d9
        00 08 00 14 a3 82 95 4e 4b e6 7b f1 17 84 c9 7c 82 92 c2 75 bf e3 ed 41
        80 28 00 04 c8 fb 0b 4c";

    const VECTOR_TRANSACTION_ID: &str = "b7 e7 a7 01 bc 34 d6 86 fa 87 df ae";

    #[test]
    fn a_binding_request_is_twenty_bytes_with_the_magic_cookie() {
        let (id, request) = binding_request();
        assert_eq!(request.len(), HEADER_BYTES);
        assert_eq!(&request[..2], &BINDING_REQUEST.to_be_bytes());
        assert_eq!(&request[2..4], &0u16.to_be_bytes());
        assert_eq!(&request[4..8], &MAGIC_COOKIE.to_be_bytes());
        assert_eq!(&request[8..], &id);
    }

    #[test]
    fn each_binding_request_gets_a_fresh_transaction_id() {
        // The id is 96 random bits; two draws colliding would be a broken
        // random source, not a flaky test.
        let (first, _) = binding_request();
        let (second, _) = binding_request();
        assert_ne!(first, second);
    }

    #[test]
    fn the_ipv4_test_vector_decodes_to_its_mapped_address() {
        let response = hex(IPV4_RESPONSE);
        let expected: SocketAddr = "192.0.2.1:32853".parse().expect("an address");
        assert_eq!(
            parse_binding_response(&response, &transaction_id_of(VECTOR_TRANSACTION_ID)),
            Ok(expected)
        );
    }

    #[test]
    fn the_ipv6_test_vector_decodes_to_its_mapped_address() {
        let response = hex(IPV6_RESPONSE);
        let expected: SocketAddr = "[2001:db8:1234:5678:11:2233:4455:6677]:32853"
            .parse()
            .expect("an address");
        assert_eq!(
            parse_binding_response(&response, &transaction_id_of(VECTOR_TRANSACTION_ID)),
            Ok(expected)
        );
    }

    #[test]
    fn a_response_with_the_wrong_transaction_id_is_refused() {
        let response = hex(IPV4_RESPONSE);
        assert_eq!(
            parse_binding_response(&response, &[0u8; TRANSACTION_ID_BYTES]),
            Err(StunError::WrongTransactionId)
        );
    }

    #[test]
    fn a_mangled_length_is_refused() {
        let mut response = hex(IPV4_RESPONSE);
        // 0x3d is not a multiple of four and does not describe the body.
        response[2..4].copy_from_slice(&0x003du16.to_be_bytes());
        assert!(matches!(
            parse_binding_response(&response, &transaction_id_of(VECTOR_TRANSACTION_ID)),
            Err(StunError::BadLength { declared: 0x3d, .. })
        ));

        // The other direction: a length that claims more than the datagram
        // carries must not make the parser read past the end.
        let mut overrun = hex(IPV4_RESPONSE);
        overrun[2..4].copy_from_slice(&0x0040u16.to_be_bytes());
        assert!(matches!(
            parse_binding_response(&overrun, &transaction_id_of(VECTOR_TRANSACTION_ID)),
            Err(StunError::BadLength { declared: 0x40, .. })
        ));
    }

    #[test]
    fn a_non_success_class_is_refused() {
        let mut response = hex(IPV4_RESPONSE);
        // Binding error response: same method, class 3.
        response[..2].copy_from_slice(&0x0111u16.to_be_bytes());
        assert!(matches!(
            parse_binding_response(&response, &transaction_id_of(VECTOR_TRANSACTION_ID)),
            Err(StunError::NotSuccess {
                message_type: 0x0111,
                class: 3
            })
        ));
    }

    #[test]
    fn a_wrong_magic_cookie_is_refused() {
        let mut response = hex(IPV4_RESPONSE);
        response[4] ^= 0xff;
        assert!(matches!(
            parse_binding_response(&response, &transaction_id_of(VECTOR_TRANSACTION_ID)),
            Err(StunError::WrongCookie { .. })
        ));
    }

    #[test]
    fn a_truncated_header_is_refused() {
        let response = hex(IPV4_RESPONSE);
        assert_eq!(
            parse_binding_response(&response[..10], &transaction_id_of(VECTOR_TRANSACTION_ID)),
            Err(StunError::TooShort { len: 10 })
        );
    }

    #[test]
    fn an_unknown_family_is_refused_rather_than_guessed() {
        let mut response = hex(IPV4_RESPONSE);
        // The family byte is the second byte of the XOR-MAPPED-ADDRESS
        // value; the attribute's value starts at byte 40 in this vector.
        response[41] = 0x03;
        assert_eq!(
            parse_binding_response(&response, &transaction_id_of(VECTOR_TRANSACTION_ID)),
            Err(StunError::UnsupportedFamily { family: 0x03 })
        );
    }
}
