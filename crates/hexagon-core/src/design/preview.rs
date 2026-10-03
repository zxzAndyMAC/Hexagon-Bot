use super::{bounded, invalid, DesignMockup, DesignMockupInput};
use crate::tools::ToolError;
use base64::Engine;
use sha2::{Digest, Sha256};

pub(super) fn validate(input: DesignMockupInput) -> Result<DesignMockup, ToolError> {
    if !bounded(&input.page, 120) || input.content.len() > 3_000_000 {
        return Err(invalid("mockup page/content exceeds bounds"));
    }
    let bytes = match input.mime.as_str() {
        "image/svg+xml" => {
            svg(&input.content)?;
            input.content.into_bytes()
        }
        "image/png" => {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(&input.content)
                .map_err(|_| invalid("invalid PNG base64"))?;
            let decoder = png::Decoder::new_with_limits(
                std::io::Cursor::new(&bytes),
                png::Limits {
                    bytes: 64 * 1024 * 1024,
                },
            );
            let mut reader = decoder
                .read_info()
                .map_err(|_| invalid("invalid PNG image"))?;
            let info = reader.info();
            if info.width < 32
                || info.height < 32
                || info.width > 8192
                || info.height > 8192
                || info.animation_control.is_some()
            {
                return Err(invalid("mockup must be a bounded static PNG"));
            }
            let length = reader
                .output_buffer_size()
                .filter(|size| *size <= 64 * 1024 * 1024)
                .ok_or_else(|| invalid("decoded mockup is too large"))?;
            reader
                .next_frame(&mut vec![0; length])
                .map_err(|_| invalid("invalid PNG pixels"))?;
            bytes
        }
        _ => return Err(invalid("mockup must be SVG or PNG")),
    };
    let digest = format!("{:x}", Sha256::digest(&bytes));
    let data_url = format!(
        "data:{};base64,{}",
        input.mime,
        base64::engine::general_purpose::STANDARD.encode(&bytes)
    );
    Ok(DesignMockup {
        page: input.page,
        mime: input.mime,
        digest,
        data_url,
    })
}

/// Owner Q5: render static images, never executable markup. Unknown SVG tags or
/// attributes are rejected rather than stripped into a different owner preview.
/// False negatives need another mockup; false positives execute unreviewed code.
fn svg(source: &str) -> Result<(), ToolError> {
    use quick_xml::events::Event;
    if source.len() > 256_000 {
        return Err(invalid("SVG mockup exceeds 256 KB"));
    }
    const TAGS: &[&str] = &[
        "svg",
        "g",
        "rect",
        "path",
        "circle",
        "ellipse",
        "line",
        "polyline",
        "polygon",
        "text",
        "tspan",
        "defs",
        "clipPath",
        "linearGradient",
        "radialGradient",
        "stop",
    ];
    const ATTRS: &[&str] = &[
        "xmlns",
        "width",
        "height",
        "viewBox",
        "x",
        "y",
        "x1",
        "y1",
        "x2",
        "y2",
        "rx",
        "ry",
        "cx",
        "cy",
        "r",
        "d",
        "points",
        "fill",
        "stroke",
        "stroke-width",
        "stroke-linecap",
        "stroke-linejoin",
        "font-size",
        "font-family",
        "font-weight",
        "text-anchor",
        "dominant-baseline",
        "opacity",
        "fill-opacity",
        "stroke-opacity",
        "transform",
        "id",
        "clip-path",
        "offset",
        "stop-color",
        "stop-opacity",
        "gradientUnits",
        "gradientTransform",
    ];
    let mut reader = quick_xml::Reader::from_str(source);
    let mut depth = 0usize;
    let mut root_seen = false;
    let mut bounded_viewbox = false;
    let mut namespace = false;
    let mut visible = 0usize;
    let mut nodes = 0usize;
    loop {
        let event = reader
            .read_event()
            .map_err(|_| invalid("malformed SVG mockup"))?;
        match &event {
            Event::Start(element) | Event::Empty(element) => {
                nodes += 1;
                if nodes > 4096 {
                    return Err(invalid("SVG mockup has too many nodes"));
                }
                let qualified = element.name();
                let tag: &str = qualified.as_ref();
                if !TAGS.contains(&tag) {
                    return Err(invalid("unsupported active or external SVG element"));
                }
                if depth == 0 {
                    if root_seen || tag != "svg" {
                        return Err(invalid("mockup requires one SVG root"));
                    }
                    root_seen = true;
                }
                if matches!(
                    tag,
                    "rect"
                        | "path"
                        | "circle"
                        | "ellipse"
                        | "line"
                        | "polyline"
                        | "polygon"
                        | "text"
                ) {
                    visible += 1;
                }
                for attribute in element.attributes() {
                    let attribute = attribute.map_err(|_| invalid("invalid SVG attribute"))?;
                    let name: &str = attribute.key.as_ref();
                    let value: &str = attribute.value.as_ref();
                    if !ATTRS.contains(&name) || value.contains('&') {
                        return Err(invalid("unsupported SVG attribute or entity"));
                    }
                    if name == "xmlns" {
                        if value != "http://www.w3.org/2000/svg" {
                            return Err(invalid("invalid SVG namespace"));
                        }
                        if depth == 0 {
                            namespace = true;
                        }
                        continue;
                    }
                    if value.to_ascii_lowercase().contains("url(") {
                        let reference = value
                            .strip_prefix("url(#")
                            .and_then(|v| v.strip_suffix(')'));
                        if !reference.is_some_and(|id| {
                            !id.is_empty()
                                && id
                                    .chars()
                                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
                        }) {
                            return Err(invalid("external SVG resource is forbidden"));
                        }
                    }
                    if value.contains(":")
                        || value.contains('\\')
                        || value.contains('@')
                        || value.len() > 64_000
                    {
                        return Err(invalid("external SVG resource or excessive attribute"));
                    }
                    if depth == 0 && name == "viewBox" {
                        let numbers: Vec<f64> = value
                            .split_whitespace()
                            .map(str::parse)
                            .collect::<Result<_, _>>()
                            .map_err(|_| invalid("invalid SVG viewBox"))?;
                        bounded_viewbox = numbers.len() == 4
                            && numbers.iter().all(|n| n.is_finite())
                            && (32.0..=8192.0).contains(&numbers[2])
                            && (32.0..=8192.0).contains(&numbers[3]);
                    }
                }
                if matches!(event, Event::Start(_)) {
                    depth += 1;
                }
            }
            Event::End(_) => {
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| invalid("unbalanced SVG"))?;
            }
            Event::DocType(_) | Event::PI(_) | Event::CData(_) => {
                return Err(invalid("active XML constructs are forbidden"))
            }
            Event::GeneralRef(reference) => {
                let value: &str = reference.as_ref();
                if !["amp", "lt", "gt", "quot", "apos"].contains(&value) {
                    return Err(invalid("custom SVG entity is forbidden"));
                }
            }
            Event::Text(text) if depth == 0 && !text.as_ref().trim().is_empty() => {
                return Err(invalid("text outside SVG root"))
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if !root_seen || depth != 0 || !namespace || !bounded_viewbox || visible == 0 {
        return Err(invalid(
            "mockup needs a complete static key-page SVG with bounded viewBox",
        ));
    }
    Ok(())
}
