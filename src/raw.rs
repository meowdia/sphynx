// SPDX-FileCopyrightText: 2026 Meowdia Community
// SPDX-License-Identifier: MIT OR Apache-2.0

use crate::error::{Collector, Diagnostic, HandlingMode, SdpIssue, SdpIssueKind, SdpLocation};

/// An unparsed SDP line, with only its type
/// [RFC8866-5](https://datatracker.ietf.org/doc/html/rfc8866#section-5)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RawLine<'a> {
    /// ASCII character for line type: b'm', b'c', b'a', etc.
    pub kind: u8,
    pub value: &'a str,
}

/// [RFC8866-5.14](https://datatracker.ietf.org/doc/html/rfc8866#section-5.14)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RawMediaDescription<'a> {
    /// the m line for this media section
    pub description: RawLine<'a>,
    pub lines: Vec<RawLine<'a>>,
}

/// > The session-level section starts with a "v=" line and continues to the first
/// > media description (or the end of the whole description, whichever comes first).
///
/// [RFC8866-5](https://datatracker.ietf.org/doc/html/rfc8866#section-5)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RawSession<'a> {
    pub session: Vec<RawLine<'a>>,
    pub media_sections: Vec<RawMediaDescription<'a>>,
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
}

impl<'a> RawSession<'a> {
    pub fn parse_document(
        sdp: &'a str,
        collector: &mut Collector<'a>,
    ) -> Result<Self, SdpIssue<'a>> {
        let mut parser = RawSessionParser {
            new: Self {
                session: Vec::new(),
                media_sections: Vec::new(),
            },
            media_section_idx: 0,
            is_skipping_media: false,
        };

        for (i, line) in sdp.split_inclusive("\n").enumerate() {
            let (line, is_crlf) = if let Some(stripped) = line.strip_suffix("\r\n") {
                (stripped, true)
            } else {
                (&line[..(line.len() - 1)], false)
            };

            if !is_crlf {
                match collector.mode {
                    HandlingMode::Strict => {
                        return Err(SdpIssue {
                            kind: SdpIssueKind::NonCanonicalLineEnding,
                            location: Some(SdpLocation::InputLine {
                                line_number: i as u32,
                            }),
                        });
                    }
                    HandlingMode::BestEffort(_) | HandlingMode::Recover(_) => {
                        collector.push_diagnostic(Diagnostic::Warning(SdpIssue {
                            kind: SdpIssueKind::NonCanonicalLineEnding,
                            location: Some(SdpLocation::InputLine {
                                line_number: i as u32,
                            }),
                        }))?;
                    }
                }
            }
            parser.feed_line(line, i as u32, collector)?;
        }

        Ok(parser.new)
    }
}

struct RawSessionParser<'a> {
    new: RawSession<'a>,
    media_section_idx: u32,
    is_skipping_media: bool,
}

impl<'a> RawSessionParser<'a> {
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
                    self.new.media_sections.push(RawMediaDescription {
                        description: l,
                        lines: Vec::new(),
                    });
                } else if let Some(media) = self.new.media_sections.last_mut() {
                    if !self.is_skipping_media {
                        media.lines.push(l);
                    }
                } else {
                    if !self.is_skipping_media {
                        self.new.session.push(l);
                    }
                };
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
        error::{Collector, HandlingMode, HandlingOptions, SdpIssue, SdpIssueKind},
        raw::{RawLine, RawSession},
    };

    #[test]
    fn strict_document() {
        const SDP: &str = "v=0\r\no=jdoe 3724394400 3724394405 IN IP4 198.51.100.1\r\ns=Call to John Smith\r\ni=SDP Offer #1\r\nu=http://www.jdoe.example.com/home.html\r\ne=Jane Doe <jane@jdoe.example.com>\r\np=+1 617 555-6011\r\nc=IN IP4 198.51.100.1\r\nt=0 0\r\nm=audio 49170 RTP/AVP 0\r\nm=audio 49180 RTP/AVP 0\r\nm=video 51372 RTP/AVP 99\r\nc=IN IP6 2001:db8::2\r\na=rtpmap:99 h263-1998/90000\r\n";
        let mut collector = Collector::new(HandlingMode::Strict);
        let Ok(sess) = RawSession::parse_document(SDP, &mut collector) else {
            panic!("Failed to parse session: {:?}", collector.items);
        };

        assert!(collector.items.is_empty());

        assert_eq!(sess.session.len(), 9);
        assert_eq!(sess.media_sections.len(), 3);
        assert_eq!(sess.media_sections[0].lines.len(), 0);
        assert_eq!(sess.media_sections[1].lines.len(), 0);
        assert_eq!(sess.media_sections[2].lines.len(), 2);
    }

    #[test]
    fn strict_lf() {
        const SDP: &str = "v=0\ns=Test SDP\ni=SDP Offer\n";

        let mut collector = Collector::new(HandlingMode::Strict);
        assert_eq!(
            RawSession::parse_document(SDP, &mut collector)
                .unwrap_err()
                .kind,
            SdpIssueKind::NonCanonicalLineEnding
        );
    }

    #[test]
    fn lenient_lf() {
        const SDP: &str = "v=0\ns=Test SDP\ni=SDP Offer\n";

        let mut collector = Collector::new(HandlingMode::BestEffort(HandlingOptions {
            max_diagnostics: None,
        }));
        let Ok(sess) = RawSession::parse_document(SDP, &mut collector) else {
            panic!("Failed to parse session: {:?}", collector.items);
        };

        for (i, issue) in collector.items.iter().map(|i| i.issue()).enumerate() {
            assert_eq!(issue.kind, SdpIssueKind::NonCanonicalLineEnding);
            assert_eq!(
                issue.location,
                Some(crate::error::SdpLocation::InputLine {
                    line_number: i as u32
                })
            );
        }

        assert_eq!(
            sess.session[0],
            RawLine::parse("v=0", 0, &mut collector).unwrap()
        );
        assert_eq!(
            sess.session[1],
            RawLine::parse("s=Test SDP", 1, &mut collector).unwrap()
        );
        assert_eq!(
            sess.session[2],
            RawLine::parse("i=SDP Offer", 2, &mut collector).unwrap()
        );
    }

    #[test]
    fn skip_media() {
        const SDP: &str = "v=0\r\ns=Test SDP\r\nm=video 12345 RTP/AVP 0\r\ninvalid_line\r\nm=audio 12347 RTP/AVP 1\r\nc=IN IP4 10.0.0.1\r\n";

        let test_skip = |mode| {
            let mut collector = Collector::new(mode);
            let sess = RawSession::parse_document(SDP, &mut collector).unwrap();

            assert_eq!(sess.media_sections.len(), 1);
            assert!(collector.items[0].is_error());
            assert_eq!(
                collector.items[0].issue(),
                &SdpIssue {
                    kind: SdpIssueKind::MalformedLine {
                        line: "invalid_line"
                    },
                    location: Some(crate::error::SdpLocation::InputLine { line_number: 3 })
                }
            );
            assert!(collector.items[1].is_warning());
            assert_eq!(
                collector.items[1].issue(),
                &SdpIssue {
                    kind: SdpIssueKind::SkippedMediaSection,
                    location: Some(crate::error::SdpLocation::MediaSection {
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
}
