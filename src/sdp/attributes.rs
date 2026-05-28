// SPDX-FileCopyrightText: 2026 Meowdia Community
// SPDX-License-Identifier: MIT OR Apache-2.0

use std::{borrow::Cow, fmt::Display, num::NonZeroU8, str::FromStr};

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

enum_attribute!(
    /// [RFC8866-6.9](https://datatracker.ietf.org/doc/html/rfc8866#section-6.9)
    ConferenceType,
    Broadcast => "broadcast",
    Meeting => "meetings",
    Moderated => "moderated",
    Test => "test",
    H332 => "H332"
);

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

    pub fn params_iter(&self) -> impl Iterator<Item = (&str, Option<&str>)> {
        self.params.split(';').map(|p| {
            p.split_once('=')
                .map(|(k, v)| (k, Some(v)))
                .unwrap_or((p, None))
        })
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RtpMap<'a> {
    pub payload_type: u8,
    pub encoding: Cow<'a, str>,
    pub clock_rate: u32,
    pub channels: Option<NonZeroU8>,
}

impl<'a> RtpMap<'a> {
    pub fn new(
        payload_type: u8,
        encoding: impl Into<Cow<'a, str>>,
        clock_rate: u32,
        channels: Option<NonZeroU8>,
    ) -> Self {
        Self {
            payload_type,
            encoding: encoding.into(),
            clock_rate,
            channels,
        }
    }

    pub fn into_owned(self) -> RtpMap<'static> {
        RtpMap {
            encoding: Cow::Owned(self.encoding.into_owned()),
            ..self
        }
    }
}

impl<'a> TryFrom<&'a str> for RtpMap<'a> {
    type Error = ();

    fn try_from(value: &'a str) -> Result<Self, Self::Error> {
        let Some((ptype, rest)) = value.split_once(' ') else {
            return Err(());
        };

        let Ok(payload_type) = ptype.parse() else {
            return Err(());
        };

        let Some((encoding, rest)) = rest.split_once('/') else {
            return Err(());
        };

        let (clock_rate, channels) = if let Some((cr, c)) = rest.split_once('/') {
            let (Ok(cr), Ok(c)) = (cr.parse(), c.parse()) else {
                return Err(());
            };
            (cr, Some(c))
        } else {
            let Ok(cr) = rest.parse() else { return Err(()) };
            (cr, None)
        };

        Ok(Self {
            payload_type,
            encoding: encoding.into(),
            clock_rate,
            channels,
        })
    }
}

impl Display for RtpMap<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} {}/{}",
            self.payload_type, self.encoding, self.clock_rate
        )?;
        if let Some(ch) = self.channels {
            write!(f, "/{ch}")?;
        };
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use crate::sdp::attributes::RtpMap;

    #[test]
    fn rtpmap() {
        let test = |s, e| {
            let Ok(parsed) = RtpMap::try_from(s) else {
                panic!("failed to parse RtpMap from {s:?}");
            };
            assert_eq!(e, parsed);
            assert_eq!(parsed.to_string(), s);
        };

        test(
            "96 L16/48000/2",
            RtpMap::new(96, "L16", 48000, Some(2.try_into().unwrap())),
        );

        test("96 L16/48000", RtpMap::new(96, "L16", 48000, None));

        test("0 L16/48000", RtpMap::new(0, "L16", 48000, None));
    }
}
