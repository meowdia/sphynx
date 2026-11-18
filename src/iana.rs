// SPDX-FileCopyrightText: 2026 Meowdia Community
// SPDX-License-Identifier: MIT OR Apache-2.0

pub use iana_sdp::sdp::{Addrtype, Bwtype, Media, Nettype, Proto};

/// Looks up the IANA-registered entry whose token exactly matches `name`.
pub(crate) trait Registered: Sized {
    fn registered(name: &str) -> Option<Self>;
}

macro_rules! impl_registered {
    ($($registry:ident),* $(,)?) => {
        $(
            impl Registered for $registry<'static> {
                fn registered(name: &str) -> Option<Self> {
                    Self::ALL.iter().copied().find(|entry| entry.as_str() == name)
                }
            }
        )*
    };
}

impl_registered!(Addrtype, Bwtype, Media, Nettype, Proto);
