// SPDX-FileCopyrightText: 2026 Meowdia Community
// SPDX-License-Identifier: MIT OR Apache-2.0

use std::{
    collections::{HashMap, HashSet},
    fmt::Display,
    hash::{Hash, Hasher},
    iter::FusedIterator,
    slice,
};

use crate::error::{Collector, Diagnostic, HandlingMode, SdpIssue, SdpIssueKind, SdpLocation};

/// An unparsed SDP line, with only its type.
/// [RFC8866-5](https://datatracker.ietf.org/doc/html/rfc8866#section-5)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RawLine<'a> {
    /// ASCII character for line type: `b'm'`, `b'c'`, `b'a'`, etc.
    pub kind: u8,
    pub value: &'a str,
}

/// Raw key/value split for keyed SDP line kinds such as `a=`, `b=`, and `k=`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RawKeyValue<'a> {
    pub key: &'a str,
    pub value: Option<&'a str>,
}

/// Ordered raw SDP lines plus precomputed lookup indexes.
#[derive(Debug, Clone)]
pub struct RawSection<'a> {
    lines: Vec<RawLine<'a>>,
    lookup: RawSectionLookup<'a>,
}

/// Iterator over indexed raw SDP lines.
#[derive(Debug, Clone)]
pub struct RawLineIter<'s, 'a> {
    lines: &'s [RawLine<'a>],
    indices: slice::Iter<'s, usize>,
}

/// A raw SDP media description plus indexed media-level lines.
///
/// The `m=` line is kept separate from the rest of the media section so later
/// protocol-specific layers can decide how much structure to impose.
#[derive(Debug, Clone)]
pub struct RawMediaDescription<'a> {
    description: RawLine<'a>,
    media_token: Option<&'a str>,
    section: RawSection<'a>,
}

/// Iterator over indexed media sections.
#[derive(Debug, Clone)]
pub struct RawMediaDescriptionIter<'s, 'a> {
    media_sections: &'s [RawMediaDescription<'a>],
    indices: slice::Iter<'s, usize>,
}

/// Session-level raw SDP plus grouped media-section lookups.
///
/// Media sections remain isolated instead of being flattened into a single map.
/// This keeps raw SDP neutral and avoids collisions for section-local namespaces
/// such as payload types or repeated attribute keys.
#[derive(Debug, Clone)]
pub struct RawSession<'a> {
    session: RawSection<'a>,
    media_sections: Vec<RawMediaDescription<'a>>,
    media_lookup: RawMediaSectionLookup<'a>,
}

#[derive(Debug, Clone, Default)]
struct RawSectionLookup<'a> {
    by_kind: HashMap<u8, Vec<usize>>,
    by_key: HashMap<u8, HashMap<&'a str, Vec<usize>>>,
}

#[derive(Debug, Clone, Default)]
struct RawMediaSectionLookup<'a> {
    by_media: HashMap<&'a str, Vec<usize>>,
    by_kind: HashMap<u8, Vec<usize>>,
    by_key: HashMap<u8, HashMap<&'a str, Vec<usize>>>,
}

#[derive(Debug, Default)]
struct RawSessionBuilder<'a> {
    session: Vec<RawLine<'a>>,
    media_sections: Vec<RawMediaDescriptionBuilder<'a>>,
}

#[derive(Debug)]
struct RawMediaDescriptionBuilder<'a> {
    description: RawLine<'a>,
    lines: Vec<RawLine<'a>>,
}

const fn uses_keyed_lookup(kind: u8) -> bool {
    matches!(kind, b'a' | b'b' | b'k')
}

impl<'a> RawLine<'a> {
    fn parse(
        line: &'a str,
        line_number: u32,
        collector: &mut Collector<'a>,
    ) -> Result<Self, SdpIssue<'a>> {
        if line.is_empty() {
            return Err(SdpIssue {
                kind: SdpIssueKind::EmptyLine,
                location: Some(SdpLocation::InputLine { line_number }),
            });
        }

        let Some((kind, value)) = line.split_once('=') else {
            return Err(SdpIssue {
                kind: SdpIssueKind::MalformedLine { line },
                location: Some(SdpLocation::InputLine { line_number }),
            });
        };

