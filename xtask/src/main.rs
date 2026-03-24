// SPDX-FileCopyrightText: 2026 Meowdia Community
// SPDX-License-Identifier: MIT OR Apache-2.0

use std::{
    collections::BTreeSet,
    env,
    fmt::Write as _,
    fs,
    io::Write as _,
    path::{Path, PathBuf},
    process::{Command, ExitCode, Stdio},
};

use roxmltree::{Document, Node};

const SNAPSHOT_URL: &str = "https://www.iana.org/assignments/sdp-parameters/sdp-parameters.xml";
const SNAPSHOT_PATH: &str = "iana/snapshots/sdp-parameters.xml";
const GENERATED_PATH: &str = "src/iana/generated.rs";
const GENERATED_LICENSE_LINE: &str = concat!("// SPDX-License-", "Identifier: MIT OR Apache-2.0");
const IANA_NAMESPACE: &str = "http://www.iana.org/assignments";
const GENERATE_COMMAND: &str = "cargo run -p xtask -- iana generate";

const TARGET_REGISTRIES: [TargetRegistry; 5] = [
    TargetRegistry {
        title: "media",
        enum_name: "KnownMediaType",
        summary: "Known media types from the SDP `media` registry.",
    },
    TargetRegistry {
        title: "proto",
        enum_name: "KnownTransportProtocol",
        summary: "Known transport protocols from the SDP `proto` registry.",
    },
    TargetRegistry {
        title: "bwtype",
        enum_name: "KnownBandwidthType",
        summary: "Known bandwidth modifiers from the SDP `bwtype` registry.",
    },
    TargetRegistry {
        title: "nettype",
        enum_name: "KnownNetworkType",
        summary: "Known network types from the SDP `nettype` registry.",
    },
    TargetRegistry {
        title: "addrtype",
        enum_name: "KnownAddressType",
        summary: "Known address types from the SDP `addrtype` registry.",
    },
];

#[derive(Debug, Clone, Copy)]
struct TargetRegistry {
    title: &'static str,
    enum_name: &'static str,
    summary: &'static str,
}

#[derive(Debug, Clone)]
struct ParsedSnapshot {
    updated: String,
    registries: Vec<ParsedRegistry>,
}

#[derive(Debug, Clone)]
struct ParsedRegistry {
    title: &'static str,
    entries: Vec<RegistryEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RegistryEntry {
    name: String,
    references: Vec<String>,
}

#[derive(Debug, Clone)]
struct RenderedEntry {
    variant: String,
    name: String,
    reference: String,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1);

    match (args.next().as_deref(), args.next().as_deref()) {
        (Some("iana"), Some("fetch")) => fetch_snapshot(),
        (Some("iana"), Some("generate")) => generate(),
        (Some("iana"), Some("check")) => check(),
        (Some("iana"), Some("update")) => {
            fetch_snapshot()?;
            generate()
        }
        _ => Err(usage()),
    }
}

fn fetch_snapshot() -> Result<(), String> {
    let snapshot_path = project_path(SNAPSHOT_PATH);
    create_parent_dir(&snapshot_path)?;
    let xml = ureq::get(SNAPSHOT_URL)
        .call()
        .map_err(|error| format!("failed to fetch `{SNAPSHOT_URL}`: {error}"))?
        .body_mut()
        .read_to_string()
        .map_err(|error| format!("failed to read response body from `{SNAPSHOT_URL}`: {error}"))?;
    fs::write(&snapshot_path, xml)
        .map_err(|error| format!("failed to write `{}`: {error}", snapshot_path.display()))?;

    println!("updated {}", snapshot_path.display());

    Ok(())
}

fn generate() -> Result<(), String> {
    let snapshot = load_snapshot()?;
    let rendered = render_generated_module(&snapshot)?;
    let generated_path = project_path(GENERATED_PATH);

    create_parent_dir(&generated_path)?;

    if write_if_changed(&generated_path, &rendered)? {
        println!("updated {}", generated_path.display());
    } else {
        println!("unchanged {}", generated_path.display());
    }

    Ok(())
}

