// SPDX-FileCopyrightText: 2026 Meowdia Community
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Types for fields and attributes defined in [RFC8866](https://datatracker.ietf.org/doc/html/rfc8866)

use std::{
    borrow::Cow,
    fmt::Display,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    num::NonZeroU16,
    ops::Deref,
    str::FromStr,
};

use thiserror::Error;

use crate::{
    error::SdpIssueKind,
    iana::{
        KnownAddressType, KnownBandwidthType, KnownMediaType, KnownNetworkType,
        KnownTransportProtocol,
    },
};

macro_rules! enum_attribute {
    (
        $(#[$doc:meta])*
        $enum_name:ident,
        $($name:ident => $value:literal),+
    ) => {
        $(#[$doc])*
        pub enum $enum_name {
            $($name),+
        }

        impl $enum_name {
            pub const ALL: &[Self] = &[$(Self::$name),+];

            pub const fn as_str(&self) -> &'static str {
                match self {
                    $(Self::$name => $value),+
                }
            }

            fn from_str(val: &str) -> Option<Self> {
                match val {
                    $($value => Some(Self::$name),)+
                    _ => None
                }
            }

            pub fn from_str_ignore_ascii_case(val: &str) -> Option<(Self, Option<&str>)> {
                match val {
                    $($value => Some((Self::$name, None)),)+
                    $(v if v.eq_ignore_ascii_case($value) => Some((Self::$name, Some(v))),)+
                    _ => None
                }
            }
        }

        impl AsRef<str> for $enum_name {
            fn as_ref(&self) -> &str {
                self.as_str()
            }
        }

        impl FromStr for $enum_name {
            type Err = ();

            fn from_str(val: &str) -> Result<Self, Self::Err> {
                Self::from_str(val).ok_or(())
            }
        }

        impl TryFrom<&str> for $enum_name {
            type Error = ();

            fn try_from(value: &str) -> Result<Self, Self::Error> {
                Self::from_str(value).ok_or(())
            }
        }

        impl Display for $enum_name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_str())
            }
        }
    }
}

/// > ```text
/// > b=<bwtype>:<bandwidth>
/// > ```
/// [RFC8866-5.8](https://datatracker.ietf.org/doc/html/rfc8866#section-5.8)
pub struct BandwidthInformation {
    pub bwtype: KnownBandwidthType,
    /// bandwidth in kilobits per second (kb/s)
    pub bandwidth: u64,
}

impl FromStr for BandwidthInformation {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let Some((bwtype, bandwidth)) = s.split_once(":") else {
            return Err(());
        };

        Ok(Self {
            bwtype: KnownBandwidthType::from_name(bwtype).ok_or(())?,
            bandwidth: bandwidth.parse().map_err(|_| ())?,
        })
    }
}

enum_attribute!(
    /// [RFC8866-6.9](https://datatracker.ietf.org/doc/html/rfc8866#section-6.9)
    ConferenceType,
    Broadcast => "broadcast",
    Meeting => "meetings",
    Moderated => "moderated",
    Test => "test",
    H332 => "H332"
);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ConnectionAddress<'a> {
    V4Unicast(UnicastIpv4Addr),
    V4Multicast(MulticastIpv4Addr),
    V6Unicast(UnicastIpv6Addr),
    V6Multicast(MulticastIpv6Addr),
    Fqdn(Cow<'a, str>),
}

