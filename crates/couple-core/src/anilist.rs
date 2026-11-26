//! Recherche d'animés sur AniList (API GraphQL publique, sans clé).
//!
//! Le cœur ne fait pas de réseau : il prépare la requête et lit la réponse.
//! L'appli envoie la requête (`src-tauri/src/animes.rs`). Une réponse
//! inattendue donne une erreur ou des résultats ignorés, jamais un plantage.

use serde::Deserialize;
use serde_json::json;

use crate::{animes::AnimeInfo, Error, Result};

/// Adresse de l'API.
pub const ENDPOINT: &str = "https://graphql.anilist.co";
/// Nombre de résultats demandés.
pub const PER_PAGE: u32 = 12;
/// Longueur maximale du texte recherché, en caractères.
pub const MAX_QUERY: usize = 100;

/// Recherche par titre, animés seulement, jamais de contenu pour adultes.
const SEARCH: &str = "query ($search: String, $perPage: Int) {
  Page(perPage: $perPage) {
    media(search: $search, type: ANIME, isAdult: false, sort: SEARCH_MATCH) {
      id
      title { romaji english native }
      coverImage { large color }
      format
      seasonYear
      startDate { year }
      genres
      episodes
    }
  }
}";

/// Un seul animé, par son identifiant (pour mettre à jour ses informations,
/// par exemple le nombre d'épisodes d'un animé encore en diffusion). Même
/// forme de réponse que la recherche.
const BY_ID: &str = "query ($id: Int) {
  Page(perPage: 1) {
    media(id: $id, type: ANIME) {
      id
      title { romaji english native }
      coverImage { large color }
      format
      seasonYear
      startDate { year }
      genres
      episodes
    }
  }
}";

/// Corps JSON de la requête qui relit un animé par son identifiant AniList.
pub fn by_id_body(anilist_id: i64) -> Result<String> {
    if anilist_id <= 0 || anilist_id > i64::from(i32::MAX) {
        return Err(Error::AniList(format!(
            "identifiant invalide : {anilist_id}"
        )));
    }
    Ok(json!({ "query": BY_ID, "variables": { "id": anilist_id } }).to_string())
}

/// Corps JSON de la requête de recherche.
pub fn search_body(text: &str) -> Result<String> {
    let text = text.trim();
    if text.is_empty() {
        return Err(Error::EmptyAnime);
    }
    if text.chars().count() > MAX_QUERY {
        return Err(Error::AnimeTitleTooLong(MAX_QUERY));
    }
    Ok(json!({
        "query": SEARCH,
        "variables": { "search": text, "perPage": PER_PAGE },
    })
    .to_string())
}

#[derive(Deserialize)]
struct Response {
    data: Option<Data>,
    #[serde(default)]
    errors: Vec<ApiError>,
}

#[derive(Deserialize)]
struct ApiError {
    #[serde(default)]
    message: String,
}

#[derive(Deserialize)]
struct Data {
    #[serde(rename = "Page")]
    page: Page,
}

#[derive(Deserialize)]
struct Page {
    #[serde(default)]
    media: Vec<serde_json::Value>,
}

#[derive(Deserialize)]
struct Media {
    id: i64,
    title: Titles,
    #[serde(rename = "coverImage")]
    cover: Option<Cover>,
    format: Option<String>,
    #[serde(rename = "seasonYear")]
    season_year: Option<i32>,
    #[serde(rename = "startDate")]
    start_date: Option<StartDate>,
    #[serde(default)]
    genres: Option<Vec<String>>,
    episodes: Option<u32>,
}

#[derive(Deserialize)]
struct Titles {
    romaji: Option<String>,
    english: Option<String>,
    native: Option<String>,
}

#[derive(Deserialize)]
struct Cover {
    large: Option<String>,
    color: Option<String>,
}

#[derive(Deserialize)]
struct StartDate {
    year: Option<i32>,
}

