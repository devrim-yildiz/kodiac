//! Deterministic filename hints. Canonical punctuation and episode names come from NFO/TMDB.
use regex::Regex;
use std::sync::LazyLock;
static EPISODE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)(?:^|[ ._\-])s(\d{1,3})[ ._-]*e(\d{1,4})(?:(?:[ ._-]*e|[ ._]*-[ ._]*e?)(\d{1,4}))?",
    )
    .unwrap()
});
static YEAR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:^|[ ._(])((?:19\d{2}|20[0-3]\d))(?:[ ._)\-]|$)").unwrap());
static RELEASE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:^|[ ._\-])(?:2160p|1080[pi]|720p|480p|german|english|multi|dl|dual|eac3|aac|dts|ac3|bluray|bdrip|webrip|web[ ._-]?dl|web|h26[45]|x26[45]|hevc|hdr|dv|sample)(?:[ ._\-]|$)").unwrap()
});
static ANIME_TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[[^\]]*\]").unwrap());
static ANIME_EPISODE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
    r"(?i)^(.*?)(?:[ ._-]+e(?:p(?:isode)?)?[ ._-]*|\s+-\s+)(\d{1,4}(?:\.\d{1,2})?)(?:v\d+)?(?:[ ._]*-[ ._]*(?:e(?:p)?[ ._]*)?(\d{1,4}))?(?:$|[ ._\[(])"
).unwrap()
});
static BARE_EPISODE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(\d{1,4}(?:\.\d{1,2})?)(?:v\d+)?(?:-(\d{1,4}))?(?:$|[ ._\[(])").unwrap()
});
static SPECIAL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
    r"(?i)(?:^|[ ._(])(ova|ona|oad|specials?|spezial|sp|ncop|nced|op|ed)(?:[ ._-]*(\d{1,3}))?(?:$|[ ._\[(])"
).unwrap()
});
static SEASON_FOLDER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^(?:season|staffel|s)[ ._-]*(\d{1,3})$").unwrap());
static ANIME_MARKER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)(?:^|[ ._\[(-])anime(?:$|[ ._\])-])").unwrap());
/// Detect anime directories and explicit anime release markers.
pub fn is_anime(path: &std::path::Path) -> bool {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    path.parent().is_some_and(|p| {
        p.components().any(|part| {
            part.as_os_str()
                .to_str()
                .is_some_and(|s| s.eq_ignore_ascii_case("anime"))
        })
    }) || (ANIME_MARKER.is_match(&name) && RELEASE.is_match(&name))
}
/// Samples and trailers are auxiliary files, never episodes in the library.
pub fn is_extra(path: &std::path::Path) -> bool {
    path.components().any(|p| {
        p.as_os_str()
            .to_str()
            .is_some_and(|s| s.eq_ignore_ascii_case("sample"))
    }) || path.file_stem().is_some_and(|s| {
        s.to_string_lossy()
            .to_ascii_lowercase()
            .split(['.', '_', ' '])
            .any(|s| matches!(s, "sample" | "trailer"))
    })
}
/// Parse explicit season, absolute, range, fractional and special episode hints.
pub fn parse_anime(name: &str) -> Scene {
    let cleaned = ANIME_TAG.replace_all(name, "");
    let mut scene = parse(cleaned.trim());
    let stem = std::path::Path::new(cleaned.trim())
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(cleaned.trim())
        .trim();
    if scene.episode.is_none() {
        let numbered = ANIME_EPISODE
            .captures(stem)
            .map(|c| {
                (
                    c[1].to_string(),
                    c[2].to_string(),
                    c.get(3).map(|m| m.as_str().to_string()),
                )
            })
            .or_else(|| {
                BARE_EPISODE.captures(stem).map(|c| {
                    (
                        String::new(),
                        c[1].to_string(),
                        c.get(2).map(|m| m.as_str().to_string()),
                    )
                })
            });
        if let Some((title, number, end)) = numbered {
            scene = parse(&title);
            scene.episode = number.split('.').next().and_then(|n| n.parse().ok());
            scene.episode_end = end
                .and_then(|n| n.parse().ok())
                .filter(|end| Some(*end) > scene.episode);
            scene.episode_label = Some(match scene.episode_end {
                Some(end) => format!("{}-{end}", scene.episode.unwrap()),
                None => number.parse::<f64>().unwrap().to_string(),
            });
        }
    }
    if let Some(c) = SPECIAL.captures(stem)
        && scene.season.is_none()
    {
        scene.season = Some(0);
        if scene.episode.is_none() {
            scene.episode = c.get(2).and_then(|m| m.as_str().parse().ok());
            scene.title = parse(&stem[..c.get(0).unwrap().start()]).title;
        }
    }
    scene
}
/// Combine filename hints with show and season directories, without assigning absolute seasons.
pub fn parse_anime_path(path: &std::path::Path) -> Scene {
    let mut scene = parse_anime(&path.file_name().unwrap_or_default().to_string_lossy());
    let ancestors: Vec<_> = path
        .parent()
        .into_iter()
        .flat_map(|p| p.ancestors())
        .take_while(|p| {
            !p.file_name()
                .is_some_and(|n| n.to_str().is_some_and(|s| s.eq_ignore_ascii_case("anime")))
        })
        .filter_map(|p| p.file_name().and_then(|s| s.to_str()))
        .collect();
    if ancestors.iter().any(|s| {
        matches!(
            s.to_ascii_lowercase().as_str(),
            "movies" | "films" | "filme"
        )
    }) {
        let mut movie = parse(
            &ANIME_TAG.replace_all(&path.file_name().unwrap_or_default().to_string_lossy(), ""),
        );
        movie.season = None;
        movie.episode = None;
        movie.episode_end = None;
        movie.episode_label = None;
        return movie;
    }
    let specials_folder = ancestors.iter().any(|s| s.eq_ignore_ascii_case("specials"));
    if specials_folder && scene.season.is_none() {
        scene.season = Some(0);
    }
    if scene.season.is_none() {
        scene.season = ancestors
            .iter()
            .find_map(|s| SEASON_FOLDER.captures(s).and_then(|c| c[1].parse().ok()));
    }
    let show = ancestors.iter().find(|s| {
        !SEASON_FOLDER.is_match(s)
            && !s.eq_ignore_ascii_case("specials")
            && parse_anime(s).episode.is_none()
    });
    if (scene.episode.is_some() || scene.season == Some(0))
        && (scene.title.is_empty()
            || specials_folder
            || scene
                .title
                .split_once('-')
                .is_some_and(|(prefix, _)| !prefix.contains(' ')))
        && let Some(show) = show
    {
        scene.title = parse(show).title;
    }
    scene
}
/// Deterministic filename hints; punctuation is not inferred.
#[derive(Debug, PartialEq)]
pub struct Scene {
    /// Display title from local metadata, enrichment or filename hints.
    pub title: String,
    /// Release year when known.
    pub year: Option<i64>,
    /// Season number; zero denotes specials.
    pub season: Option<i64>,
    /// Episode number when known.
    pub episode: Option<i64>,
    /// Last episode covered by a combined source file.
    pub episode_end: Option<i64>,
    /// Original absolute/fractional/range numbering for display and sorting.
    pub episode_label: Option<String>,
}
/// Extract title, release year and SxxExx hints while stripping recognized release suffixes.
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
        episode: episode.as_ref().and_then(|c| c[2].parse().ok()),
        episode_end: episode
            .as_ref()
            .and_then(|c| c.get(3))
            .and_then(|m| m.as_str().parse().ok()),
        episode_label: episode
            .and_then(|c| c.get(3).map(|end| format!("{}-{}", &c[2], end.as_str()))),
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

