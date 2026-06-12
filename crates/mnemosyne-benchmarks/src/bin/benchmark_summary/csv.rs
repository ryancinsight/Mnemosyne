use std::borrow::Cow;

pub fn parse_summary_line(line: &str) -> Option<(Cow<'_, str>, f64, f64)> {
    let fields = parse_csv_line_cow(line);
    if fields.len() != 3 {
        return None;
    }

    let mean_ns = fields[1].parse().ok()?;
    let median_ns = fields[2].parse().ok()?;
    Some((fields[0].clone(), mean_ns, median_ns))
}

pub fn parse_csv_line_cow(line: &str) -> Vec<Cow<'_, str>> {
    let mut fields = Vec::new();
    let mut chars = line.char_indices().peekable();
    let mut in_quotes = false;
    let mut start = 0;
    let mut has_escapes = false;

    while let Some((idx, ch)) = chars.next() {
        match ch {
            '"' if in_quotes && chars.peek().map(|&(_, c)| c) == Some('"') => {
                has_escapes = true;
                chars.next();
            }
            '"' => in_quotes = !in_quotes,
            ',' if !in_quotes => {
                let segment = &line[start..idx];
                fields.push(process_segment(segment, has_escapes));
                start = idx + 1;
                has_escapes = false;
            }
            _ => {}
        }
    }
    let segment = &line[start..];
    fields.push(process_segment(segment, has_escapes));
    fields
}

pub fn escape_csv(value: &str) -> String {
    if value.contains(',') || value.contains('"') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_owned()
    }
}

fn process_segment(segment: &str, has_escapes: bool) -> Cow<'_, str> {
    let trimmed = segment.trim();
    let stripped = if trimmed.starts_with('"') && trimmed.ends_with('"') {
        &trimmed[1..trimmed.len() - 1]
    } else {
        trimmed
    };

    if has_escapes {
        Cow::Owned(stripped.replace("\"\"", "\""))
    } else {
        Cow::Borrowed(stripped)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_escaped_summary_row() {
        let row = parse_summary_line("\"allocator, \"\"quoted\"\"\",1.250000,2.500000")
            .expect("valid escaped row");

        assert_eq!(row.0, "allocator, \"quoted\"");
        assert_eq!(row.1, 1.25);
        assert_eq!(row.2, 2.5);
    }
}
