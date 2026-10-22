//! The door in front of Vellum.
//!
//! Vellum sits behind Cloudflare Access. The pad cannot sign in through a
//! browser, so it carries a service token: `RIDDLE_CF_ACCESS_ID` and
//! `RIDDLE_CF_ACCESS_SECRET` ride along as `CF-Access-Client-Id` and
//! `CF-Access-Client-Secret`, next to the device bearer Vellum checks itself.
//! The token goes only to the host named by `RIDDLE_VELLUM_BASE` — an oracle
//! pointed at a provider never sees it.
//!
//! Access says no with a redirect to its login page. Followed, that page
//! answers 200 with HTML, and every caller would read it as an empty shelf or
//! a silent oracle. So agents built here never follow redirects, and a 3xx or
//! an HTML body is a refusal with a name.

/// An agent that stops at the first redirect. Callers add their own timeouts.
pub fn builder() -> ureq::AgentBuilder {
    ureq::AgentBuilder::new().redirects(0)
}

/// Sign a request: the bearer always, the Access service token only when the
/// request goes to Vellum's host and both halves are set.
pub fn authed(req: ureq::Request, bearer: &str) -> ureq::Request {
    let req = req.set("Authorization", bearer);
    let id = std::env::var("RIDDLE_CF_ACCESS_ID").unwrap_or_default();
    let secret = std::env::var("RIDDLE_CF_ACCESS_SECRET").unwrap_or_default();
    let vellum = crate::vault::base().unwrap_or_default();
    if id.trim().is_empty() || secret.trim().is_empty() || !same_host(req.url(), &vellum) {
        return req;
    }
    req.set("CF-Access-Client-Id", id.trim())
        .set("CF-Access-Client-Secret", secret.trim())
}

/// Why Access turned the pad away, if this response is Access talking rather
/// than Vellum: a redirect (to its login page) or an HTML body (the page
/// itself, or its block screen). Vellum's own answers are JSON or SSE.
pub fn refusal(r: &ureq::Response) -> Option<String> {
    let redirect = (300..400).contains(&r.status());
    let html = r.content_type().eq_ignore_ascii_case("text/html");
    (redirect || html).then(|| {
        format!(
            "Access refused the pad ({}). Check RIDDLE_CF_ACCESS_ID and RIDDLE_CF_ACCESS_SECRET",
            r.status()
        )
    })
}

fn host(url: &str) -> Option<String> {
    let rest = url.split_once("://")?.1;
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let host = &rest[..end];
    (!host.is_empty()).then(|| host.to_ascii_lowercase())
}

fn same_host(a: &str, b: &str) -> bool {
    matches!((host(a), host(b)), (Some(x), Some(y)) if x == y)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response(raw: &str) -> ureq::Response {
        raw.parse().unwrap()
    }

    #[test]
    fn the_service_token_only_travels_to_vellum() {
        let vellum = "https://vellum.example.com";
        assert!(same_host(
            "https://vellum.example.com/api/device/v1/notes?limit=50",
            vellum
        ));
        assert!(same_host(
            "https://Vellum.Example.com/v1/chat/completions",
            vellum
        ));
        assert!(!same_host(
            "https://api.openai.com/v1/chat/completions",
            vellum
        ));
        assert!(!same_host("https://vellum.example.com.evil.net/v1", vellum));
        assert!(!same_host("https://vellum.example.com/v1", ""));
    }

    #[test]
    fn a_login_redirect_is_a_refusal() {
        let r = response(
            "HTTP/1.1 302 Found\r\nLocation: https://team.cloudflareaccess.com/login\r\n\r\n",
        );
        assert!(refusal(&r)
            .unwrap()
            .contains("Access refused the pad (302)"));
    }

    #[test]
    fn an_html_page_is_a_refusal() {
        let r = response(
            "HTTP/1.1 403 Forbidden\r\nContent-Type: text/html; charset=utf-8\r\n\r\n<html></html>",
        );
        assert!(refusal(&r).is_some());
    }

    #[test]
    fn vellum_json_and_streams_pass() {
        let json = response("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{}");
        let sse = response("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\ndata: x\n\n");
        let denied = response("HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\n\r\n{\"error\":\"device token required\"}");
        assert!(refusal(&json).is_none());
        assert!(refusal(&sse).is_none());
        assert!(refusal(&denied).is_none());
    }
}
