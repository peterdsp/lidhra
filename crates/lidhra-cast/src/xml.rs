//! Tiny tag extractor for the XML that TVs speak (UPnP descriptions, SOAP replies, Roku
//! `device-info`). No DOM and no dependency: it walks tags, compares *local* names (so
//! `<s:Fault>` matches `Fault`) case-insensitively, and counts depth of same-named elements
//! so a nested `<device>` inside `<deviceList>` does not close the root `<device>` early.
//!
//! Comments, processing instructions, doctypes and CDATA sections are skipped while
//! looking for tags, and quoted attribute values may contain `>`. That is enough for
//! real-world descriptions; it is not a validating parser.

use std::fmt::Write;

/// Byte offsets of one element: `start..end` is the whole element, `inner` its content.
#[derive(Clone, Copy, Debug)]
struct Span {
    start: usize,
    inner_start: usize,
    inner_end: usize,
    end: usize,
}

struct Tag<'a> {
    name: &'a str,
    start: usize,
    end: usize,
    closing: bool,
    empty: bool,
}

/// Local part of a possibly prefixed name (`s:Envelope` -> `Envelope`).
fn local(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

/// Next real tag at or after byte `from`, skipping comments, `<?...?>`, `<!...>` and CDATA.
fn next_tag(xml: &str, mut from: usize) -> Option<Tag<'_>> {
    let b = xml.as_bytes();
    loop {
        let lt = from + xml.get(from..)?.find('<')?;
        let rest = &xml[lt..];
        if rest.starts_with("<!--") {
            from = lt + rest.find("-->")? + 3;
            continue;
        }
        if rest.starts_with("<![CDATA[") {
            from = lt + rest.find("]]>")? + 3;
            continue;
        }
        if rest.starts_with("<?") || rest.starts_with("<!") {
            from = lt + rest.find('>')? + 1;
            continue;
        }
        // Find the closing '>' while honouring quoted attribute values.
        let mut quote = None;
        let mut j = lt + 1;
        while j < b.len() {
            let c = b[j];
            match quote {
                Some(q) if c == q => quote = None,
                Some(_) => {}
                None if c == b'"' || c == b'\'' => quote = Some(c),
                None if c == b'>' => break,
                None => {}
            }
            j += 1;
        }
        if j >= b.len() {
            return None;
        }
        let closing = b.get(lt + 1) == Some(&b'/');
        let name_start = lt + 1 + usize::from(closing);
        let raw = &xml[name_start..j];
        let name_len = raw.find(|c: char| c.is_whitespace() || c == '/').unwrap_or(raw.len());
        let name = local(&raw[..name_len]);
        if name.is_empty() {
            // A stray '<' in text: not a tag, keep scanning after it.
            from = lt + 1;
            continue;
        }
        let empty = !closing && b[j - 1] == b'/';
        return Some(Tag { name, start: lt, end: j + 1, closing, empty });
    }
}

/// First element named `name` (local name, any case) starting at or after `from`.
fn find(xml: &str, name: &str, from: usize) -> Option<Span> {
    let mut pos = from;
    let open = loop {
        let t = next_tag(xml, pos)?;
        pos = t.end;
        if !t.closing && t.name.eq_ignore_ascii_case(name) {
            break t;
        }
    };
    if open.empty {
        return Some(Span { start: open.start, inner_start: open.end, inner_end: open.end, end: open.end });
    }
    let mut depth = 1usize;
    loop {
        let t = next_tag(xml, pos)?;
        pos = t.end;
        if !t.name.eq_ignore_ascii_case(name) || t.empty {
            continue;
        }
        if t.closing {
            depth -= 1;
            if depth == 0 {
                return Some(Span { start: open.start, inner_start: open.end, inner_end: t.start, end: t.end });
            }
        } else {
            depth += 1;
        }
    }
}

/// Raw inner markup of the first `name` element.
pub(crate) fn element<'a>(xml: &'a str, name: &str) -> Option<&'a str> {
    find(xml, name, 0).map(|s| &xml[s.inner_start..s.inner_end])
}

/// Raw inner markup of every `name` element that is not nested inside another one.
pub(crate) fn elements<'a>(xml: &'a str, name: &str) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut pos = 0;
    while let Some(s) = find(xml, name, pos) {
        out.push(&xml[s.inner_start..s.inner_end]);
        pos = s.end;
    }
    out
}