/// Lit la réponse d'AniList. Un résultat illisible est ignoré.
pub fn parse_search(body: &[u8]) -> Result<Vec<AnimeInfo>> {
    let response: Response = serde_json::from_slice(body)
        .map_err(|e| Error::AniList(format!("réponse illisible ({e})")))?;
    let Some(data) = response.data else {
        let message = response
            .errors
            .into_iter()
            .map(|e| e.message)
            .find(|m| !m.is_empty())
            .unwrap_or_else(|| "réponse vide".to_owned());
        return Err(Error::AniList(message));
    };
    Ok(data
        .page
        .media
        .into_iter()
        .filter_map(|value| serde_json::from_value::<Media>(value).ok())
        .filter_map(|m| {
            let non_empty = |t: Option<String>| t.filter(|t| !t.trim().is_empty());
            // Le titre anglais est le plus souvent celui connu en France.
            let title = non_empty(m.title.english).or(non_empty(m.title.romaji))?;
            let cover = m.cover.unwrap_or(Cover {
                large: None,
                color: None,
            });
            AnimeInfo {
                anilist_id: Some(m.id),
                title,
                title_native: m.title.native.unwrap_or_default(),
                cover_url: cover.large.unwrap_or_default(),
                color: cover.color.unwrap_or_default(),
                format: m.format.unwrap_or_default(),
                year: m.season_year.or(m.start_date.and_then(|d| d.year)),
                genres: m.genres.unwrap_or_default(),
                episodes: m.episodes,
            }
            .cleaned()
            .ok()
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_the_request() {
        let body: serde_json::Value =
            serde_json::from_str(&search_body(" Frieren ").unwrap()).unwrap();
        assert_eq!(body["variables"]["search"], "Frieren");
        assert_eq!(body["variables"]["perPage"], PER_PAGE);
        assert!(body["query"].as_str().unwrap().contains("isAdult: false"));
        assert!(matches!(search_body("  "), Err(Error::EmptyAnime)));
        assert!(search_body(&"a".repeat(MAX_QUERY + 1)).is_err());
    }

    #[test]
    fn builds_the_by_id_request() {
        let body: serde_json::Value = serde_json::from_str(&by_id_body(154_587).unwrap()).unwrap();
        assert_eq!(body["variables"]["id"], 154_587);
        assert!(by_id_body(0).is_err());
        assert!(by_id_body(i64::MAX).is_err());
    }

    #[test]
    fn reads_a_response() {
        let body = r##"{"data":{"Page":{"media":[
          {"id":154587,"title":{"romaji":"Sousou no Frieren","english":"Frieren: Beyond Journey's End","native":"葬送のフリーレン"},
           "coverImage":{"large":"https://s4.anilist.co/file/anilistcdn/media/anime/cover/medium/bx154587-n1fmjRv4JQUd.jpg","color":"#d6f1a1"},
           "format":"TV","seasonYear":2023,"startDate":{"year":2023},"genres":["Adventure","Drama","Fantasy"],"episodes":28},
          {"id":2,"title":{"romaji":"Kimi no Na wa.","english":null,"native":null},
           "coverImage":{"large":"https://evil.fr/a.jpg","color":null},
           "format":"MOVIE","seasonYear":null,"startDate":{"year":2016},"genres":null,"episodes":1},
          {"id":3,"title":{"romaji":null,"english":"","native":"題名"},"coverImage":null},
          {"id":"pas un nombre"}
        ]}}}"##;
        let results = parse_search(body.as_bytes()).unwrap();
        assert_eq!(results.len(), 2);
        let frieren = &results[0];
        assert_eq!(frieren.anilist_id, Some(154_587));
        assert_eq!(frieren.title, "Frieren: Beyond Journey's End");
        assert_eq!(frieren.title_native, "葬送のフリーレン");
        assert!(frieren.cover_url.starts_with("https://s4.anilist.co/"));
        assert_eq!(frieren.episodes, Some(28));
        assert_eq!(frieren.genres.len(), 3);
        // Sans titre anglais : le titre romaji ; affiche hors d'AniList refusée.
        let film = &results[1];
        assert_eq!(film.title, "Kimi no Na wa.");
        assert_eq!(film.cover_url, "");
        assert_eq!(film.year, Some(2016));
    }

    #[test]
    fn reports_api_errors() {
        let err = parse_search(
            br#"{"data":null,"errors":[{"message":"Too Many Requests.","status":429}]}"#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("Too Many Requests"));
        assert!(parse_search(b"<html>").is_err());
    }
}