        if kind.is_empty() {
            return Err(SdpIssue {
                kind: SdpIssueKind::MalformedLine { line },
                location: Some(SdpLocation::InputLine { line_number }),
            });
        }

        let kind = if kind.len() > 1 {
            let trimmed = kind.trim_ascii_end();
            if trimmed.len() == 1 {
                if matches!(collector.mode, HandlingMode::Strict) {
                    return Err(SdpIssue {
                        kind: SdpIssueKind::TrailingSpaceInType {
                            count: kind.len() - 1,
                        },
                        location: Some(SdpLocation::InputLine { line_number }),
                    });
                }
                collector.push_diagnostic(Diagnostic::Warning(SdpIssue {
                    kind: SdpIssueKind::TrailingSpaceInType {
                        count: kind.len() - 1,
                    },
                    location: Some(SdpLocation::InputLine { line_number }),
                }))?;
                trimmed.as_bytes()[0]
            } else {
                return Err(SdpIssue {
                    kind: SdpIssueKind::MalformedLine { line },
                    location: Some(SdpLocation::InputLine { line_number }),
                });
            }
        } else {
            kind.as_bytes()[0]
        };

        Ok(Self { kind, value })
    }

    /// Returns the raw key/value split for keyed line kinds (`a=`, `b=`, `k=`).
    pub fn split_key_value(&self) -> Option<RawKeyValue<'a>> {
        if !uses_keyed_lookup(self.kind) {
            return None;
        }

        let (key, value) = self
            .value
            .split_once(':')
            .map_or((self.value, None), |(key, value)| (key, Some(value)));

        Some(RawKeyValue { key, value })
    }

    /// Returns the raw key for keyed line kinds (`a=`, `b=`, `k=`).
    pub fn key(&self) -> Option<&'a str> {
        self.split_key_value().map(|keyed| keyed.key)
    }
}

impl Display for RawLine<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}={}", char::from(self.kind), self.value)
    }
}

impl<'a> RawSectionLookup<'a> {
    fn new(lines: &[RawLine<'a>]) -> Self {
        let mut lookup = Self::default();

        for (index, line) in lines.iter().enumerate() {
            lookup.by_kind.entry(line.kind).or_default().push(index);

            if let Some(key) = line.key() {
                lookup
                    .by_key
                    .entry(line.kind)
                    .or_default()
                    .entry(key)
                    .or_default()
                    .push(index);
            }
        }

        lookup
    }

    fn kind_indices(&self, kind: u8) -> &[usize] {
        self.by_kind.get(&kind).map_or(&[], Vec::as_slice)
    }

    fn key_indices(&self, kind: u8, key: &str) -> &[usize] {
        self.by_key
            .get(&kind)
            .and_then(|keys| keys.get(key))
            .map_or(&[], Vec::as_slice)
    }
}

impl<'a> RawSection<'a> {
    fn new(lines: Vec<RawLine<'a>>) -> Self {
        let lookup = RawSectionLookup::new(&lines);

        Self { lines, lookup }
    }

    pub fn as_slice(&self) -> &[RawLine<'a>] {
        &self.lines
    }

    pub fn iter(&self) -> slice::Iter<'_, RawLine<'a>> {
        self.lines.iter()
    }

    pub const fn len(&self) -> usize {
        self.lines.len()
    }

    pub const fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    pub fn contains_kind(&self, kind: u8) -> bool {
        self.lookup.by_kind.contains_key(&kind)
    }

    pub fn first_of_kind(&self, kind: u8) -> Option<&RawLine<'a>> {
        self.lines_by_kind(kind).next()
    }

    pub fn lines_by_kind(&self, kind: u8) -> RawLineIter<'_, 'a> {
        RawLineIter::new(&self.lines, self.lookup.kind_indices(kind))
    }

    pub fn contains_key(&self, kind: u8, key: &str) -> bool {
        !self.lookup.key_indices(kind, key).is_empty()
    }

    pub fn first_by_key(&self, kind: u8, key: &str) -> Option<&RawLine<'a>> {
        self.lines_by_key(kind, key).next()
    }

    pub fn lines_by_key(&self, kind: u8, key: &str) -> RawLineIter<'_, 'a> {
        RawLineIter::new(&self.lines, self.lookup.key_indices(kind, key))
    }

    pub fn attribute(&self, key: &str) -> Option<&RawLine<'a>> {
        self.first_by_key(b'a', key)
    }

    pub fn attributes(&self, key: &str) -> RawLineIter<'_, 'a> {
        self.lines_by_key(b'a', key)
    }
}