impl<'a> ConnectionAddress<'a> {
    pub fn into_owned(self) -> ConnectionAddress<'static> {
        match self {
            ConnectionAddress::V4Unicast(ipv4_addr) => ConnectionAddress::V4Unicast(ipv4_addr),
            ConnectionAddress::V4Multicast(ipv4_addr) => ConnectionAddress::V4Multicast(ipv4_addr),
            ConnectionAddress::V6Unicast(ipv6_addr) => ConnectionAddress::V6Unicast(ipv6_addr),
            ConnectionAddress::V6Multicast(ipv6_addr) => ConnectionAddress::V6Multicast(ipv6_addr),
            ConnectionAddress::Fqdn(cow) => ConnectionAddress::Fqdn(Cow::Owned(cow.into_owned())),
        }
    }

    pub const fn addr(&self) -> Option<IpAddr> {
        match self {
            ConnectionAddress::V4Unicast(addr) => Some(IpAddr::V4(addr.addr())),
            ConnectionAddress::V4Multicast(addr) => Some(IpAddr::V4(addr.addr())),
            ConnectionAddress::V6Unicast(addr) => Some(IpAddr::V6(addr.addr())),
            ConnectionAddress::V6Multicast(addr) => Some(IpAddr::V6(addr.addr())),
            _ => None,
        }
    }

    fn from_ip(s: &str) -> Option<Self> {
        let mut parts = s.splitn(3, '/');
        let ip = parts.next()?;
        let addr = IpAddr::from_str(ip).ok()?;
        match (
            addr,
            addr.is_multicast(),
            parts.next().map(NonZeroU16::from_str).transpose().ok()?,
            parts.next().map(NonZeroU16::from_str).transpose().ok()?,
        ) {
            (IpAddr::V4(addr), true, Some(ttl), num_addr) => {
                Some(Self::V4Multicast(MulticastIpv4Addr {
                    addr,
                    ttl,
                    num_addr,
                }))
            }
            (IpAddr::V4(addr), _, None, None) => Some(Self::V4Unicast(UnicastIpv4Addr(addr))),
            (IpAddr::V6(addr), true, num_addr, None) => {
                Some(Self::V6Multicast(MulticastIpv6Addr { addr, num_addr }))
            }
            (IpAddr::V6(addr), _, None, None) => Some(Self::V6Unicast(UnicastIpv6Addr(addr))),
            _ => None,
        }
    }
}

impl<'a> From<&'a str> for ConnectionAddress<'a> {
    fn from(value: &'a str) -> Self {
        Self::from_ip(value).unwrap_or(ConnectionAddress::Fqdn(Cow::Borrowed(value)))
    }
}

impl From<String> for ConnectionAddress<'static> {
    fn from(value: String) -> Self {
        Self::from_ip(&value).unwrap_or(ConnectionAddress::Fqdn(Cow::Owned(value)))
    }
}

impl Display for ConnectionAddress<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConnectionAddress::V4Unicast(addr) => write!(f, "{addr}"),
            ConnectionAddress::V4Multicast(addr) => write!(f, "{addr}"),
            ConnectionAddress::V6Unicast(addr) => write!(f, "{addr}"),
            ConnectionAddress::V6Multicast(addr) => write!(f, "{addr}"),
            ConnectionAddress::Fqdn(cow) => f.write_str(cow),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionInformation<'a> {
    pub net_type: KnownNetworkType,
    pub addr_type: KnownAddressType,
    pub addr: ConnectionAddress<'a>,
}

impl<'a> ConnectionInformation<'a> {
    pub const fn new(
        net_type: KnownNetworkType,
        addr_type: KnownAddressType,
        addr: ConnectionAddress<'a>,
    ) -> Self {
        Self {
            net_type,
            addr_type,
            addr,
        }
    }

    pub fn into_owned(self) -> ConnectionInformation<'static> {
        ConnectionInformation::new(self.net_type, self.addr_type, self.addr.into_owned())
    }
}

impl<'a> TryFrom<&'a str> for ConnectionInformation<'a> {
    type Error = SdpIssueKind<'a>;

    fn try_from(value: &'a str) -> Result<Self, Self::Error> {
        let mut parts = value.splitn(3, ' ');

        let Some(net_type) = parts.next().and_then(KnownNetworkType::from_name) else {
            return Err(SdpIssueKind::MalformedMediaSection);
        };
        let Some(addr_type) = parts.next().and_then(KnownAddressType::from_name) else {
            return Err(SdpIssueKind::MalformedMediaSection);
        };
        let Some(addr) = parts.next().map(ConnectionAddress::from) else {
            return Err(SdpIssueKind::MalformedMediaSection);
        };

        Ok(Self {
            net_type,
            addr_type,
            addr,
        })
    }
}

