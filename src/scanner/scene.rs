use regex::Regex;
use std::sync::LazyLock;
static EPISODE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)(?:^|[ ._\-])s(\d{1,3})e(\d{1,4})").unwrap());
static YEAR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:^|[ ._(])((?:19\d{2}|20[0-3]\d))(?:[ ._)\-]|$)").unwrap());
static RELEASE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:^|[ ._\-])(?:2160p|1080[pi]|720p|480p|german|english|multi|dl|dual|eac3|aac|dts|ac3|bluray|bdrip|webrip|web[ ._-]?dl|web|h26[45]|x26[45]|hevc|hdr|dv|sample)(?:[ ._\-]|$)").unwrap()
});
#[derive(Debug, PartialEq)]
pub struct Scene {
    pub title: String,
    pub year: Option<i64>,
    pub season: Option<i64>,
    pub episode: Option<i64>,
}
pub fn parse(name: &str) -> Scene {
    let stem = name
        .rsplit_once('.')
        .filter(|(_, ext)| {
            matches!(
                ext.to_ascii_lowercase().as_str(),
                "mkv" | "mp4" | "avi" | "m4v" | "mov" | "webm" | "ts" | "m2ts"
            )
        })
        .map_or(name, |(stem, _)| stem);
    let episode = EPISODE.captures(stem);
    let before = episode
        .as_ref()
        .map_or(stem, |c| &stem[..c.get(0).unwrap().start()]);
    let year = YEAR.captures(before);
    let end = year
        .as_ref()
        .map(|c| c.get(0).unwrap().start())
        .into_iter()
        .chain(RELEASE.find(before).map(|m| m.start()))
        .min()
        .unwrap_or(before.len());
    let title = before[..end]
        .replace(['.', '_'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_matches([' ', '-', '(', ')', '[', ']'])
        .to_string();
    Scene {
        title: if title.is_empty() {
            before.replace(['.', '_'], " ")
        } else {
            title
        },
        year: year.and_then(|c| c[1].parse().ok()),
        season: episode.as_ref().and_then(|c| c[1].parse().ok()),
        episode: episode.and_then(|c| c[2].parse().ok()),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scene_hints_do_not_invent_punctuation() {
        let s = parse(
            "Alien.Earth.S01E05.German.DL.EAC3.1080p.DV.HDR.DSNP.WEB.H265-ZeroTwo_sample.mkv",
        );
        assert_eq!(s.title, "Alien Earth");
        assert_eq!((s.season, s.episode), (Some(1), Some(5)));
        let m = parse("Blade.Runner.2049.2017.2160p.BluRay.x265.mkv");
        assert_eq!(m.title, "Blade Runner 2049");
        assert_eq!(m.year, Some(2017));
        assert_eq!(parse("Up.2009.1080p.mkv").title, "Up");
        assert_eq!(parse("A.Movie.WEB.H265.mkv").title, "A Movie");
    }
}
