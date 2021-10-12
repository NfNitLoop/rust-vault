use serde::Serialize;
use tera_embed::{TeraEmbed, TideTeraRender, rust_embed::{self, RustEmbed}};
use tide_tera::prelude::*;

#[derive(Clone)]
struct AppState {
    templates: TeraEmbed<Templates>,
}

#[derive(RustEmbed)]
#[folder = "templates"]
struct Templates;

type AppRequest = tide::Request<AppState>;


#[async_std::main]
async fn main() -> tide::Result<()> {
    tide::log::start();

    let state = AppState {
        templates: TeraEmbed::new()
    };

    let mut app = tide::with_state(state);

    app.at("/:name").get(|req: AppRequest| async move {
        let tera = req.state().templates.tera()?;
        tera.body("hello.html", Greet {
            name: req.param("name")?.into(),
        })
    });

    app.listen("127.0.0.1:8080").await?;

    Ok(())
}



#[derive(Serialize)]
struct Greet {
    name: String,
}