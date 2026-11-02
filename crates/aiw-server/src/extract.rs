//! Request extractors that refuse in the API's own error shape.
//!
//! axum's `Json`, `Query` and `Path` answer a malformed request with `422` and a plain-text
//! English body. The UI (and every other client) expects `{"error": "<Italian message>"}` with
//! the status codes of the rest of the API, so the handlers use these thin wrappers instead:
//! same tuple-struct shape, same behaviour, a different rejection. The detail the parser
//! reports (the serde message) is kept after a colon, with its common phrases in Italian.

use axum::extract::rejection::{JsonRejection, PathRejection, QueryRejection};
use axum::extract::{FromRequest, FromRequestParts, OptionalFromRequest, Request};
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::json;

/// A refused request: `{"error": message}` with `status` (400 unless the body was too large).
pub struct ApiRejection {
    status: StatusCode,
    message: String,
}

impl ApiRejection {
    fn bad_request(message: String) -> Self {
        Self { status: StatusCode::BAD_REQUEST, message }
    }
}

impl IntoResponse for ApiRejection {
    fn into_response(self) -> Response {
        (self.status, axum::Json(json!({ "error": self.message }))).into_response()
    }
}

/// The phrases serde, serde_json and the standard parsers use most, in Italian. What is not here
/// stays as the parser wrote it.
fn localize(detail: &str) -> String {
    // Applied in order: the specific phrases first, the general ones after them.
    const PHRASES: &[(&str, &str)] = &[
        ("missing field ", "campo mancante "),
        ("unknown field ", "campo sconosciuto "),
        ("unknown variant ", "variante sconosciuta "),
        ("invalid type: integer ", "tipo non valido: numero intero "),
        ("invalid type: floating point ", "tipo non valido: numero decimale "),
        ("invalid type: string ", "tipo non valido: stringa "),
        ("invalid type: boolean ", "tipo non valido: booleano "),
        ("invalid type: sequence", "tipo non valido: lista"),
        ("invalid type: map", "tipo non valido: oggetto"),
        ("invalid type: ", "tipo non valido: "),
        ("invalid value: integer ", "valore non valido: numero intero "),
        ("invalid value: string ", "valore non valido: stringa "),
        ("invalid value: ", "valore non valido: "),
        ("invalid length ", "lunghezza non valida "),
        (", expected a string", ", attesa una stringa"),
        (", expected a boolean", ", atteso un booleano"),
        (", expected a sequence", ", attesa una lista"),
        (", expected a map", ", atteso un oggetto"),
        (", expected struct ", ", atteso un oggetto "),
        (", expected u8", ", atteso un numero intero non negativo"),
        (", expected u16", ", atteso un numero intero non negativo"),
        (", expected u32", ", atteso un numero intero non negativo"),
        (", expected u64", ", atteso un numero intero non negativo"),
        (", expected usize", ", atteso un numero intero non negativo"),
        (", expected i32", ", atteso un numero intero"),
        (", expected i64", ", atteso un numero intero"),
        (", expected f64", ", atteso un numero"),
        (", expected ", ", atteso "),
        ("EOF while parsing a value", "testo JSON incompleto"),
        ("EOF while parsing an object", "oggetto JSON incompleto"),
        ("EOF while parsing a list", "lista JSON incompleta"),
        ("EOF while parsing a string", "stringa JSON incompleta"),
        ("EOF while parsing", "testo JSON incompleto"),
        ("expected ident", "testo non valido, atteso un valore JSON"),
        ("expected value", "atteso un valore JSON"),
        ("trailing characters", "caratteri in eccesso dopo il JSON"),
        ("key must be a string", "la chiave deve essere una stringa"),
        ("invalid digit found in string", "numero non valido"),
        ("cannot parse integer from empty string", "numero mancante"),
        ("number too large to fit in target type", "numero troppo grande"),
        ("number too small to fit in target type", "numero troppo piccolo"),
        ("provided string was not `true` or `false`", "serve true o false"),
        ("invalid unicode code point", "carattere unicode non valido"),
    ];
    let mut out = detail.to_string();
    for (en, it) in PHRASES {
        out = out.replace(en, it);
    }
    // `… at line 1 column 7` (serde_json) → `… (riga 1, colonna 7)`.
    if let Some((head, tail)) = out.rsplit_once(" at line ") {
        if let Some((line, col)) = tail.split_once(" column ") {
            if line.bytes().all(|b| b.is_ascii_digit()) && col.bytes().all(|b| b.is_ascii_digit()) && !line.is_empty() && !col.is_empty() {
                out = format!("{head} (riga {line}, colonna {col})");
            }
        }
    }
    out
}