#[cfg(test)]
mod anime_tests {
    use super::*;
    #[test]
    fn fansub_hints_preserve_numbering_and_movies() {
        let s = parse_anime("[SubsPlease] Frieren - 028v2 [1080p] [ABCDEF12].mkv");
        assert_eq!(s.title, "Frieren");
        assert_eq!((s.season, s.episode), (None, Some(28)));
        let s = parse_anime("[Group] Frieren.S02E03.1080p.mkv");
        assert_eq!(
            (s.title.as_str(), s.season, s.episode),
            ("Frieren", Some(2), Some(3))
        );
        let s = parse_anime("[Group] Your.Name.2016.1080p.mkv");
        assert_eq!(
            (s.title.as_str(), s.year, s.episode),
            ("Your Name", Some(2016), None)
        );
        assert_eq!(
            parse_anime("[Group] Show - 01.5 [1080p].mkv")
                .episode_label
                .as_deref(),
            Some("1.5")
        );
        assert_eq!(
            parse_anime("[Group] Show - 01-02 [1080p].mkv").episode_end,
            Some(2)
        );
        assert_eq!(parse("Film - 01.mkv").episode, None);
        assert!(is_anime(std::path::Path::new(
            "/media/Anime/Frieren/episode.mkv"
        )));
        assert!(!is_anime(std::path::Path::new("/media/movies/Anime.mkv")));
    }
}

