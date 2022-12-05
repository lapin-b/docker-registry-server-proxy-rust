use axum::{http::Request, middleware::Next, response::Response};
use once_cell::sync::Lazy;
use regex::{Regex, Captures};

const REPLACE_REGEX: Lazy<Regex> = Lazy::new(|| {
    regex::Regex::new("^/v2/(?P<isProxy>proxy/)?(?P<containerRef>[a-zA-Z0-9-/<>]+)/(?P<object>blobs|manifests|tags)(?P<rest>/.*)?$")
        .unwrap()
});

pub async fn rewrite_container_part_url<B>(mut req: Request<B>, next: Next<B>) -> Response {
    let uri = req.uri_mut();

    *uri = REPLACE_REGEX.replace(&*uri.to_string(), |captures: &Captures| {
        println!("{:?}", captures);

        format!(
            "/v2/{}{}/{}{}",
            captures.name("isProxy").map(|m| m.as_str()).unwrap_or(""),
            captures.name("containerRef").unwrap().as_str().replace("/", "~"),
            captures.name("object").unwrap().as_str(),
            captures.name("rest").unwrap().as_str()
        )
    }).parse().unwrap();

    next.run(req).await
}