impl Display for ConnectionInformation<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {} {}", self.net_type, self.addr_type, self.addr)
    }
}

enum_attribute!(
    /// [RFC8866-6.7](https://datatracker.ietf.org/doc/html/rfc8866#section-6.7)
    Direction,
    SendOnly => "sendonly",
    SendRecv => "sendrecv",
    RecvOnly => "recvonly",
    Inactive => "inactive"
);

/// > ```text
/// > fmtp-value = fmt SP format-specific-params
/// > format-specific-params = byte-string
/// >    ; Notes:
/// >    ; - The format parameters are media type parameters and
/// >    ;   need to reflect their syntax.
/// > ```
///
/// > ```text
/// > fmt = token
/// >       ;typically an RTP payload type for audio
/// >       ;and video media
/// > ```
///
/// [RFC8866-6.15](https://datatracker.ietf.org/doc/html/rfc8866#section-6.15)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Fmtp<'a> {
    pub format: Cow<'a, str>,
    pub params: Cow<'a, str>,
}

impl<'a> Fmtp<'a> {
    pub fn new(format: impl Into<Cow<'a, str>>, params: impl Into<Cow<'a, str>>) -> Self {
        Self {
            format: format.into(),
            params: params.into(),
        }
    }

    pub const fn new_owned(format: String, params: String) -> Fmtp<'static> {
        Fmtp {
            format: Cow::Owned(format),
            params: Cow::Owned(params),
        }
    }

    pub fn into_owned(self) -> Fmtp<'static> {
        Fmtp::new_owned(self.format.into_owned(), self.params.into_owned())
    }
}

impl<'a> TryFrom<&'a str> for Fmtp<'a> {
    type Error = ();

    fn try_from(value: &'a str) -> Result<Self, Self::Error> {
        let mut parts = value.splitn(2, ' ');
        let format = parts.next().ok_or(())?;
        let params = parts.next().ok_or(())?;

        Ok(Self::new(format, params))
    }
}

impl<'a> Display for Fmtp<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}", self.format, self.params)
    }
}

/// [RFC8866-6.13](https://datatracker.ietf.org/doc/html/rfc8866#section-6.13)
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct FrameRate(pub f32);

impl FromStr for FrameRate {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parsed: f32 = s.parse().map_err(|_| ())?;
        if parsed.is_normal() && parsed.is_sign_positive() {
            Ok(Self(parsed))
        } else {
            Err(())
        }
    }
}

impl Display for FrameRate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// > ```text
/// > media-field =         %s"m" "=" media SP port ["/" integer]
/// >                           SP proto 1*(SP fmt) CRLF
/// > ```
///
/// [RFC8866-5.14](https://datatracker.ietf.org/doc/html/rfc8866#section-5.14)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaField<'a> {
    pub media_type: KnownMediaType,
    pub port: (u16, Option<NonZeroU16>),
    pub proto: KnownTransportProtocol,
    pub fmt: Vec<Cow<'a, str>>,
}

impl<'a> MediaField<'a> {
    pub fn into_owned(self) -> MediaField<'static> {
        MediaField {
            fmt: self
                .fmt
                .into_iter()
                .map(|f| Cow::Owned(f.into_owned()))
                .collect(),
            ..self
        }
    }
}

impl<'a> TryFrom<&'a str> for MediaField<'a> {
    type Error = ();

    fn try_from(value: &'a str) -> Result<Self, Self::Error> {
        let mut parts = value.split(' ');

        let Some(media_type) = parts.next().and_then(KnownMediaType::from_name) else {
            return Err(());
        };

        let Some(port) = parts.next() else {
            return Err(());
        };
        let (port, num_port) = if let Some((p, n)) = port.split_once('/') {
            (p.parse().map_err(|_| ())?, Some(n.parse().map_err(|_| ())?))
        } else {
            (port.parse().map_err(|_| ())?, None)
        };

        let Some(proto) = parts.next().and_then(KnownTransportProtocol::from_name) else {
            return Err(());
        };

        let Some(first_fmt) = parts.next() else {
            return Err(());
        };

        let fmt = core::iter::once(first_fmt)
            .chain(parts)
            .map(Into::into)
            .collect();

        Ok(Self {
            media_type,
            port: (port, num_port),
            proto,
            fmt,
        })
    }
}

