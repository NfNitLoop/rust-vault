use std::{sync::Arc, time::Duration};

use async_std::sync::Mutex;
use futures::FutureExt;
use serde::Serialize;
use tera_embed::{TeraEmbed, TideTeraRender, rust_embed::{self, RustEmbed}};

#[derive(Clone)]
struct AppState {
    templates: TeraEmbed<Templates>,

    // Used to (less-than-gracefully) stop the server.
    // See: https://github.com/http-rs/tide/issues/528
    stopper: Arc<Mutex<stop_token::StopSource>>
}

#[derive(RustEmbed)]
#[folder = "templates"]
struct Templates;

type AppRequest = tide::Request<AppState>;


fn main() -> tide::Result<()> {
    async_std::task::block_on(async_run_server())
}

async fn async_run_server() -> tide::Result<()> {
    // tide::log::start();

    let stopper = stop_token::StopSource::new();
    let stop = stopper.token();

    let state = AppState {
        templates: TeraEmbed::new(),
        stopper: Arc::new(Mutex::new(stopper))
    };

    let mut app = tide::with_state(state);

    app.at("/:name").get(|req: AppRequest| async move {
        let tera = req.state().templates.tera()?;
        tera.body("hello.html", Greet {
            name: req.param("name")?.into(),
        })
    });

    app.at("/shutdown").get(|req: AppRequest| async move {
        let tera = req.state().templates.tera()?;
        let stopper = req.state().stopper.clone();

        async_std::task::spawn(async move {
            async_std::task::sleep(Duration::from_millis(500)).await;
            println!("User requested shutdown.");
            let mut lock = stopper.lock().await;
            
            // Replace w/ a new, unrelated stopper, to let the old stopper stop.
            let stopper = std::mem::replace(&mut *lock, stop_token::StopSource::new());
            drop(stopper);
        });

        tera.body("message.html", Message{
            message: "The server will now shut down.".into()
        })
    });

    let host_and_port = "127.0.0.1:8080";

    let server = app.listen(host_and_port);

    let url = format!("http://{}/Bob", host_and_port);

    println!("Server running at: {}", &url);
    match webbrowser::open(&url) {
        Ok(_) => {},
        Err(_) => {
            println!("Couldn't open browser.");
        }
    }

    let server = server.fuse();
    let stop = stop.fuse();

    futures::pin_mut!(server, stop);

    // Hacky way to shut down the server.
    // TODO: Change when either:
    // * Tide supports graceful shutdowns: https://github.com/http-rs/tide/issues/528
    // * OR: stop-token fixes the FutureExt bug: https://github.com/async-rs/stop-token/issues/12
    futures::select! {
        result = server => {
            println!("Server error.");
            return Ok(result?);
        },
        _ = stop => {
            return Ok(());
        }
    };
}



#[derive(Serialize)]
struct Greet {
    name: String,
}

#[derive(Serialize)]
struct Message {
    message: String,
}
