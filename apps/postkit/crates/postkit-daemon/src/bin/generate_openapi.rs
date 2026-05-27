use utoipa::OpenApi as _;

fn main() {
    let json = postkit_daemon::ApiDoc::openapi()
        .to_pretty_json()
        .expect("failed to serialize OpenAPI spec");
    println!("{json}");
}
