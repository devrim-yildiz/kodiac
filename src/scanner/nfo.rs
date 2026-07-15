
use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Nfo {
    pub title: String,
    pub director: Vec<String>,
    pub actor: Vec<Actor>,
    pub rating: Option<f64>,
    pub genre: Vec<String>,
    pub set: Option<NfoSet>,
    pub plot: String,
    pub year: Option<i64>,
    pub runtime: Option<f64>,
    pub showtitle: Option<String>,
    pub season: Option<i64>,
    pub episode: Option<i64>,
}
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Actor {
    pub name: String,
}
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct NfoSet {
    pub name: String,
}
pub fn parse(xml: &str) -> anyhow::Result<Nfo> {
    // No DTD/external entity resolution; reject declarations rather than accepting an
    // ambiguous document. Input size is bounded by the caller before deserialization.
    anyhow::ensure!(
        !xml.contains("<!DOCTYPE") && !xml.contains("<!ENTITY"),
        "NFO declarations are not supported"
    );
    let nfo: Nfo = quick_xml::de::from_str(xml)?;
    Ok(nfo)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn movie_and_episode() {
        let movie = parse("<movie><title>A &amp; B</title><year>2024</year><plot><![CDATA[<story>]]></plot><runtime>92</runtime></movie>").unwrap();
        assert_eq!(movie.title, "A & B");
        assert_eq!(movie.year, Some(2024));
        assert_eq!(movie.plot, "<story>");
        let ep = parse("<episodedetails><title>Pilot</title><showtitle>Series</showtitle><season>1</season><episode>2</episode></episodedetails>").unwrap();
        assert_eq!(ep.episode, Some(2));
        assert!(parse("<!DOCTYPE movie><movie/>").is_err());
        assert!(parse("<movie><year>invalid</year></movie>").is_err());
    }
}