fn check() -> Result<(), String> {
    let snapshot = load_snapshot()?;
    let expected = render_generated_module(&snapshot)?;
    let generated_path = project_path(GENERATED_PATH);
    let current = fs::read_to_string(&generated_path).map_err(|error| {
        format!(
            "failed to read generated file `{}`: {error}",
            generated_path.display()
        )
    })?;

    if current == expected {
        println!("checked {}", generated_path.display());
        return Ok(());
    }

    print_generated_diff(&generated_path, &expected);

    Err(format!(
        "generated IANA enums are outdated, run `{GENERATE_COMMAND}`"
    ))
}

fn load_snapshot() -> Result<ParsedSnapshot, String> {
    let snapshot_path = project_path(SNAPSHOT_PATH);
    let xml = fs::read_to_string(&snapshot_path).map_err(|error| {
        format!(
            "failed to read snapshot `{}`: {error}",
            snapshot_path.display()
        )
    })?;

    parse_snapshot(&xml)
}

fn parse_snapshot(xml: &str) -> Result<ParsedSnapshot, String> {
    let document =
        Document::parse(xml).map_err(|error| format!("failed to parse snapshot XML: {error}"))?;
    let root = document.root_element();

    let updated =
        child_text(root, "updated").ok_or_else(|| "missing root `<updated>` value".to_owned())?;
    let mut registries = Vec::with_capacity(TARGET_REGISTRIES.len());

    for target in TARGET_REGISTRIES {
        let registry = child_elements(root, "registry")
            .find(|node| child_text(*node, "title").as_deref() == Some(target.title))
            .ok_or_else(|| format!("missing target registry `{}` in snapshot", target.title))?;

        let entries = child_elements(registry, "record")
            .map(parse_record)
            .collect::<Result<Vec<_>, _>>()?;

        registries.push(ParsedRegistry {
            title: target.title,
            entries,
        });
    }

    Ok(ParsedSnapshot {
        updated,
        registries,
    })
}

fn render_generated_module(snapshot: &ParsedSnapshot) -> Result<String, String> {
    let mut output = String::new();
    let current_year = current_year()?;

    writeln!(output, "{}", generated_copyright_line(&current_year)).unwrap();
    writeln!(output, "{GENERATED_LICENSE_LINE}").unwrap();
    writeln!(output).unwrap();
    writeln!(
        output,
        "// This file is @generated by `{GENERATE_COMMAND}`."
    )
    .unwrap();
    writeln!(output, "// Source snapshot: {SNAPSHOT_PATH}.").unwrap();
    writeln!(
        output,
        "// IANA SDP Parameters updated: {}.",
        snapshot.updated
    )
    .unwrap();
    writeln!(output).unwrap();
    writeln!(
        output,
        "pub const IANA_SDP_PARAMETERS_URL: &str = {};",
        string_literal(SNAPSHOT_URL)
    )
    .unwrap();
    writeln!(
        output,
        "pub const IANA_SDP_PARAMETERS_SNAPSHOT: &str = {};",
        string_literal(SNAPSHOT_PATH)
    )
    .unwrap();
    writeln!(
        output,
        "pub const IANA_SDP_PARAMETERS_UPDATED: &str = {};",
        string_literal(&snapshot.updated)
    )
    .unwrap();

    for target in TARGET_REGISTRIES {
        let entries = snapshot
            .registries
            .iter()
            .find(|registry| registry.title == target.title)
            .map(|registry| registry.entries.as_slice())
            .ok_or_else(|| format!("missing rendered registry `{}`", target.title))?;

        writeln!(output).unwrap();
        render_registry(&mut output, target, entries);
    }

    format_rust_source(&output)
}