impl Display for RawSection<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for line in &self.lines {
            write!(f, "{line}\r\n")?;
        }
        Ok(())
    }
}

impl PartialEq for RawSection<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.lines == other.lines
    }
}

impl Eq for RawSection<'_> {}

impl Hash for RawSection<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.lines.hash(state);
    }
}

impl<'s, 'a> RawLineIter<'s, 'a> {
    fn new(lines: &'s [RawLine<'a>], indices: &'s [usize]) -> Self {
        Self {
            lines,
            indices: indices.iter(),
        }
    }
}

impl<'s, 'a> Iterator for RawLineIter<'s, 'a> {
    type Item = &'s RawLine<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        self.indices.next().map(|&index| &self.lines[index])
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.indices.size_hint()
    }
}

impl<'s, 'a> DoubleEndedIterator for RawLineIter<'s, 'a> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.indices.next_back().map(|&index| &self.lines[index])
    }
}

impl ExactSizeIterator for RawLineIter<'_, '_> {}

impl FusedIterator for RawLineIter<'_, '_> {}

impl<'a> RawMediaDescription<'a> {
    fn from_parts(description: RawLine<'a>, lines: Vec<RawLine<'a>>) -> Self {
        Self {
            description,
            media_token: description.value.split_ascii_whitespace().next(),
            section: RawSection::new(lines),
        }
    }

    pub const fn description(&self) -> &RawLine<'a> {
        &self.description
    }

    /// Returns the raw `<media>` token from the `m=` line when present.
    pub const fn media_token(&self) -> Option<&'a str> {
        self.media_token
    }

    pub const fn section(&self) -> &RawSection<'a> {
        &self.section
    }

    pub fn lines(&self) -> &[RawLine<'a>] {
        self.section.as_slice()
    }

    pub fn iter(&self) -> slice::Iter<'_, RawLine<'a>> {
        self.section.iter()
    }

    pub const fn len(&self) -> usize {
        self.section.len()
    }

    pub const fn is_empty(&self) -> bool {
        self.section.is_empty()
    }

    pub fn contains_kind(&self, kind: u8) -> bool {
        self.section.contains_kind(kind)
    }

    pub fn first_of_kind(&self, kind: u8) -> Option<&RawLine<'a>> {
        self.section.first_of_kind(kind)
    }

    pub fn lines_by_kind(&self, kind: u8) -> RawLineIter<'_, 'a> {
        self.section.lines_by_kind(kind)
    }

    pub fn contains_key(&self, kind: u8, key: &str) -> bool {
        self.section.contains_key(kind, key)
    }

    pub fn first_by_key(&self, kind: u8, key: &str) -> Option<&RawLine<'a>> {
        self.section.first_by_key(kind, key)
    }

    pub fn lines_by_key(&self, kind: u8, key: &str) -> RawLineIter<'_, 'a> {
        self.section.lines_by_key(kind, key)
    }

    pub fn attribute(&self, key: &str) -> Option<&RawLine<'a>> {
        self.section.attribute(key)
    }

    pub fn attributes(&self, key: &str) -> RawLineIter<'_, 'a> {
        self.section.attributes(key)
    }
}

impl Display for RawMediaDescription<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}\r\n", self.description)?;
        write!(f, "{}", self.section)
    }
}

impl PartialEq for RawMediaDescription<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.description == other.description && self.section == other.section
    }
}

impl Eq for RawMediaDescription<'_> {}

impl Hash for RawMediaDescription<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.description.hash(state);
        self.section.hash(state);
    }
}