impl Display for MediaField<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ", self.media_type)?;

        match self.port {
            (port, Some(num)) => write!(f, "{port}/{num} ")?,
            (port, None) => write!(f, "{port} ")?,
        };

        write!(f, "{}", self.proto)?;

        for fmt in &self.fmt {
            write!(f, " {fmt}")?;
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum MulticastAddrParseError {
    #[error(transparent)]
    AddrParseError(#[from] std::net::AddrParseError),
    #[error("not a multicast address")]
    NotMulticast,
    #[error("invalid connection address")]
    Generic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MulticastIpv4Addr {
    addr: Ipv4Addr,
    pub ttl: NonZeroU16,
    pub num_addr: Option<NonZeroU16>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MulticastIpv6Addr {
    addr: Ipv6Addr,
    pub num_addr: Option<NonZeroU16>,
}

impl MulticastIpv4Addr {
    /// Returns `None` if `addr` is not a multicast address
    pub const fn new(
        addr: Ipv4Addr,
        ttl: NonZeroU16,
        num_addr: Option<NonZeroU16>,
    ) -> Option<Self> {
        if !addr.is_multicast() {
            return None;
        }
        Some(Self {
            addr,
            ttl,
            num_addr,
        })
    }

    pub const fn addr(&self) -> Ipv4Addr {
        self.addr
    }

    pub fn addresses(&self) -> impl Iterator<Item = Ipv4Addr> {
        (0..self.num_addr.unwrap_or(NonZeroU16::MIN).get()).filter_map(|i| {
            u32::from(self.addr)
                .checked_add(i.into())
                .map(Ipv4Addr::from)
        })
    }
}

impl Deref for MulticastIpv4Addr {
    type Target = Ipv4Addr;

    fn deref(&self) -> &Self::Target {
        &self.addr
    }
}

impl FromStr for MulticastIpv4Addr {
    type Err = MulticastAddrParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parts = s.splitn(3, '/');

        let Some(addr) = parts.next().map(|a| a.parse::<Ipv4Addr>()).transpose()? else {
            return Err(MulticastAddrParseError::Generic);
        };
        if !addr.is_multicast() {
            return Err(MulticastAddrParseError::NotMulticast);
        }
        let Some(ttl) = parts.next().and_then(|t| t.parse().ok()) else {
            return Err(MulticastAddrParseError::Generic);
        };
        let num_addr = parts
            .next()
            .map(|n| n.parse().map_err(|_| MulticastAddrParseError::Generic))
            .transpose()?;

        Ok(Self {
            addr,
            ttl,
            num_addr,
        })
    }
}

impl Display for MulticastIpv4Addr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.addr(), self.ttl)?;
        if let Some(num_addr) = self.num_addr {
            write!(f, "/{num_addr}")?;
        }
        Ok(())
    }
}

impl MulticastIpv6Addr {
    /// Returns `None` if `addr` is not a multicast address
    pub const fn new(addr: Ipv6Addr, num_addr: Option<NonZeroU16>) -> Option<Self> {
        if !addr.is_multicast() {
            return None;
        }
        Some(Self { addr, num_addr })
    }

    pub const fn addr(&self) -> Ipv6Addr {
        self.addr
    }

    pub fn addresses(&self) -> impl Iterator<Item = Ipv6Addr> {
        (0..self.num_addr.unwrap_or(NonZeroU16::MIN).get()).filter_map(|i| {
            u128::from_be_bytes(self.addr.octets())
                .checked_add(i.into())
                .map(u128::to_be_bytes)
                .map(Ipv6Addr::from)
        })
    }
}