fn render_registry(output: &mut String, target: TargetRegistry, entries: &[RegistryEntry]) {
    let rendered_entries = prepare_entries(entries);

    writeln!(output, "#[non_exhaustive]").unwrap();
    writeln!(output, "#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]").unwrap();
    writeln!(output, "#[doc = {}]", string_literal(target.summary)).unwrap();
    writeln!(output, "pub enum {} {{", target.enum_name).unwrap();

    for entry in &rendered_entries {
        writeln!(
            output,
            "    #[doc = {}]",
            string_literal(&format!("IANA token `{}`.", entry.name))
        )
        .unwrap();
        writeln!(
            output,
            "    #[doc = {}]",
            string_literal(&format!("Reference: {}.", entry.reference))
        )
        .unwrap();
        writeln!(output, "    {},", entry.variant).unwrap();
    }

    writeln!(output, "}}").unwrap();
    writeln!(output).unwrap();
    writeln!(output, "impl {} {{", target.enum_name).unwrap();
    writeln!(output, "    pub const ALL: &'static [Self] = &[").unwrap();

    for entry in &rendered_entries {
        writeln!(output, "        Self::{},", entry.variant).unwrap();
    }

    writeln!(output, "    ];").unwrap();
    writeln!(output).unwrap();
    writeln!(
        output,
        "    pub fn from_name(name: &str) -> Option<Self> {{"
    )
    .unwrap();
    writeln!(output, "        match name {{").unwrap();

    for entry in &rendered_entries {
        writeln!(
            output,
            "            {} => Some(Self::{}),",
            string_literal(&entry.name),
            entry.variant
        )
        .unwrap();
    }

    writeln!(output, "            _ => None,").unwrap();
    writeln!(output, "        }}").unwrap();
    writeln!(output, "    }}").unwrap();
    writeln!(output).unwrap();
    writeln!(output, "    pub const fn as_str(self) -> &'static str {{").unwrap();
    writeln!(output, "        match self {{").unwrap();

    for entry in &rendered_entries {
        writeln!(
            output,
            "            Self::{} => {},",
            entry.variant,
            string_literal(&entry.name)
        )
        .unwrap();
    }

    writeln!(output, "        }}").unwrap();
    writeln!(output, "    }}").unwrap();
    writeln!(output).unwrap();
    writeln!(
        output,
        "    pub const fn reference(self) -> &'static str {{"
    )
    .unwrap();
    writeln!(output, "        match self {{").unwrap();

    for entry in &rendered_entries {
        writeln!(
            output,
            "            Self::{} => {},",
            entry.variant,
            string_literal(&entry.reference)
        )
        .unwrap();
    }

    writeln!(output, "        }}").unwrap();
    writeln!(output, "    }}").unwrap();
    writeln!(output, "}}").unwrap();
    writeln!(output).unwrap();
    writeln!(
        output,
        "impl core::fmt::Display for {} {{",
        target.enum_name
    )
    .unwrap();
    writeln!(
        output,
        "    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {{"
    )
    .unwrap();
    writeln!(output, "        formatter.write_str(self.as_str())").unwrap();
    writeln!(output, "    }}").unwrap();
    writeln!(output, "}}").unwrap();
    writeln!(output).unwrap();
    writeln!(
        output,
        "impl core::str::FromStr for {} {{",
        target.enum_name
    )
    .unwrap();
    writeln!(output, "    type Err = ();").unwrap();
    writeln!(
        output,
        "    fn from_str(value: &str) -> Result<Self, Self::Err> {{"
    )
    .unwrap();
    writeln!(output, "        Self::from_name(value).ok_or(())").unwrap();
    writeln!(output, "    }}").unwrap();
    writeln!(output, "}}").unwrap();
}

fn prepare_entries(entries: &[RegistryEntry]) -> Vec<RenderedEntry> {
    let mut sorted_entries = entries.to_vec();
    sorted_entries.sort_by(|left, right| left.name.cmp(&right.name));

    let mut used_variants = BTreeSet::new();

    sorted_entries
        .into_iter()
        .map(|entry| RenderedEntry {
            variant: normalize_variant_name(&entry.name, &mut used_variants),
            reference: if entry.references.is_empty() {
                "Unspecified".to_owned()
            } else {
                entry.references.join(", ")
            },
            name: entry.name,
        })
        .collect()
}

fn parse_record(record: Node<'_, '_>) -> Result<RegistryEntry, String> {
    let name = child_text(record, "name")
        .ok_or_else(|| "encountered a target record without a name".to_owned())?;
    let references = child_elements(record, "xref")
        .filter_map(render_reference)
        .collect();

    Ok(RegistryEntry { name, references })
}