impl<'a> RawMediaSectionLookup<'a> {
    fn new(media_sections: &[RawMediaDescription<'a>]) -> Self {
        let mut lookup = Self::default();

        for (section_index, media_section) in media_sections.iter().enumerate() {
            if let Some(media_token) = media_section.media_token() {
                lookup
                    .by_media
                    .entry(media_token)
                    .or_default()
                    .push(section_index);
            }

            let mut seen_kinds = HashSet::new();
            let mut seen_keys = HashSet::new();

            for line in media_section.iter() {
                if seen_kinds.insert(line.kind) {
                    lookup
                        .by_kind
                        .entry(line.kind)
                        .or_default()
                        .push(section_index);
                }

                if let Some(key) = line.key()
                    && seen_keys.insert((line.kind, key))
                {
                    lookup
                        .by_key
                        .entry(line.kind)
                        .or_default()
                        .entry(key)
                        .or_default()
                        .push(section_index);
                }
            }
        }

        lookup
    }

    fn media_indices(&self, media: &str) -> &[usize] {
        self.by_media.get(media).map_or(&[], Vec::as_slice)
    }

    fn kind_indices(&self, kind: u8) -> &[usize] {
        self.by_kind.get(&kind).map_or(&[], Vec::as_slice)
    }

    fn key_indices(&self, kind: u8, key: &str) -> &[usize] {
        self.by_key
            .get(&kind)
            .and_then(|keys| keys.get(key))
            .map_or(&[], Vec::as_slice)
    }
}

impl<'s, 'a> RawMediaDescriptionIter<'s, 'a> {
    fn new(media_sections: &'s [RawMediaDescription<'a>], indices: &'s [usize]) -> Self {
        Self {
            media_sections,
            indices: indices.iter(),
        }
    }
}

impl<'s, 'a> Iterator for RawMediaDescriptionIter<'s, 'a> {
    type Item = &'s RawMediaDescription<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        self.indices
            .next()
            .map(|&index| &self.media_sections[index])
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.indices.size_hint()
    }
}

impl<'s, 'a> DoubleEndedIterator for RawMediaDescriptionIter<'s, 'a> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.indices
            .next_back()
            .map(|&index| &self.media_sections[index])
    }
}

impl ExactSizeIterator for RawMediaDescriptionIter<'_, '_> {}

impl FusedIterator for RawMediaDescriptionIter<'_, '_> {}

impl<'a> RawSession<'a> {
    fn from_parts(
        session: Vec<RawLine<'a>>,
        media_sections: Vec<RawMediaDescriptionBuilder<'a>>,
    ) -> Self {
        let media_sections = media_sections
            .into_iter()
            .map(|media| RawMediaDescription::from_parts(media.description, media.lines))
            .collect::<Vec<_>>();
        let media_lookup = RawMediaSectionLookup::new(&media_sections);

        Self {
            session: RawSection::new(session),
            media_sections,
            media_lookup,
        }
    }

    pub fn parse_document(
        sdp: &'a str,
        collector: &mut Collector<'a>,
    ) -> Result<Self, SdpIssue<'a>> {
        let mut parser = RawSessionParser {
            new: RawSessionBuilder::default(),
            media_section_idx: 0,
            is_skipping_media: false,
        };

        let mut expect_crlf: Option<bool> = None;

        for (i, line) in sdp.split_inclusive('\n').enumerate() {
            let (line, is_crlf) = line.strip_suffix("\r\n").map_or_else(
                || (line.strip_suffix('\n').unwrap_or(line), false),
                |stripped| (stripped, true),
            );

            let eol_mismatch = expect_crlf.replace(is_crlf).unwrap_or(is_crlf) != is_crlf;

            if eol_mismatch {
                match collector.mode {
                    HandlingMode::Strict => {
                        return Err(SdpIssue {
                            kind: SdpIssueKind::MixedLineEndings,
                            location: Some(SdpLocation::InputLine {
                                line_number: i as u32,
                            }),
                        });
                    }
                    HandlingMode::BestEffort(_) | HandlingMode::Recover(_) => {
                        collector.push_diagnostic(Diagnostic::Warning(SdpIssue {
                            kind: SdpIssueKind::MixedLineEndings,
                            location: Some(SdpLocation::InputLine {
                                line_number: i as u32,
                            }),
                        }))?;
                    }
                }
            }
            parser.feed_line(line, i as u32, collector)?;
        }

