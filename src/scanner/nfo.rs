//! Local NFO XML deserialization with declaration rejection; callers must bound input size.

use serde::Deserialize;

/// Movie or episode metadata from a local NFO document; absent fields default.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Nfo {
    /// Explicit movie root prevents a numbered movie title becoming an episode.
    #[serde(skip)]
    pub is_movie: bool,
    /// Explicit TMDB provider identifier from a movie or tvshow NFO.
    pub tmdbid: Option<i64>,
    /// Standard Kodi provider identifiers.
    pub uniqueid: Vec<UniqueId>,
    /// Display title from local metadata, enrichment or filename hints.
    pub title: String,
    /// Director credits in the source NFO.
    pub director: Vec<String>,
    /// Actor credits in the source NFO.
    pub actor: Vec<Actor>,
    /// Optional rating on a ten-point scale.
    pub rating: Option<f64>,
    /// Genre names from local metadata.
    pub genre: Vec<String>,
    /// Optional named collection.
    pub set: Option<NfoSet>,
    /// Plot summary from local or remote metadata.
    pub plot: String,
    /// Release year when known.
    pub year: Option<i64>,
    /// NFO runtime in minutes.
    pub runtime: Option<f64>,
    /// Display name of the parent show.
    pub showtitle: Option<String>,
    /// Season number; zero denotes specials.
    pub season: Option<i64>,
    /// Episode number when known.
    pub episode: Option<i64>,
}
/// A Kodi NFO provider identifier.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct UniqueId {
    /// Provider name such as tmdb.
    #[serde(rename = "@type")]
    pub provider: String,
    /// Numeric provider identifier.
    #[serde(rename = "$text")]
    pub value: String,
}
impl Nfo {
    /// Use positive explicit TMDB identifiers; reject malformed or unrelated IDs.
    pub fn tmdb_id(&self) -> Option<i64> {
        self.tmdbid
            .or_else(|| {
                self.uniqueid
                    .iter()
                    .find(|id| id.provider.eq_ignore_ascii_case("tmdb"))
                    .and_then(|id| id.value.parse().ok())
            })
            .filter(|id| *id > 0)
    }
}
/// An actor credit extracted from NFO metadata.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Actor {
    /// Display name of this metadata entry.
    pub name: String,
}
/// A named collection extracted from an NFO set element.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct NfoSet {
    /// Display name of this metadata entry.
    pub name: String,
    /// Older NFO writers put the collection name directly inside the set element.
    #[serde(rename = "$text")]
    pub text: String,
}
/// Deserialize NFO XML, rejecting DOCTYPE and ENTITY declarations.
///
/// # Errors
/// Returns an error for rejected declarations, malformed XML or invalid field values.
/// The caller must enforce an input-size limit before invoking this function.
pub fn parse(xml: &str) -> anyhow::Result<Nfo> {
    // No DTD/external entity resolution; reject declarations rather than accepting an
    // ambiguous document. Input size is bounded by the caller before deserialization.
    anyhow::ensure!(
        !xml.contains("<!DOCTYPE") && !xml.contains("<!ENTITY"),
        "NFO declarations are not supported"
    );
    let mut nfo: Nfo = quick_xml::de::from_str(xml)?;
    let mut reader = quick_xml::Reader::from_str(xml);
    loop {
        match reader.read_event()? {
            quick_xml::events::Event::Start(e) | quick_xml::events::Event::Empty(e) => {
                nfo.is_movie = e.name().as_ref() == b"movie";
                break;
            }
            quick_xml::events::Event::Eof => break,
            _ => {}
        }
    }
    Ok(nfo)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn movie_and_episode() {
        let plain = parse("<movie><set>Alien Collection</set></movie>").unwrap();
        assert_eq!(plain.set.unwrap().text, "Alien Collection");
        let nested = parse("<movie><set><name>Alien Collection</name></set></movie>").unwrap();
        assert_eq!(nested.set.unwrap().name, "Alien Collection");
        let movie = parse("<movie><title>A &amp; B</title><year>2024</year><plot><![CDATA[<story>]]></plot><runtime>92</runtime></movie>").unwrap();
        assert_eq!(movie.title, "A & B");
        assert_eq!(movie.year, Some(2024));
        assert!(movie.is_movie);
        assert_eq!(parse(r#"<?xml version="1.0"?><tvshow><title>Alias</title><uniqueid type="tmdb">42</uniqueid></tvshow>"#).unwrap().tmdb_id(),Some(42));
        assert_eq!(movie.plot, "<story>");
        let ep = parse("<episodedetails><title>Pilot</title><showtitle>Series</showtitle><season>1</season><episode>2</episode></episodedetails>").unwrap();
        assert_eq!(ep.episode, Some(2));
        assert!(parse("<!DOCTYPE movie><movie/>").is_err());
        assert!(parse("<movie><year>invalid</year></movie>").is_err());
    }
}
