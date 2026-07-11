#[derive(Debug, PartialEq)]
pub enum Range {
    Full,
    Partial {
        start: u64,
        end: u64,
    },
    Unsatisfiable,
}
pub fn parse(value: Option<&str>, length: u64) -> Range {
    let Some(value) = value else {
        return Range::Full;
    };
    let Some(spec) = value.strip_prefix("bytes=") else {
        return Range::Full;
    };
    if spec.contains(',') {
        return Range::Full;
    }
    let Some((first, last)) = spec.trim().split_once('-') else {
        return Range::Full;
    };
    let decimal = |s: &str| -> Option<u64> {
        if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
            None
        } else {
            s.parse().ok()
        }
    };
    if first.is_empty() {
        let Some(suffix) = decimal(last) else {
            return Range::Full;
        };
        if suffix == 0 || length == 0 {
            return Range::Unsatisfiable;
        }
        return Range::Partial {
            start: length.saturating_sub(suffix),
            end: length - 1,
        };
    }
    let Some(start) = decimal(first) else {
        return Range::Full;
    };
    let end = if last.is_empty() {
        length.saturating_sub(1)
    } else {
        let Some(end) = decimal(last) else {
            return Range::Full;
        };
        if end < start {
            return Range::Full;
        }
        end
    };
    if start >= length {
        return Range::Unsatisfiable;
    }
    Range::Partial {
        start,
        end: end.min(length - 1),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn boundaries() {
        for (input, length, expected) in [
            (None, 10, Range::Full),
            (Some("bytes=0-0"), 10, Range::Partial { start: 0, end: 0 }),
            (Some("bytes=5-"), 10, Range::Partial { start: 5, end: 9 }),
            (Some("bytes=-3"), 10, Range::Partial { start: 7, end: 9 }),
            (Some("bytes=0-999"), 10, Range::Partial { start: 0, end: 9 }),
            (Some("bytes=-999"), 10, Range::Partial { start: 0, end: 9 }),
            (Some("bytes=10-"), 10, Range::Unsatisfiable),
            (Some("bytes=-0"), 10, Range::Unsatisfiable),
            (Some("bytes=0-"), 0, Range::Unsatisfiable),
            (Some("bytes=-1"), 0, Range::Unsatisfiable),
            (Some("bytes=8-2"), 10, Range::Full),
            (Some("bytes=0-1,3-4"), 10, Range::Full),
            (Some("bytes=+1-2"), 10, Range::Full),
            (Some("bytes=18446744073709551616-"), 10, Range::Full),
            (Some("items=1-2"), 10, Range::Full),
            (Some("bytes=garbage"), 10, Range::Full),
        ] {
            assert_eq!(parse(input, length), expected, "{input:?}");
        }
    }
}