        Ok(parser.finish())
    }

    pub const fn session(&self) -> &RawSection<'a> {
        &self.session
    }

    pub fn media_sections(&self) -> &[RawMediaDescription<'a>] {
        &self.media_sections
    }

    pub fn media_sections_by_media_token(&self, media: &str) -> RawMediaDescriptionIter<'_, 'a> {
        RawMediaDescriptionIter::new(&self.media_sections, self.media_lookup.media_indices(media))
    }

    pub fn media_sections_with_kind(&self, kind: u8) -> RawMediaDescriptionIter<'_, 'a> {
        RawMediaDescriptionIter::new(&self.media_sections, self.media_lookup.kind_indices(kind))
    }

    pub fn media_sections_with_key(&self, kind: u8, key: &str) -> RawMediaDescriptionIter<'_, 'a> {
        RawMediaDescriptionIter::new(
            &self.media_sections,
            self.media_lookup.key_indices(kind, key),
        )
    }

    pub fn media_sections_with_attribute(&self, key: &str) -> RawMediaDescriptionIter<'_, 'a> {
        self.media_sections_with_key(b'a', key)
    }
}

impl Display for RawSession<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", &self.session)?;
        for section in &self.media_sections {
            write!(f, "{section}")?;
        }
        Ok(())
    }
}

impl PartialEq for RawSession<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.session == other.session && self.media_sections == other.media_sections
    }
}

impl Eq for RawSession<'_> {}

impl Hash for RawSession<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.session.hash(state);
        self.media_sections.hash(state);
    }
}

struct RawSessionParser<'a> {
    new: RawSessionBuilder<'a>,
    media_section_idx: u32,
    is_skipping_media: bool,
}

