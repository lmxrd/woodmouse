//! Cross-site request forgery protection with Fetch Metadata (OWASP's recommendation; the same rule as Go's
//! `http.CrossOriginProtection`). No crate fits: `axum_csrf` needs tokens in the frontend, and this needs
//! none, since browsers say where a request comes from.
//!
//! A request that changes something (any method but GET, HEAD and OPTIONS) is refused with 403 when:
//! - its `Sec-Fetch-Site` is there and is neither `same-origin` nor `none` (typed into the address bar), or
//! - it has no `Sec-Fetch-Site` (an older browser) but an `Origin` that is not the allowed one:
//!   `PUBLIC_URL` when it is set, else the request's own `Host` (forwarded hosts are not trusted).
//!
//! A request with neither header is not from a browser and passes, as does one with an API key
//! (`Authorization: Bearer`): a page on another site cannot send that header without a preflight we never
//! answer. No route is cross-origin by design: the single sign-on callback and the calendar feeds are GETs.

use axum::extract::Request;
use axum::http::{HeaderMap, Method, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::error::ApiError;

/// Whether a request may go on (see the module docs).
pub fn allowed(method: &Method, headers: &HeaderMap, public_url: Option<&str>) -> bool {
    if matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS) {
        return true;
    }
    let bearer = headers
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .is_some_and(|h| h.starts_with("Bearer "));
    if bearer {
        return true;
    }
    if let Some(site) = headers.get("sec-fetch-site") {
        return matches!(site.to_str(), Ok("same-origin" | "none"));
    }
    let Some(origin) = headers.get(header::ORIGIN) else {
        return true;
    };
    let Ok(origin) = origin.to_str() else {
        return false;
    };
    match public_url {
        Some(url) => url.trim_end_matches('/') == origin,
        None => {
            let origin_host = origin.split_once("://").map(|(_, rest)| rest);
            let host = headers.get(header::HOST).and_then(|h| h.to_str().ok());
            origin_host.is_some() && origin_host == host
        }
    }
}

/// The middleware on every API route.
pub async fn protect(request: Request, next: Next) -> Response {
    let public_url = crate::config::public_url();
    if allowed(request.method(), request.headers(), public_url.as_deref()) {
        next.run(request).await
    } else {
        ApiError::forbidden("Cross-site request refused").into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.insert(*name, value.parse().unwrap());
        }
        map
    }

    #[test]
    fn the_rules() {
        let post = Method::POST;
        let host = ("host", "tasks.example.com");
        assert!(allowed(
            &Method::GET,
            &headers(&[("sec-fetch-site", "cross-site")]),
            None
        ));
        assert!(allowed(
            &post,
            &headers(&[host, ("sec-fetch-site", "same-origin")]),
            None
        ));
        assert!(allowed(
            &post,
            &headers(&[("sec-fetch-site", "none")]),
            None
        ));
        assert!(!allowed(
            &post,
            &headers(&[("sec-fetch-site", "same-site")]),
            None
        ));
        assert!(!allowed(
            &post,
            &headers(&[("sec-fetch-site", "cross-site")]),
            None
        ));
        // Older browsers: the Origin decides.
        assert!(allowed(
            &post,
            &headers(&[host, ("origin", "https://tasks.example.com")]),
            None
        ));
        assert!(!allowed(
            &post,
            &headers(&[host, ("origin", "https://evil.example")]),
            None
        ));
        assert!(!allowed(&post, &headers(&[host, ("origin", "null")]), None));
        assert!(allowed(
            &post,
            &headers(&[
                ("host", "127.0.0.1:8000"),
                ("origin", "https://tasks.example.com")
            ]),
            Some("https://tasks.example.com/")
        ));
        // With PUBLIC_URL, only it decides: the Host (a proxy may rewrite it) no longer does.
        assert!(!allowed(
            &post,
            &headers(&[
                ("host", "127.0.0.1:8000"),
                ("origin", "http://127.0.0.1:8000")
            ]),
            Some("https://tasks.example.com/")
        ));
        // Not a browser.
        assert!(allowed(&post, &headers(&[host]), None));
        assert!(allowed(
            &post,
            &headers(&[
                ("sec-fetch-site", "cross-site"),
                ("authorization", "Bearer wm_x")
            ]),
            None
        ));
    }
}
