// SPDX-FileCopyrightText: 2026 Meowdia Community
// SPDX-License-Identifier: MIT OR Apache-2.0

use std::num::NonZeroUsize;

use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HandlingMode {
    /// Fully RFC-compliant parsing mode, rejects all errors
    Strict,
    /// RFC-compliant parsing mode, may skip a malformed media section and restart
    /// on the following one
    Recover(HandlingOptions),
    /// Best-effort parsing mode, accepts malformed input and may
    /// skip media sections in the same fashion as [HandlingMode::Recover]
    BestEffort(HandlingOptions),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HandlingOptions {
    pub max_diagnostics: Option<NonZeroUsize>,
}

/// Configuration for SDP processing behavior and diagnostic collection.
///
/// Use [`SdpOptions::strict`] to stop at the first hard error.
/// Use [`SdpOptions::recover`] to recover when a safe point
/// is available.
/// Use [`SdpOptions::best_effort`] to prefer producing output even from
/// heavily malformed input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SdpOptions {
    pub(crate) mode: HandlingMode,
}

impl SdpOptions {
    /// Construct options that stop at the first hard error.
    pub const fn strict() -> Self {
        Self {
            mode: HandlingMode::Strict,
        }
    }

    /// Construct options that recover when a safe synchronization point
    /// is available and stop after collecting `max_diagnostics`.
    pub fn recover(max_diagnostics: Option<NonZeroUsize>) -> Self {
        Self {
            mode: HandlingMode::Recover(HandlingOptions { max_diagnostics }),
        }
    }

    /// Construct options that prefer producing output even from heavily
    /// malformed input and stop after collecting `max_diagnostics`.
    pub fn best_effort(max_diagnostics: Option<NonZeroUsize>) -> Self {
        Self {
            mode: HandlingMode::BestEffort(HandlingOptions { max_diagnostics }),
        }
    }
}

/// Source location metadata for an SDP diagnostic or fatal SDP failure.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SdpLocation {
    /// The diagnostic applies to the session-level section.
    Session { line_number: Option<u32> },
    /// The diagnostic applies to a media section identified by parse-order index.
    MediaSection {
        index: u32,
        line_number: Option<u32>,
    },
    /// The diagnostic applies to a logical line identified by parse-order index.
    ///
    /// This index is based on parse order and may include empty lines.
    Line {
        index: u32,
        line_number: Option<u32>,
    },
    /// The diagnostic is tied to a source line, but not a more specific SDP structure.
    InputLine { line_number: u32 },
}

