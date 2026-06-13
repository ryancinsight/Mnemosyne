use std::borrow::Cow;

pub fn parse_summary_line(line: &str) -> Option<(Cow<'_, str>, f64, f64)> {
    let mut fields = CsvFields::new(line);
    let benchmark = fields.next()?;
    let mean_ns = fields.next()?.parse().ok()?;
    let median_ns = fields.next()?.parse().ok()?;
    if fields.next().is_some() {
        return None;
    }

    Some((benchmark, mean_ns, median_ns))
}

struct CsvFields<'a> {
    line: &'a str,
    start: usize,
}

impl<'a> CsvFields<'a> {
    fn new(line: &'a str) -> Self {
        Self { line, start: 0 }
    }
}

impl<'a> Iterator for CsvFields<'a> {
    type Item = Cow<'a, str>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.start > self.line.len() {
            return None;
        }

        let start = self.start;
        let mut chars = self.line[start..].char_indices().peekable();
        let mut in_quotes = false;
        let mut has_escapes = false;

        while let Some((relative_idx, ch)) = chars.next() {
            match ch {
                '"' if in_quotes && chars.peek().map(|&(_, c)| c) == Some('"') => {
                    has_escapes = true;
                    chars.next();
                }
                '"' => in_quotes = !in_quotes,
                ',' if !in_quotes => {
                    let end = start + relative_idx;
                    self.start = end + 1;
                    return Some(process_segment(&self.line[start..end], has_escapes));
                }
                _ => {}
            }
        }

        self.start = self.line.len() + 1;
        Some(process_segment(&self.line[start..], has_escapes))
    }
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

    #[test]
    fn rejects_rows_with_missing_summary_fields() {
        assert_eq!(parse_summary_line("allocator,1.250000"), None);
    }

    #[test]
    fn rejects_rows_with_extra_summary_fields() {
        assert_eq!(
            parse_summary_line("allocator,1.250000,2.500000,extra"),
            None
        );
    }
}