fn normalize_variant_name(name: &str, used_variants: &mut BTreeSet<String>) -> String {
    let mut candidate = String::new();
    let mut segment = String::new();

    for character in name.chars() {
        if character.is_ascii_alphanumeric() {
            segment.push(character);
        } else if !segment.is_empty() {
            candidate.push_str(&pascal_case_segment(&segment));
            segment.clear();
        }
    }

    if !segment.is_empty() {
        candidate.push_str(&pascal_case_segment(&segment));
    }

    if candidate.is_empty() {
        candidate.push_str("Value");
    }

    if candidate
        .chars()
        .next()
        .is_some_and(|character| character.is_ascii_digit())
    {
        candidate.insert(0, 'V');
    }

    if is_reserved_identifier(&candidate) {
        candidate.push_str("Value");
    }

    if used_variants.insert(candidate.clone()) {
        return candidate;
    }

    let base = candidate;

    for suffix in 2usize.. {
        let candidate = format!("{base}{suffix}");

        if used_variants.insert(candidate.clone()) {
            return candidate;
        }
    }

    unreachable!()
}

fn pascal_case_segment(segment: &str) -> String {
    let mut characters = segment.chars();
    let mut output = String::with_capacity(segment.len());

    if let Some(first) = characters.next() {
        output.extend(first.to_uppercase());
    }

    for character in characters {
        output.extend(character.to_lowercase());
    }

    output
}

fn is_reserved_identifier(candidate: &str) -> bool {
    syn::parse_str::<syn::Ident>(candidate.to_ascii_lowercase().as_str()).is_err()
}

fn child_elements<'a, 'input>(
    node: Node<'a, 'input>,
    name: &'static str,
) -> impl Iterator<Item = Node<'a, 'input>> {
    node.children()
        .filter(move |child| child.has_tag_name((IANA_NAMESPACE, name)))
}

fn child_text(node: Node<'_, '_>, name: &'static str) -> Option<String> {
    child_elements(node, name)
        .find_map(|child| child.text())
        .map(str::trim)
        .map(str::to_owned)
}

fn render_reference(xref: Node<'_, '_>) -> Option<String> {
    let xref_type = xref.attribute("type")?;
    let data = xref.attribute("data").unwrap_or_default();

    let reference = match xref_type {
        "rfc" => normalize_rfc_reference(data),
        "draft" | "uri" => data.to_owned(),
        "note" => format!("Note {data}"),
        _ => data.to_owned(),
    };

    (!reference.is_empty()).then_some(reference)
}

fn normalize_rfc_reference(value: &str) -> String {
    if let Some(rfc_number) = value
        .strip_prefix("rfc")
        .or_else(|| value.strip_prefix("RFC"))
    {
        return format!("RFC {rfc_number}");
    }

    value.to_owned()
}

fn string_literal(value: &str) -> String {
    let mut literal = String::with_capacity(value.len() + 2);
    literal.push('"');

    for character in value.chars() {
        match character {
            '\\' => literal.push_str("\\\\"),
            '"' => literal.push_str("\\\""),
            '\n' => literal.push_str("\\n"),
            '\r' => literal.push_str("\\r"),
            '\t' => literal.push_str("\\t"),
            _ => literal.push(character),
        }
    }

    literal.push('"');
    literal
}

fn write_if_changed(path: &Path, contents: &str) -> Result<bool, String> {
    match fs::read_to_string(path) {
        Ok(existing) if existing == contents => Ok(false),
        Ok(_) | Err(_) => {
            fs::write(path, contents)
                .map_err(|error| format!("failed to write `{}`: {error}", path.display()))?;
            Ok(true)
        }
    }
}

fn print_generated_diff(path: &Path, expected: &str) {
    let expected_path =
        env::temp_dir().join(format!("sphynx-iana-expected-{}.rs", std::process::id()));

    if let Err(error) = fs::write(&expected_path, expected) {
        eprintln!(
            "failed to write temporary diff input `{}`: {error}",
            expected_path.display()
        );
        return;
    }

    let output = Command::new("git")
        .args(["diff", "--no-index", "--no-ext-diff", "--"])
        .arg(path)
        .arg(&expected_path)
        .output();

    match output {
        Ok(output) => {
            if !output.stdout.is_empty() {
                eprint!("{}", String::from_utf8_lossy(&output.stdout));
            }

            if !output.stderr.is_empty() {
                eprint!("{}", String::from_utf8_lossy(&output.stderr));
            }
        }
        Err(error) => {
            eprintln!("failed to run `git diff --no-index`: {error}");
        }
    }

    let _ = fs::remove_file(expected_path);
}

