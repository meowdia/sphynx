/// An unparsed SDP line, with only its type
/// [RFC8866-5](https://datatracker.ietf.org/doc/html/rfc8866#section-5)
pub struct RawLine<'a> {
    /// ASCII character for line type: b'm', b'c', b'a', etc.
    pub kind: u8,
    pub value: &'a str,
}

/// [RFC8866-5.14](https://datatracker.ietf.org/doc/html/rfc8866#section-5.14)
pub struct RawMediaDescription<'a> {
    /// the m line for this media section
    pub description: RawLine<'a>,
    pub lines: Vec<RawLine<'a>>,
}

/// > The session-level section starts with a "v=" line and continues to the first
/// > media description (or the end of the whole description, whichever comes first).
///
/// [RFC8866-5](https://datatracker.ietf.org/doc/html/rfc8866#section-5)
pub struct RawSession<'a> {
    pub session: Vec<RawLine<'a>>,
    pub media_sections: Vec<RawMediaDescription<'a>>,
}