impl Deref for MulticastIpv6Addr {
    type Target = Ipv6Addr;

    fn deref(&self) -> &Self::Target {
        &self.addr
    }
}

impl FromStr for MulticastIpv6Addr {
    type Err = MulticastAddrParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parts = s.splitn(2, '/');

        let Some(addr) = parts.next().map(|a| a.parse::<Ipv6Addr>()).transpose()? else {
            return Err(MulticastAddrParseError::Generic);
        };
        if !addr.is_multicast() {
            return Err(MulticastAddrParseError::NotMulticast);
        }
        let num_addr = parts
            .next()
            .map(|n| n.parse().map_err(|_| MulticastAddrParseError::Generic))
            .transpose()?;

        Ok(Self { addr, num_addr })
    }
}

impl Display for MulticastIpv6Addr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.addr())?;
        if let Some(num_addr) = self.num_addr {
            write!(f, "/{num_addr}")?;
        }
        Ok(())
    }
}

pub struct NonWsString<T: Deref<Target = str>>(T);

impl<T: Deref<Target = str>> NonWsString<T> {
    pub fn new(val: T) -> Option<Self> {
        if val.deref().chars().any(char::is_whitespace) {
            None
        } else {
            Some(Self(val))
        }
    }

    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T: Deref<Target = str>> Deref for NonWsString<T> {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

enum_attribute!(
    /// [RFC8866-6.8](https://datatracker.ietf.org/doc/html/rfc8866#section-6.8)
    Orientation,
    Landscape => "landscape",
    Portrait => "portrait",
    Seascape => "seascape"
);

/// [RFC8866-6.4](https://datatracker.ietf.org/doc/html/rfc8866#section-6.4)
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct PacketTime(pub f32);

impl FromStr for PacketTime {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parsed: f32 = s.parse().map_err(|_| ())?;
        if parsed.is_normal() && parsed.is_sign_positive() {
            Ok(Self(parsed))
        } else {
            Err(())
        }
    }
}

impl Display for PacketTime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// [RFC8866-6.14](https://datatracker.ietf.org/doc/html/rfc8866#section-6.14)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Quality(u8);

impl Quality {
    /// Returns `None` if `quality` is not in range `0..=10`
    pub const fn new(quality: u8) -> Option<Self> {
        match quality {
            q @ 0..=10 => Some(Self(q)),
            _ => None,
        }
    }
}

impl FromStr for Quality {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s.parse().map_err(|_| ())?).ok_or(())
    }
}

impl Display for Quality {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// > ```text
/// > time-field =          %s"t" "=" start-time SP stop-time CRLF
/// > ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TimeField {
    pub start_time: i64,
    pub stop_time: i64,
}

impl FromStr for TimeField {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (start, stop) = s.split_once(' ').ok_or(())?;

        Ok(Self {
            start_time: start.parse().map_err(|_| ())?,
            stop_time: stop.parse().map_err(|_| ())?,
        })
    }
}

impl Display for TimeField {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}", self.start_time, self.stop_time)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UnicastIpv4Addr(Ipv4Addr);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UnicastIpv6Addr(Ipv6Addr);

macro_rules! unicast_impl {
    ($name:ident, $kind:ty) => {
        impl $name {
            /// Returns `None` if `addr` is a multicast address
            pub const fn new(addr: $kind) -> Option<Self> {
                if addr.is_multicast() {
                    return None;
                }
                return Some(Self(addr));
            }

            pub const fn addr(&self) -> $kind {
                self.0
            }
        }

        impl Deref for $name {
            type Target = $kind;

            fn deref(&self) -> &$kind {
                &self.0
            }
        }

        impl FromStr for $name {
            type Err = ();

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                if let Ok(addr) = s.parse::<$kind>()
                    && !addr.is_multicast()
                {
                    Ok(Self(addr))
                } else {
                    Err(())
                }
            }
        }

        impl Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.0)
            }
        }
    };
}

