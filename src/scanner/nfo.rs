//! Local NFO XML deserialization with declaration rejection; callers must bound input size.

use serde::Deserialize;

/// Movie or episode metadata from a local NFO document; absent fields default.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Nfo {
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