fn format_rust_source(source: &str) -> Result<String, String> {
    let mut rustfmt = Command::new("rustfmt")
        .args(["--edition", "2024"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|error| format!("failed to spawn `rustfmt`: {error}"))?;

    rustfmt
        .stdin
        .as_mut()
        .ok_or_else(|| "failed to open `rustfmt` stdin".to_owned())?
        .write_all(source.as_bytes())
        .map_err(|error| format!("failed to write generated source to `rustfmt`: {error}"))?;

    let output = rustfmt
        .wait_with_output()
        .map_err(|error| format!("failed to wait for `rustfmt`: {error}"))?;

    if !output.status.success() {
        return Err(format!(
            "`rustfmt` exited unsuccessfully: {}",
            output.status
        ));
    }

    String::from_utf8(output.stdout)
        .map_err(|error| format!("`rustfmt` returned non-UTF-8 output: {error}"))
}

fn generated_copyright_line(year: &str) -> String {
    let prefix = concat!("// SPDX-FileCopyright", "Text: ");
    format!("{prefix}{year} Meowdia Community")
}

fn current_year() -> Result<String, String> {
    let output = Command::new("date")
        .args(["-u", "+%Y"])
        .output()
        .map_err(|error| format!("failed to spawn `date`: {error}"))?;

    if !output.status.success() {
        return Err(format!("`date` exited unsuccessfully: {}", output.status));
    }

    let year = String::from_utf8(output.stdout)
        .map_err(|error| format!("`date` returned non-UTF-8 output: {error}"))?;
    let year = year.trim();

    if year.len() != 4 || !year.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(format!("`date` returned an unexpected year: {year}"));
    }

    Ok(year.to_owned())
}

fn create_parent_dir(path: &Path) -> Result<(), String> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };

    fs::create_dir_all(parent)
        .map_err(|error| format!("failed to create `{}`: {error}", parent.display()))
}

fn project_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask manifest should have a parent")
        .to_path_buf()
}

fn project_path(relative_path: &str) -> PathBuf {
    project_root().join(relative_path)
}

fn usage() -> String {
    "usage: cargo run -p xtask -- iana <fetch|generate|check|update>".to_owned()
}

#[cfg(test)]
mod tests {
    use super::{normalize_variant_name, parse_snapshot};
    use std::collections::BTreeSet;

    #[test]
    fn parse_snapshot_collects_multiple_xrefs() {
        let snapshot = parse_snapshot(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<registry xmlns="http://www.iana.org/assignments" id="sdp-parameters">
  <updated>2026-03-05</updated>
  <registry id="sdp-parameters-1">
    <title>media</title>
    <record><name>audio</name><xref type="rfc" data="rfc8866"/></record>
  </registry>
  <registry id="sdp-parameters-2">
    <title>proto</title>
    <record>
      <name>UDP/TLS/RTP/SAVPF</name>
      <xref type="rfc" data="rfc5764"/>
      <xref type="note" data="1"/>
    </record>
  </registry>
  <registry id="sdp-parameters-3">
    <title>bwtype</title>
    <record><name>AS</name><xref type="rfc" data="rfc8859"/></record>
  </registry>
  <registry id="sdp-parameters-4">
    <title>nettype</title>
    <record><name>IN</name><xref type="rfc" data="rfc8866"/></record>
  </registry>
  <registry id="sdp-parameters-5">
    <title>addrtype</title>
    <record><name>IP4</name><xref type="rfc" data="rfc8866"/></record>
  </registry>
</registry>"#,
        )
        .expect("snapshot should parse");
        let references = snapshot.registries[1].entries[0]
            .references
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();

        assert_eq!(references, ["RFC 5764", "Note 1"]);
    }

    #[test]
    fn variant_names_are_normalized_from_sdp_tokens() {
        let mut used_variants = BTreeSet::new();

        assert_eq!(
            normalize_variant_name("UDP/TLS/RTP/SAVPF", &mut used_variants),
            "UdpTlsRtpSavpf"
        );
        assert_eq!(normalize_variant_name("AS", &mut used_variants), "AsValue");
    }
}