#[cfg(test)]
mod full_anime_tests {
    use super::*;
    #[test]
    fn actual_library_formats_and_folder_hints() {
        for (name, title, season, episode) in [
            ("Pokemon.E394.mkv", "Pokemon", None, 394),
            ("Bleach.S16.E07.mkv", "Bleach", Some(16), 7),
            (
                "Dragon Ball Z Kai - E073.mkv",
                "Dragon Ball Z Kai",
                None,
                73,
            ),
            (
                "Attack.on.Titan.Ep.17.German.1080p.mkv",
                "Attack on Titan",
                None,
                17,
            ),
            ("[Group] Frieren - 028v2 [1080p].mkv", "Frieren", None, 28),
        ] {
            let s = parse_anime(name);
            assert_eq!(
                (s.title.as_str(), s.season, s.episode),
                (title, season, Some(episode)),
                "{name}"
            );
        }
        let s = parse_anime_path(std::path::Path::new(
            "/media/anime/Dragonball Classic/stars-dragonball.r.e050.1080p.mkv",
        ));
        assert_eq!(
            (s.title.as_str(), s.episode),
            ("Dragonball Classic", Some(50))
        );
        let s = parse_anime_path(std::path::Path::new("/media/anime/Frieren/Season 2/03.mkv"));
        assert_eq!(
            (s.title.as_str(), s.season, s.episode),
            ("Frieren", Some(2), Some(3))
        );
        let s = parse_anime("[Group] Show - 12-13 [1080p].mkv");
        assert_eq!(
            (s.episode, s.episode_end, s.episode_label.as_deref()),
            (Some(12), Some(13), Some("12-13"))
        );
        let s = parse_anime("Show OVA 02.mkv");
        assert_eq!(
            (s.title.as_str(), s.season, s.episode),
            ("Show", Some(0), Some(2))
        );
        assert!(is_extra(std::path::Path::new(
            "/media/anime/Show/Sample/Show.E01.mkv"
        )));
        assert!(is_extra(std::path::Path::new(
            "/media/anime/Show.E01.sample.mkv"
        )));
        assert!(!is_extra(std::path::Path::new(
            "/media/anime/Sample People/Show.E01.mkv"
        )));
        assert_eq!(parse_anime("Your Name (2016).mkv").episode, None);
        let special = parse_anime_path(std::path::Path::new(
            "/media/anime/Show/Specials/Interview.mkv",
        ));
        assert_eq!((special.title.as_str(), special.season), ("Show", Some(0)));
        assert_eq!(
            parse_anime_path(std::path::Path::new("/media/anime/Movies/Film - 02.mkv")).episode,
            None
        );
    }
}