unicast_impl!(UnicastIpv4Addr, Ipv4Addr);
unicast_impl!(UnicastIpv6Addr, Ipv6Addr);

#[cfg(test)]
mod test {
    use std::net::Ipv4Addr;

    use crate::{
        iana::{KnownAddressType, KnownNetworkType},
        sdp::{
            ConnectionAddress, ConnectionInformation, MediaField, MulticastIpv4Addr,
            MulticastIpv6Addr, UnicastIpv4Addr, UnicastIpv6Addr,
        },
    };

    #[test]
    fn media_description() {
        let test_md = |s, md| {
            let Ok(parsed) = MediaField::try_from(s) else {
                panic!("Failed to parse media description from {s}");
            };
            assert_eq!(parsed, md);
            assert_eq!(s, md.to_string());
        };

        test_md(
            "video 6767 RTP/AVP 67",
            MediaField {
                media_type: crate::iana::KnownMediaType::Video,
                port: (6767, None),
                proto: crate::iana::KnownTransportProtocol::RtpAvp,
                fmt: vec!["67".into()],
            },
        );

        test_md(
            "video 6767/2 RTP/SAVPF 67",
            MediaField {
                media_type: crate::iana::KnownMediaType::Video,
                port: (6767, Some(2.try_into().unwrap())),
                proto: crate::iana::KnownTransportProtocol::RtpSavpf,
                fmt: vec!["67".into()],
            },
        );
    }

    #[test]
    fn connection_info() {
        let test_parse = |s, e| {
            let Ok(i) = ConnectionInformation::try_from(s) else {
                panic!("Failed to parse ConnectionInformation from {s:?}");
            };
            assert_eq!(i, e);
            assert_eq!(s, e.to_string());
        };

        test_parse(
            "IN IP4 233.252.0.1/127",
            ConnectionInformation::new(
                KnownNetworkType::InValue,
                KnownAddressType::Ip4,
                ConnectionAddress::V4Multicast(
                    MulticastIpv4Addr::new(
                        Ipv4Addr::new(233, 252, 0, 1),
                        127.try_into().unwrap(),
                        None,
                    )
                    .unwrap(),
                ),
            ),
        );
        test_parse(
            "IN IP6 ff00::db8:0:101/3",
            ConnectionInformation::new(
                KnownNetworkType::InValue,
                KnownAddressType::Ip6,
                ConnectionAddress::V6Multicast(
                    MulticastIpv6Addr::new(
                        "ff00::db8:0:101".parse().unwrap(),
                        Some(3.try_into().unwrap()),
                    )
                    .unwrap(),
                ),
            ),
        );
        test_parse(
            "IN IP6 cafe::db8:0:101",
            ConnectionInformation::new(
                KnownNetworkType::InValue,
                KnownAddressType::Ip6,
                ConnectionAddress::V6Unicast(
                    UnicastIpv6Addr::new("cafe::db8:0:101".parse().unwrap()).unwrap(),
                ),
            ),
        );
    }

