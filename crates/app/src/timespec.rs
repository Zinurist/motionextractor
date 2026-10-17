use std::str::FromStr;

/// A position in a video given on the command line: a frame index (`120`, `120f`), seconds
/// (`2.5s`) or a timestamp (`1:02.5`, `0:01:02.5`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TimeSpec {
    Frame(u64),
    Seconds(f64),
}

impl TimeSpec {
    /// Frame index at the given frame rate (rounded to the nearest frame).
    pub fn to_frame(self, fps: f64) -> u64 {
        match self {
            TimeSpec::Frame(n) => n,
            TimeSpec::Seconds(s) => (s * fps).round() as u64,
        }
    }
}

impl FromStr for TimeSpec {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bad = || format!("invalid time {s:?}, expected e.g. 120, 120f, 2.5s or 1:02.5");
        let t = s.trim();
        if t.contains(':') {
            let mut seconds = 0.0;
            for part in t.split(':') {
                let v: f64 = part.parse().map_err(|_| bad())?;
                seconds = seconds * 60.0 + v;
            }
            return non_negative(seconds).map(TimeSpec::Seconds).ok_or_else(bad);
        }
        if let Some(v) = t.strip_suffix('s') {
            let v: f64 = v.trim().parse().map_err(|_| bad())?;
            return non_negative(v).map(TimeSpec::Seconds).ok_or_else(bad);
        }
        let v = t.strip_suffix('f').unwrap_or(t).trim();
        v.parse().map(TimeSpec::Frame).map_err(|_| bad())
    }
}

fn non_negative(v: f64) -> Option<f64> {
    (v.is_finite() && v >= 0.0).then_some(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses() {
        assert_eq!("120".parse(), Ok(TimeSpec::Frame(120)));
        assert_eq!("7f".parse(), Ok(TimeSpec::Frame(7)));
        assert_eq!("2.5s".parse(), Ok(TimeSpec::Seconds(2.5)));
        assert_eq!("1:02.5".parse(), Ok(TimeSpec::Seconds(62.5)));
        assert_eq!("1:00:00".parse(), Ok(TimeSpec::Seconds(3600.0)));
        assert!("-1".parse::<TimeSpec>().is_err());
        assert!("-1s".parse::<TimeSpec>().is_err());
        assert!("abc".parse::<TimeSpec>().is_err());
    }

    #[test]
    fn to_frame() {
        assert_eq!(TimeSpec::Frame(5).to_frame(30.0), 5);
        assert_eq!(TimeSpec::Seconds(0.5).to_frame(30.0), 15);
        assert_eq!(TimeSpec::Seconds(1.0).to_frame(29.97), 30);
    }
}
