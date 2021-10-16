//! Utils for serving static files from Tide.
//! 

use tera_embed::rust_embed::RustEmbed;
use tide::Response;

pub(crate) async fn serve<RE: RustEmbed, T>(req: tide::Request<T>) -> tide::Result {
    let path = req.param("path")?;
    let file = match RE::get(path) {
        Some(file) => file,
        None => {
            return Ok(Response::builder(404).body("Not found").build());
        }
    };

    let mut response = Response::builder(200)
        // This is likely doing a lot of extra copying. Would be nice if Tide took a Cow<bytes>
        .body(file.data.as_ref());

    if let Some(guess) =  mime_guess::from_path(path).first() {
        let mut ctype = guess.to_string();
        if ctype.starts_with("text/") {
            ctype.push_str("; charset=utf-8");
        }
        response = response.header("Content-Type", ctype);
    }

    Ok(response .build())
}