fn json_rejection(rej: JsonRejection) -> ApiRejection {
    // The parser's own message, without axum's English lead-in.
    let detail = |inner: &dyn std::error::Error| localize(&inner.source().map(|s| s.to_string()).unwrap_or_else(|| inner.to_string()));
    match &rej {
        JsonRejection::JsonDataError(e) => ApiRejection::bad_request(format!("dati non validi: {}", detail(e))),
        JsonRejection::JsonSyntaxError(e) => ApiRejection::bad_request(format!("JSON non valido: {}", detail(e))),
        JsonRejection::MissingJsonContentType(_) => ApiRejection::bad_request("serve l'intestazione Content-Type: application/json".into()),
        _ if rej.status() == StatusCode::PAYLOAD_TOO_LARGE => {
            ApiRejection { status: StatusCode::PAYLOAD_TOO_LARGE, message: "il corpo della richiesta è troppo grande".into() }
        }
        _ => ApiRejection::bad_request("impossibile leggere il corpo della richiesta".into()),
    }
}

fn query_rejection(rej: QueryRejection) -> ApiRejection {
    let text = rej.body_text();
    let detail = text.strip_prefix("Failed to deserialize query string: ").unwrap_or(&text);
    ApiRejection::bad_request(format!("parametri non validi: {}", localize(detail)))
}

fn path_rejection(rej: PathRejection) -> ApiRejection {
    let text = rej.body_text();
    let detail = text.strip_prefix("Invalid URL: ").unwrap_or(&text);
    ApiRejection::bad_request(format!("indirizzo non valido: {}", localize(detail)))
}

/// JSON body, and JSON response (like `axum::Json`).
pub struct Json<T>(pub T);

impl<T: DeserializeOwned, S: Send + Sync> FromRequest<S> for Json<T> {
    type Rejection = ApiRejection;
    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        <axum::Json<T> as FromRequest<S>>::from_request(req, state).await.map(|axum::Json(v)| Json(v)).map_err(json_rejection)
    }
}

/// `Option<Json<T>>`: no `Content-Type` means no body; a body that is there must be valid.
impl<T: DeserializeOwned, S: Send + Sync> OptionalFromRequest<S> for Json<T> {
    type Rejection = ApiRejection;
    async fn from_request(req: Request, state: &S) -> Result<Option<Self>, Self::Rejection> {
        <axum::Json<T> as OptionalFromRequest<S>>::from_request(req, state)
            .await
            .map(|o| o.map(|axum::Json(v)| Json(v)))
            .map_err(json_rejection)
    }
}

impl<T: Serialize> IntoResponse for Json<T> {
    fn into_response(self) -> Response {
        axum::Json(self.0).into_response()
    }
}

/// Query string (`?a=1&b=2`).
pub struct Query<T>(pub T);

impl<T: DeserializeOwned, S: Send + Sync> FromRequestParts<S> for Query<T> {
    type Rejection = ApiRejection;
    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        axum::extract::Query::<T>::from_request_parts(parts, state).await.map(|axum::extract::Query(v)| Query(v)).map_err(query_rejection)
    }
}

/// Parameters captured by the route (`/api/sessions/{id}`).
pub struct Path<T>(pub T);

impl<T: DeserializeOwned + Send, S: Send + Sync> FromRequestParts<S> for Path<T> {
    type Rejection = ApiRejection;
    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        axum::extract::Path::<T>::from_request_parts(parts, state).await.map(|axum::extract::Path(v)| Path(v)).map_err(path_rejection)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_phrases_come_out_in_italian() {
        assert_eq!(localize("missing field `rules` at line 1 column 2"), "campo mancante `rules` (riga 1, colonna 2)");
        assert_eq!(
            localize("rules[0].action: invalid type: integer `5`, expected a string at line 1 column 30"),
            "rules[0].action: tipo non valido: numero intero `5`, attesa una stringa (riga 1, colonna 30)"
        );
        assert_eq!(
            localize("max_turns: invalid value: integer `-3`, expected u32 at line 1 column 9"),
            "max_turns: valore non valido: numero intero `-3`, atteso un numero intero non negativo (riga 1, colonna 9)"
        );
        assert_eq!(localize("expected value at line 1 column 1"), "atteso un valore JSON (riga 1, colonna 1)");
        assert_eq!(localize("EOF while parsing a value at line 1 column 12"), "testo JSON incompleto (riga 1, colonna 12)");
        assert_eq!(localize("limit: invalid digit found in string"), "limit: numero non valido");
        // Unknown text is left alone.
        assert_eq!(localize("qualcosa di nuovo"), "qualcosa di nuovo");
        assert_eq!(localize("trailing at line x column y"), "trailing at line x column y");
    }
}