    #[test]
    fn connection_address_parse() {
        assert_eq!(
            ConnectionAddress::from("192.168.67.1"),
            ConnectionAddress::V4Unicast(
                UnicastIpv4Addr::new(Ipv4Addr::new(192, 168, 67, 1)).unwrap()
            )
        );

        assert_eq!(
            ConnectionAddress::from("224.0.0.1/64"),
            ConnectionAddress::V4Multicast(
                MulticastIpv4Addr::new(Ipv4Addr::new(224, 0, 0, 1), 64.try_into().unwrap(), None)
                    .unwrap()
            )
        );
        assert_eq!(
            ConnectionAddress::from("224.0.0.1/64/2"),
            ConnectionAddress::V4Multicast(
                MulticastIpv4Addr::new(
                    Ipv4Addr::new(224, 0, 0, 1),
                    64.try_into().unwrap(),
                    Some(2.try_into().unwrap())
                )
                .unwrap()
            )
        );

        assert_eq!(
            ConnectionAddress::from("80:0:0:80::01"),
            ConnectionAddress::V6Unicast(
                UnicastIpv6Addr::new("80:0:0:80::01".parse().unwrap()).unwrap()
            )
        );

        assert_eq!(
            ConnectionAddress::from("ff00:0:0:80::1"),
            ConnectionAddress::V6Multicast(
                MulticastIpv6Addr::new("ff00:0:0:80::1".parse().unwrap(), None).unwrap()
            )
        );

        assert_eq!(
            ConnectionAddress::from("ff00:0:0:80::1/2"),
            ConnectionAddress::V6Multicast(
                MulticastIpv6Addr::new(
                    "ff00:0:0:80::1".parse().unwrap(),
                    Some(2.try_into().unwrap())
                )
                .unwrap()
            )
        );

        assert_eq!(
            ConnectionAddress::from("jiffly.cloud"),
            ConnectionAddress::Fqdn("jiffly.cloud".into())
        );
    }

    #[test]
    fn multicast_address_iter() {
        let v4_addr = MulticastIpv4Addr::new(
            Ipv4Addr::new(224, 0, 0, 0),
            64.try_into().unwrap(),
            Some(4.try_into().unwrap()),
        )
        .unwrap();
        let mut v4_addresses = v4_addr.addresses();
        assert_eq!(v4_addresses.next(), Some(Ipv4Addr::new(224, 0, 0, 0)));
        assert_eq!(v4_addresses.next(), Some(Ipv4Addr::new(224, 0, 0, 1)));
        assert_eq!(v4_addresses.next(), Some(Ipv4Addr::new(224, 0, 0, 2)));
        assert_eq!(v4_addresses.next(), Some(Ipv4Addr::new(224, 0, 0, 3)));
        assert_eq!(v4_addresses.next(), None);

        let v4_addr = MulticastIpv4Addr::new(
            Ipv4Addr::new(224, 0, 255, 255),
            64.try_into().unwrap(),
            Some(4.try_into().unwrap()),
        )
        .unwrap();
        let mut v4_addresses = v4_addr.addresses();
        assert_eq!(v4_addresses.next(), Some(Ipv4Addr::new(224, 0, 255, 255)));
        assert_eq!(v4_addresses.next(), Some(Ipv4Addr::new(224, 1, 0, 0)));
        assert_eq!(v4_addresses.next(), Some(Ipv4Addr::new(224, 1, 0, 1)));
        assert_eq!(v4_addresses.next(), Some(Ipv4Addr::new(224, 1, 0, 2)));
        assert_eq!(v4_addresses.next(), None);

        let v6_addr =
            MulticastIpv6Addr::new("ff00::".parse().unwrap(), Some(4.try_into().unwrap())).unwrap();
        let mut v6_addresses = v6_addr.addresses();
        assert_eq!(v6_addresses.next(), Some("ff00::".parse().unwrap()));
        assert_eq!(v6_addresses.next(), Some("ff00::1".parse().unwrap()));
        assert_eq!(v6_addresses.next(), Some("ff00::2".parse().unwrap()));
        assert_eq!(v6_addresses.next(), Some("ff00::3".parse().unwrap()));
        assert_eq!(v6_addresses.next(), None);

        let v6_addr =
            MulticastIpv6Addr::new("ff00::ffff".parse().unwrap(), Some(4.try_into().unwrap()))
                .unwrap();
        let mut v6_addresses = v6_addr.addresses();
        assert_eq!(v6_addresses.next(), Some("ff00::ffff".parse().unwrap()));
        assert_eq!(v6_addresses.next(), Some("ff00::1:0".parse().unwrap()));
        assert_eq!(v6_addresses.next(), Some("ff00::1:1".parse().unwrap()));
        assert_eq!(v6_addresses.next(), Some("ff00::1:2".parse().unwrap()));
        assert_eq!(v6_addresses.next(), None);
    }
}