/// Unescaped, trimmed text of the first `name` element; `None` when absent or empty.
pub(crate) fn text(xml: &str, name: &str) -> Option<String> {
    let inner = element(xml, name)?.trim();
    let value = match inner.strip_prefix("<![CDATA[").and_then(|s| s.strip_suffix("]]>")) {
        Some(cdata) => cdata.trim().to_string(),
        None => unescape(inner),
    };
    (!value.is_empty()).then_some(value)
}

/// Copy of `xml` with every `name` element removed (used to ignore embedded devices).
pub(crate) fn without(xml: &str, name: &str) -> String {
    let mut out = String::with_capacity(xml.len());
    let mut pos = 0;
    while let Some(s) = find(xml, name, pos) {
        out.push_str(&xml[pos..s.start]);
        pos = s.end;
    }
    out.push_str(&xml[pos..]);
    out
}

/// Escape text for element content or a double- or single-quoted attribute.
pub(crate) fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

/// Decode the five predefined entities and numeric character references.
pub(crate) fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after = &rest[amp + 1..];
        let decoded = after.find(';').filter(|&semi| semi <= 10).and_then(|semi| {
            let ent = &after[..semi];
            let ch = match ent {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                _ => {
                    let num = ent.strip_prefix('#')?;
                    let code = match num.strip_prefix('x').or_else(|| num.strip_prefix('X')) {
                        Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                        None => num.parse().ok()?,
                    };
                    char::from_u32(code)
                }
            };
            ch.map(|c| (c, semi))
        });
        match decoded {
            Some((c, semi)) => {
                out.push(c);
                rest = &after[semi + 1..];
            }
            None => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Append `<name>escaped value</name>` to `out`.
pub(crate) fn push_element(out: &mut String, name: &str, value: &str) {
    let _ = write!(out, "<{name}>{}</{name}>", escape(value));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_local_names_and_skips_similar_prefixes() {
        let xml = r#"<s:Envelope><s:Body><serviceList/><service a="x>y"><x>1</x></service></s:Body></s:Envelope>"#;
        assert_eq!(element(xml, "Body"), Some(r#"<serviceList/><service a="x>y"><x>1</x></service>"#));
        assert_eq!(text(xml, "x").as_deref(), Some("1"));
        assert_eq!(element(xml, "serviceList"), Some(""));
        assert_eq!(elements(xml, "service").len(), 1);
    }

    #[test]
    fn counts_depth_of_nested_same_name_elements() {
        let xml = "<device><name>root</name><deviceList><device><name>child</name></device></deviceList>\
                   <tail>t</tail></device>";
        let dev = element(xml, "device").unwrap();
        assert!(dev.ends_with("<tail>t</tail>"));
        let own = without(dev, "deviceList");
        assert_eq!(text(&own, "name").as_deref(), Some("root"));
        assert_eq!(elements(xml, "device").len(), 1);
    }

    #[test]
    fn skips_comments_cdata_and_declarations() {
        let xml = "<?xml version=\"1.0\"?><!DOCTYPE x><!-- <a>no</a> --><r><![CDATA[<a>no</a>]]><a>yes</a>\
                   <b><![CDATA[ raw & text ]]></b></r>";
        assert_eq!(text(xml, "a").as_deref(), Some("yes"));
        assert_eq!(text(xml, "b").as_deref(), Some("raw & text"));
    }

    #[test]
    fn escapes_and_unescapes() {
        assert_eq!(escape(r#"a&b<c>"d'"#), "a&amp;b&lt;c&gt;&quot;d&apos;");
        assert_eq!(unescape("a&amp;b&lt;&#65;&#x42;&bogus; & tail"), "a&b<AB&bogus; & tail");
        assert_eq!(unescape(&escape("Tom & Jerry <3 \"x\"")), "Tom & Jerry <3 \"x\"");
    }

    #[test]
    fn empty_or_missing_text_is_none() {
        assert_eq!(text("<a> </a>", "a"), None);
        assert_eq!(text("<a/>", "a"), None);
        assert_eq!(text("<a>1</a>", "b"), None);
        assert_eq!(element("<a>unterminated", "a"), None);
    }
}