/// Detailed payload for an SDP issue.
///
/// These issues are produced while parsing, validating, decoding, encoding,
/// or otherwise processing SDP. Some can be downgraded to diagnostics in
/// recovery modes; others become fatal failures when the operation cannot
/// continue.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash, Error)]
pub enum SdpIssueKind<'a> {
    /// An unknown `a=` attribute was encountered.
    ///
    /// RFC 8866 requires unknown attributes to be ignored by SDP receivers;
    /// surfacing them as a diagnostic remains useful for callers. See
    /// [section 5](https://datatracker.ietf.org/doc/html/rfc8866#section-5)
    /// and
    /// [section 5.13](https://datatracker.ietf.org/doc/html/rfc8866#section-5.13).
    #[error("unknown SDP attribute `{name}`")]
    UnknownAttribute { name: &'a str },
    /// The line ending sequence was changed while parsing the document, i.e.:
    /// `LF` -> `CRLF` or `CRLF` -> `LF`
    #[error("SDP EOL sequence changed in the middle of parsing")]
    MixedLineEndings,
    /// An attribute was repeated.
    #[error("duplicate SDP attribute `{name}`")]
    DuplicateAttribute { name: &'a str },
    /// A known attribute name was parsed, but its value was invalid.
    #[error("invalid value `{value}` for SDP attribute `{name}`")]
    InvalidAttributeValue { name: &'a str, value: &'a str },
    /// A line used a type letter outside the SDP core set.
    ///
    /// The set of type letters is intentionally small in
    /// [RFC 8866 section 5](https://datatracker.ietf.org/doc/html/rfc8866#section-5).
    #[error("unknown SDP line type `{ty}`")]
    UnknownLineType { ty: char },
    /// A required field from the SDP session or media grammar was missing.
    #[error("missing required SDP field `{field}`")]
    MissingRequiredField { field: &'static str },
    /// A raw line was malformed.
    #[error("malformed SDP line")]
    MalformedLine { line: &'a str },
    /// A raw line was empty
    #[error("empty SDP line")]
    EmptyLine,
    /// 1 or more trailing spaces after the type of the SDP line
    #[error("{count} trailing spaces in sdp line")]
    TrailingSpaceInType { count: usize },
    /// A raw media description line was malformed.
    #[error("malformed SDP media description")]
    MalformedMediaDescription { value: &'a str },
    /// A media description failed to match the expected `m=`-anchored structure.
    ///
    /// See [RFC 8866 section 5.14](https://datatracker.ietf.org/doc/html/rfc8866#section-5.14).
    #[error("malformed SDP media section")]
    MalformedMediaSection,
    /// A malformed media section was skipped and processing resumed at a later boundary.
    ///
    /// Media sections are naturally delimited by `m=` lines in
    /// [RFC 8866 section 5](https://datatracker.ietf.org/doc/html/rfc8866#section-5)
    /// and
    /// [section 5.14](https://datatracker.ietf.org/doc/html/rfc8866#section-5.14).
    #[error("skipped malformed SDP media section")]
    SkippedMediaSection,
    /// Input ended before processing could complete a required construct.
    #[error("unexpected end of input")]
    UnexpectedEndOfInput,
    /// Processing aborted after collecting too many recoverable diagnostics.
    #[error("diagnostic limit exceeded ({max_diagnostics})")]
    DiagnosticLimitExceeded { max_diagnostics: usize },
    /// An `a=charset:` value was not recognized or not supported.
    ///
    /// See
    /// [RFC 8866 section 6.10](https://datatracker.ietf.org/doc/html/rfc8866#section-6.10).
    #[error("unsupported SDP charset `{charset}`")]
    UnsupportedCharset { charset: &'a str },
    /// Text affected by the selected charset could not be decoded.
    ///
    /// See
    /// [RFC 8866 section 6.10](https://datatracker.ietf.org/doc/html/rfc8866#section-6.10).
    #[error("failed to decode `{field}` using charset `{charset}`")]
    DecodingFailed {
        charset: &'a str,
        field: &'static str,
    },
    /// Text could not be encoded for SDP output using the selected charset.
    #[error("failed to encode `{field}` using charset `{charset}`")]
    EncodingFailed {
        charset: &'a str,
        field: &'static str,
    },
}

impl<'a> SdpIssueKind<'a> {
    /// Stable machine-readable code for this SDP issue kind.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::UnknownAttribute { .. } => "unknown_attribute",
            Self::MixedLineEndings => "mixed_line_ending",
            Self::DuplicateAttribute { .. } => "duplicate_attribute",
            Self::InvalidAttributeValue { .. } => "invalid_attribute_value",
            Self::UnknownLineType { .. } => "unknown_line_type",
            Self::MissingRequiredField { .. } => "missing_required_field",
            Self::MalformedLine { .. } => "malformed_line",
            Self::EmptyLine { .. } => "empty_line",
            Self::TrailingSpaceInType { .. } => "trailing_space_in_type",
            Self::MalformedMediaDescription { .. } => "malformed_media_description",
            Self::MalformedMediaSection => "malformed_media_section",
            Self::SkippedMediaSection => "skipped_media_section",
            Self::UnexpectedEndOfInput => "unexpected_end_of_input",
            Self::DiagnosticLimitExceeded { .. } => "diagnostic_limit_exceeded",
            Self::UnsupportedCharset { .. } => "unsupported_charset",
            Self::DecodingFailed { .. } => "decoding_failed",
            Self::EncodingFailed { .. } => "encoding_failed",
        }
    }
}

/// An SDP issue with optional location metadata.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash, Error)]
#[error("{kind}")]
pub struct SdpIssue<'a> {
    pub kind: SdpIssueKind<'a>,
    /// Location information for the issue.
    pub location: Option<SdpLocation>,
}

impl<'a> SdpIssue<'a> {
    pub const fn code(&self) -> &'static str {
        self.kind.code()
    }
}

/// A non-fatal issue encountered while parsing SDP
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Diagnostic<'a> {
    /// The input is usable, but it is suspicious or non-canonical.
    Warning(SdpIssue<'a>),
    /// The input is malformed, but recovery may still be possible.
    Error(SdpIssue<'a>),
}

impl<'a> Diagnostic<'a> {
    pub fn issue(&self) -> &SdpIssue<'a> {
        match self {
            Diagnostic::Warning(sdp_issue) | Diagnostic::Error(sdp_issue) => sdp_issue,
        }
    }

    pub fn is_warning(&self) -> bool {
        matches!(self, Diagnostic::Warning { .. })
    }

    pub fn is_error(&self) -> bool {
        matches!(self, Diagnostic::Error { .. })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Collector<'a> {
    pub(crate) items: Vec<Diagnostic<'a>>,
    pub(crate) mode: HandlingMode,
}

impl<'a> Collector<'a> {
    pub fn items(&self) -> &[Diagnostic<'a>] {
        &self.items
    }

    pub fn new(mode: HandlingMode) -> Self {
        Self {
            items: Vec::new(),
            mode,
        }
    }

    /// Pushes a new [Diagnostic] to this Collector, returns a `Result::Err` if
    /// the limit of diagnostics was reached
    pub fn push_diagnostic(&mut self, diagnostic: Diagnostic<'a>) -> Result<(), SdpIssue<'a>> {
        match self.mode {
            HandlingMode::BestEffort(opt) | HandlingMode::Recover(opt) => {
                self.items.push(diagnostic);
                if let Some(md) = opt.max_diagnostics
                    && self.items.len() >= md.into()
                {
                    return Err(SdpIssue {
                        kind: SdpIssueKind::DiagnosticLimitExceeded {
                            max_diagnostics: md.into(),
                        },
                        location: None,
                    });
                }
                Ok(())
            }
            HandlingMode::Strict => {
                self.items.push(diagnostic);
                Ok(())
            }
        }
    }
}

/// Collection of recoverable diagnostics emitted during SDP processing.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct Diagnostics<'a> {
    pub items: Vec<Diagnostic<'a>>,
}

/// A fatal SDP failure that prevented normal completion.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash, Error)]
#[error("{issue}")]
pub struct SdpFailure<'a> {
    /// The issue that could not be recovered from.
    pub issue: SdpIssue<'a>,
    /// Diagnostics collected before the failure was returned.
    pub diagnostics: Diagnostics<'a>,
}

impl<'a> SdpFailure<'a> {
    pub const fn code(&self) -> &'static str {
        self.issue.code()
    }
}

/// Successful SDP operation output plus any collected recoverable diagnostics.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SdpOutput<'a, T> {
    pub value: T,
    pub diagnostics: Diagnostics<'a>,
}

/// Result of an SDP operation that may collect recoverable diagnostics.
pub type SdpResult<'a, T> = Result<SdpOutput<'a, T>, SdpFailure<'a>>;
