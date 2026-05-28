// SPDX-FileCopyrightText: 2026 Meowdia Community
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Types for fields and attributes defined in [RFC8866](https://datatracker.ietf.org/doc/html/rfc8866)

use std::{
    borrow::Cow,
    fmt::Display,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    num::{NonZeroI64, NonZeroU16},
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

pub mod attributes;

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

/// > ```text
/// > r=<repeat interval> <active duration> <offsets from start-time>
/// > ```
/// [RFC8866-5.10](https://datatracker.ietf.org/doc/html/rfc8866#section-5.10)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepeatTimes {
    interval: i64,
    duration: i64,
    offsets: Vec<i64>,
}

fn parse_timed_time(val: &str) -> Option<i64> {
    let (mul, rest) = match val.char_indices().next_back()? {
        (i, 'd') => (60 * 60 * 24, &val[..i]),
        (i, 'h') => (60 * 60, &val[..i]),
        (i, 'm') => (60, &val[..i]),
        (i, 's') => (1, &val[..i]),
        (_, x) if x.is_numeric() => (1, val),
        _ => return None,
    };
    rest.parse().map(|v: i64| v * mul).ok()
}

impl FromStr for RepeatTimes {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut times = s.split(' ').map(parse_timed_time);

        let Some(interval) = times.next().flatten() else {
            return Err(());
        };
        let Some(duration) = times.next().flatten() else {
            return Err(());
        };

        let offsets = times.map(|o| o.ok_or(())).collect::<Result<_, _>>()?;

        Ok(Self {
            interval,
            duration,
            offsets,
        })
    }
}

impl Display for RepeatTimes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}", self.interval, self.duration)?;
        for offset in &self.offsets {
            write!(f, " {offset}")?;
        }
        Ok(())
    }
}

/// > ```text
/// > time-field =          %s"t" "=" start-time SP stop-time CRLF
/// > ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TimeField {
    pub start_time: i64,
    pub stop_time: Option<NonZeroI64>,
}

#[derive(Debug, Clone)]
pub struct Repeats<'a> {
    time_field: TimeField,
    repeat: usize,
    repeat_times: &'a RepeatTimes,
    offsets: &'a [i64],
}

impl Iterator for Repeats<'_> {
    type Item = i64;

    fn next(&mut self) -> Option<Self::Item> {
        let time = if let [offset, rest @ ..] = self.offsets {
            self.offsets = rest;
            self.time_field.start_time + (self.repeat_times.interval * self.repeat as i64) + offset
        } else {
            let (offset, rest) = &self.repeat_times.offsets.split_first().unwrap_or((&0, &[]));
            self.offsets = rest;

            if !self.repeat_times.offsets.is_empty() {
                self.repeat += 1;
            }

            let time = self.time_field.start_time
                + (self.repeat_times.interval * self.repeat as i64)
                + *offset;

            if self.repeat_times.offsets.is_empty() {
                self.repeat += 1;
            }

            time
        };

        if let Some(stop) = self.time_field.stop_time
            && time < stop.get()
        {
            Some(time)
        } else {
            None
        }
    }
}

impl TimeField {
    pub fn repeats<'a>(&self, repeat_times: &'a RepeatTimes) -> Repeats<'a> {
        Repeats {
            time_field: *self,
            repeat: 0,
            repeat_times,
            offsets: &repeat_times.offsets,
        }
    }
}

impl FromStr for TimeField {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (start, stop) = s.split_once(' ').ok_or(())?;

        Ok(Self {
            start_time: start.parse().map_err(|_| ())?,
            stop_time: stop.parse::<i64>().map(NonZeroI64::new).map_err(|_| ())?,
        })
    }
}

impl Display for TimeField {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} {}",
            self.start_time,
            self.stop_time.map(NonZeroI64::get).unwrap_or(0)
        )
    }
}

pub struct TimezoneAdjustments {
    /// (<adjustment time>, <offset>) in seconds
    pub adjustments: Vec<(i64, i64)>,
}

impl FromStr for TimezoneAdjustments {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parts = s.split(' ');
        let mut adjs = Vec::new();
        loop {
            match (parts.next(), parts.next()) {
                (Some(adj), Some(offs)) => {
                    let (Ok(adj), Some(offs)) = (adj.parse::<i64>(), parse_timed_time(offs)) else {
                        break Err(());
                    };
                    adjs.push((adj, offs));
                }
                (None, None) => break Ok(Self { adjustments: adjs }),
                _ => break Err(()),
            }
        }
    }
}

impl Display for TimezoneAdjustments {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let [first, rest @ ..] = &self.adjustments[..] else {
            return Ok(());
        };

        write!(f, "{} {}", first.0, first.1)?;

        for (adj, offs) in rest {
            write!(f, " {adj} {offs}")?;
        }

        Ok(())
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
            MulticastIpv6Addr, RepeatTimes, TimeField, UnicastIpv4Addr, UnicastIpv6Addr,
            parse_timed_time,
        },
    };

    #[test]
    fn repeat_time_value() {
        assert_eq!(parse_timed_time("7d"), Some(7 * 60 * 60 * 24));
        assert_eq!(parse_timed_time("6h"), Some(6 * 60 * 60));
        assert_eq!(parse_timed_time("10m"), Some(10 * 60));
        assert_eq!(parse_timed_time("10s"), Some(10));
        assert_eq!(parse_timed_time("10"), Some(10));
    }

    #[test]
    fn repeat_time_iter() {
        let timef = TimeField {
            start_time: 0,
            stop_time: Some((60 * 60 * 24 * 10).try_into().unwrap()),
        };

        let repeats = RepeatTimes {
            interval: 3600,
            duration: 10,
            offsets: vec![0, 1800],
        };

        let repeats: Vec<_> = timef.repeats(&repeats).collect();

        assert_eq!(&repeats[..4], [0, 1800, 3600, 5400]);

        assert!(
            repeats
                .last()
                .is_some_and(|l| *l < timef.stop_time.unwrap().get())
        );

        let repeats = RepeatTimes {
            interval: 1800,
            duration: 10,
            offsets: vec![0, 60, 120],
        };

        let repeats: Vec<_> = timef.repeats(&repeats).collect();

        assert_eq!(&repeats[..6], [0, 60, 120, 1800, 1860, 1920]);

        assert!(
            repeats
                .last()
                .is_some_and(|l| *l < timef.stop_time.unwrap().get())
        );

        let repeats = RepeatTimes {
            interval: 1800,
            duration: 10,
            offsets: vec![0],
        };

        let repeats: Vec<_> = timef.repeats(&repeats).collect();

        assert_eq!(&repeats[..3], [0, 1800, 3600]);

        assert!(
            repeats
                .last()
                .is_some_and(|l| *l < timef.stop_time.unwrap().get())
        );

        let zero_offsets = RepeatTimes {
            interval: 1800,
            duration: 10,
            offsets: vec![],
        };

        let zero_offset_repeats: Vec<_> = timef.repeats(&zero_offsets).collect();
        assert_eq!(zero_offset_repeats, repeats);
    }

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