impl<'a> RawSessionParser<'a> {
    fn finish(self) -> RawSession<'a> {
        RawSession::from_parts(self.new.session, self.new.media_sections)
    }

    fn feed_line(
        &mut self,
        line: &'a str,
        line_number: u32,
        collector: &mut Collector<'a>,
    ) -> Result<(), SdpIssue<'a>> {
        let line = RawLine::parse(line, line_number, collector);
        match (line, collector.mode) {
            (Ok(l), _) => {
                if l.kind == b'm' {
                    self.media_section_idx += 1;
                    self.is_skipping_media = false;
                    self.new.media_sections.push(RawMediaDescriptionBuilder {
                        description: l,
                        lines: Vec::new(),
                    });
                } else if let Some(media) = self.new.media_sections.last_mut() {
                    if !self.is_skipping_media {
                        media.lines.push(l);
                    }
                } else if !self.is_skipping_media {
                    self.new.session.push(l);
                }
                Ok(())
            }
            (Err(e), HandlingMode::Strict) => Err(e),
            (Err(e), _) if matches!(e.kind, SdpIssueKind::DiagnosticLimitExceeded { .. }) => Err(e),
            (Err(e), _) => {
                if self.is_skipping_media {
                    return Ok(());
                }
                if self.new.media_sections.is_empty() {
                    Err(e)
                } else {
                    self.new.media_sections.pop();
                    self.is_skipping_media = true;
                    collector.push_diagnostic(Diagnostic::Error(e))?;
                    collector.push_diagnostic(Diagnostic::Warning(SdpIssue {
                        kind: SdpIssueKind::SkippedMediaSection,
                        location: Some(SdpLocation::MediaSection {
                            index: self.media_section_idx - 1,
                            line_number: Some(line_number),
                        }),
                    }))?;
                    Ok(())
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        error::{
            Collector, HandlingMode, HandlingOptions, SdpIssue, SdpIssueKind, SdpLocation,
            SdpOptions,
        },
        raw::{RawKeyValue, RawLine, RawSession},
    };

    #[test]
    fn strict_document() {
        const SDP: &str = "v=0\r\no=jdoe 3724394400 3724394405 IN IP4 198.51.100.1\r\ns=Call to John Smith\r\ni=SDP Offer #1\r\nu=http://www.jdoe.example.com/home.html\r\ne=Jane Doe <jane@jdoe.example.com>\r\np=+1 617 555-6011\r\nc=IN IP4 198.51.100.1\r\nt=0 0\r\nm=audio 49170 RTP/AVP 0\r\nm=audio 49180 RTP/AVP 0\r\nm=video 51372 RTP/AVP 99\r\nc=IN IP6 2001:db8::2\r\na=rtpmap:99 h263-1998/90000\r\n";
        let mut collector = Collector::new(HandlingMode::Strict);
        let Ok(sess) = RawSession::parse_document(SDP, &mut collector) else {
            panic!("Failed to parse session: {:?}", collector.items);
        };

        assert!(collector.items.is_empty());

        assert_eq!(sess.session().len(), 9);
        assert_eq!(sess.media_sections().len(), 3);
        assert_eq!(sess.media_sections()[0].lines().len(), 0);
        assert_eq!(sess.media_sections()[1].lines().len(), 0);
        assert_eq!(sess.media_sections()[2].lines().len(), 2);
    }

    #[test]
    fn strict_lf() {
        const SDP: &str = "v=0\ns=Test SDP\ni=SDP Offer\n";

        let mut collector = Collector::new(HandlingMode::Strict);
        let sess = RawSession::parse_document(SDP, &mut collector).unwrap();

        assert_eq!(
            sess.session().as_slice()[0],
            RawLine::parse("v=0", 0, &mut collector).unwrap()
        );
        assert_eq!(
            sess.session().as_slice()[1],
            RawLine::parse("s=Test SDP", 1, &mut collector).unwrap()
        );
        assert_eq!(
            sess.session().as_slice()[2],
            RawLine::parse("i=SDP Offer", 2, &mut collector).unwrap()
        );
    }

    #[test]
    fn strict_eol() {
        const SDP: &str = "v=0\r\ns=Test SDP\ni=SDP Offer\r\n";

        let mut collector = Collector::new(HandlingMode::Strict);

        assert_eq!(
            RawSession::parse_document(SDP, &mut collector).unwrap_err(),
            SdpIssue {
                kind: SdpIssueKind::MixedLineEndings,
                location: Some(SdpLocation::InputLine { line_number: 1 })
            }
        );
    }

    #[test]
    fn lenient_eol() {
        const SDP: &str = "v=0\r\ns=Test SDP\ni=SDP Offer\r\n";

        let mut collector = Collector::new(HandlingMode::BestEffort(HandlingOptions {
            max_diagnostics: None,
        }));
        let Ok(sess) = RawSession::parse_document(SDP, &mut collector) else {
            panic!("Failed to parse session: {:?}", collector.items);
        };

        assert!(collector.items.iter().all(|i| i.is_warning()));
        assert_eq!(
            collector.items[0].issue(),
            &SdpIssue {
                kind: SdpIssueKind::MixedLineEndings,
                location: Some(SdpLocation::InputLine { line_number: 1 })
            }
        );
        assert_eq!(
            collector.items[1].issue(),
            &SdpIssue {
                kind: SdpIssueKind::MixedLineEndings,
                location: Some(SdpLocation::InputLine { line_number: 2 })
            }
        );

        assert_eq!(
            sess.session().as_slice()[0],
            RawLine::parse("v=0", 0, &mut collector).unwrap()
        );
        assert_eq!(
            sess.session().as_slice()[1],
            RawLine::parse("s=Test SDP", 1, &mut collector).unwrap()
        );
        assert_eq!(
            sess.session().as_slice()[2],
            RawLine::parse("i=SDP Offer", 2, &mut collector).unwrap()
        );
    }

    #[test]
    fn skip_media() {
        const SDP: &str = "v=0\r\ns=Test SDP\r\nm=video 12345 RTP/AVP 0\r\ninvalid_line\r\nm=audio 12347 RTP/AVP 1\r\nc=IN IP4 10.0.0.1\r\n";

        let test_skip = |mode| {
            let mut collector = Collector::new(mode);
            let sess = RawSession::parse_document(SDP, &mut collector).unwrap();

            assert_eq!(sess.media_sections().len(), 1);
            assert!(collector.items[0].is_error());
            assert_eq!(
                collector.items[0].issue(),
                &SdpIssue {
                    kind: SdpIssueKind::MalformedLine {
                        line: "invalid_line"
                    },
                    location: Some(SdpLocation::InputLine { line_number: 3 })
                }
            );
            assert!(collector.items[1].is_warning());
            assert_eq!(
                collector.items[1].issue(),
                &SdpIssue {
                    kind: SdpIssueKind::SkippedMediaSection,
                    location: Some(SdpLocation::MediaSection {
                        index: 0,
                        line_number: Some(3)
                    })
                }
            );
        };

        test_skip(HandlingMode::BestEffort(HandlingOptions {
            max_diagnostics: None,
        }));
        test_skip(HandlingMode::Recover(HandlingOptions {
            max_diagnostics: None,
        }));
    }

    #[test]
    fn strict_raw_line() {
        let mut collector = Collector::new(HandlingMode::Strict);
        assert_eq!(
            RawLine::parse("a=123", 0, &mut collector),
            Ok(RawLine {
                kind: b'a',
                value: "123"
            })
        );

        assert!(collector.items.is_empty());
    }

    #[test]
    fn strict_trailing_space_type() {
        let mut collector = Collector::new(HandlingMode::Strict);
        assert_eq!(
            RawLine::parse("a =123", 0, &mut collector)
                .unwrap_err()
                .kind,
            SdpIssueKind::TrailingSpaceInType { count: 1 }
        );

        let mut collector = Collector::new(HandlingMode::Strict);
        assert_eq!(
            RawLine::parse("a     =123", 0, &mut collector)
                .unwrap_err()
                .kind,
            SdpIssueKind::TrailingSpaceInType { count: 5 }
        );
    }

    #[test]
    fn lenient_trailing_space_type() {
        let test_trailing =
            |line: &str, kind: u8, value: &str, space_count: usize, mode: HandlingMode| {
                let mut collector = Collector::new(mode);

                let line = RawLine::parse(line, 0, &mut collector);
                let Ok(line) = line else {
                    panic!("{line:?} could not be parsed");
                };

                assert_eq!(line.kind, kind);
                assert_eq!(line.value, value);

                assert!(collector.items[0].is_warning());
                assert_eq!(
                    collector.items[0].issue().kind,
                    SdpIssueKind::TrailingSpaceInType { count: space_count }
                );
            };

        for line in [("a =123", 1), ("a     =123", 5)] {
            for mode in [
                HandlingMode::Recover(HandlingOptions {
                    max_diagnostics: None,
                }),
                HandlingMode::BestEffort(HandlingOptions {
                    max_diagnostics: None,
                }),
            ] {
                test_trailing(line.0, b'a', "123", line.1, mode);
            }
        }
    }

    #[test]
    fn multibyte_type() {
        let test_multibyte = |line: &str, mode: HandlingMode| {
            let mut collector = Collector::new(mode);
            assert_eq!(
                RawLine::parse(line, 0, &mut collector).unwrap_err().kind,
                SdpIssueKind::MalformedLine { line },
            );
        };

        for line in ["am=123", "bigtype=abcdef"] {
            for mode in [
                HandlingMode::Strict,
                HandlingMode::Recover(HandlingOptions {
                    max_diagnostics: None,
                }),
                HandlingMode::BestEffort(HandlingOptions {
                    max_diagnostics: None,
                }),
            ] {
                test_multibyte(line, mode);
            }
        }
    }

    #[test]
    fn keyed_line_split() {
        let mut collector = Collector::new(HandlingMode::Strict);

        let property = RawLine::parse("a=recvonly", 0, &mut collector).unwrap();
        assert_eq!(
            property.split_key_value(),
            Some(RawKeyValue {
                key: "recvonly",
                value: None,
            })
        );

        let value = RawLine::parse("a=rtpmap:96 opus/48000/2", 1, &mut collector).unwrap();
        assert_eq!(
            value.split_key_value(),
            Some(RawKeyValue {
                key: "rtpmap",
                value: Some("96 opus/48000/2"),
            })
        );

        let bandwidth = RawLine::parse("b=AS:128", 2, &mut collector).unwrap();
        assert_eq!(
            bandwidth.split_key_value(),
            Some(RawKeyValue {
                key: "AS",
                value: Some("128"),
            })
        );

        let media = RawLine::parse("m=audio 9 RTP/AVP 0", 3, &mut collector).unwrap();
        assert_eq!(media.split_key_value(), None);
    }

    #[test]
    fn raw_lookups_keep_media_sections_isolated() {
        const SDP: &str = "v=0\r\ns=Lookup Test\r\na=group:BUNDLE 0 1\r\nm=audio 9 RTP/AVP 0 96\r\na=mid:0\r\na=rtpmap:96 opus/48000/2\r\na=fmtp:96 minptime=10;useinbandfec=1\r\nm=video 9 RTP/AVP 97\r\na=mid:1\r\na=rtpmap:97 H264/90000\r\na=rtcp-fb:97 nack pli\r\n";

        let mut collector = Collector::new(HandlingMode::Strict);
        let sess = RawSession::parse_document(SDP, &mut collector).unwrap();

        assert!(collector.items.is_empty());

        let session = sess.session();
        assert_eq!(
            session.attribute("group").unwrap().split_key_value(),
            Some(RawKeyValue {
                key: "group",
                value: Some("BUNDLE 0 1"),
            })
        );
        assert_eq!(session.lines_by_kind(b'a').count(), 1);
        assert!(session.contains_key(b'a', "group"));

        assert_eq!(sess.media_sections_by_media_token("audio").count(), 1);
        assert_eq!(sess.media_sections_by_media_token("video").count(), 1);
        assert_eq!(sess.media_sections_with_attribute("mid").count(), 2);
        assert_eq!(sess.media_sections_with_attribute("rtpmap").count(), 2);
        assert_eq!(sess.media_sections_with_key(b'a', "rtcp-fb").count(), 1);
        assert_eq!(sess.media_sections_with_kind(b'a').count(), 2);

        let audio = sess.media_sections_by_media_token("audio").next().unwrap();
        assert_eq!(audio.media_token(), Some("audio"));
        assert_eq!(
            audio.attribute("mid").unwrap().split_key_value(),
            Some(RawKeyValue {
                key: "mid",
                value: Some("0"),
            })
        );
        assert_eq!(audio.attributes("rtpmap").count(), 1);
        assert_eq!(
            audio.attributes("rtpmap").next().unwrap().split_key_value(),
            Some(RawKeyValue {
                key: "rtpmap",
                value: Some("96 opus/48000/2"),
            })
        );
        assert!(audio.attribute("rtcp-fb").is_none());

        let video = sess.media_sections_by_media_token("video").next().unwrap();
        assert_eq!(video.media_token(), Some("video"));
        assert_eq!(video.attributes("rtpmap").count(), 1);
        assert_eq!(
            video.attribute("rtcp-fb").unwrap().split_key_value(),
            Some(RawKeyValue {
                key: "rtcp-fb",
                value: Some("97 nack pli"),
            })
        );
    }

    #[test]
    fn write_roundtrip_line() {
        let rt = |l| {
            let parsed = RawLine::parse(l, 0, &mut Collector::new(HandlingMode::Strict)).unwrap();
            assert_eq!(
                parsed,
                RawLine::parse(
                    &format!("{parsed}"),
                    0,
                    &mut Collector::new(HandlingMode::Strict)
                )
                .unwrap()
            );
        };

        rt("v=0");
        rt("m=video 9 RTP/AVP 0 96");
        rt("a=cryptex");
        rt("b=AS:1000");
        rt("c=IN IP4 127.0.0.1");
    }

    #[test]
    fn write_roundtrip_skipped_media() {
        const SDP: &str = "v=0\r\ns=Test SDP\r\nm=video 12345 RTP/AVP 0\r\ninvalid_line\r\nm=audio 12347 RTP/AVP 1\r\nc=IN IP4 10.0.0.1\r\n";

        let parse = |sdp| {
            RawSession::parse_document(sdp, &mut Collector::new(SdpOptions::recover(None).mode))
                .unwrap()
        };
        let parsed = parse(SDP);
        assert_eq!(parsed, parse(&format!("{parsed}")));
    }

    #[test]
    fn write_roundtrip_doc() {
        const SDP: &str = "v=0\r\no=jdoe 3724394400 3724394405 IN IP4 198.51.100.1\r\ns=Call to John Smith\r\ni=SDP Offer #1\r\nu=http://www.jdoe.example.com/home.html\r\ne=Jane Doe <jane@jdoe.example.com>\r\np=+1 617 555-6011\r\nc=IN IP4 198.51.100.1\r\nt=0 0\r\nm=audio 49170 RTP/AVP 0\r\nm=audio 49180 RTP/AVP 0\r\nm=video 51372 RTP/AVP 99\r\nc=IN IP6 2001:db8::2\r\na=rtpmap:99 h263-1998/90000\r\n";

        let parse = |sdp| {
            RawSession::parse_document(sdp, &mut Collector::new(HandlingMode::Strict)).unwrap()
        };
        let parsed = parse(SDP);
        assert_eq!(parsed, parse(&format!("{parsed}")));
    }
}
