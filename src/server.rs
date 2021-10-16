use std::{borrow::Cow, sync::Arc, time::Duration};

use async_std::sync::Mutex;
use futures::FutureExt;
use serde::Serialize;

use tera_embed::{TeraEmbed, TideTeraRender, rust_embed::{self, RustEmbed}};

use crate::{VaultOpts, statics};

#[derive(Clone)]
struct AppState {
    templates: TeraEmbed<Templates>,

    // Used to (less-than-gracefully) stop the server.
    // See: https://github.com/http-rs/tide/issues/528
    stopper: Arc<Mutex<stop_token::StopSource>>,
    nav: Vec<NavItem>
}

type AppRequest = tide::Request<AppState>;

#[derive(RustEmbed)]
#[folder = "templates"]
struct Templates;

#[derive(RustEmbed)]
#[folder = "static"]
struct Statics;

pub(crate) async fn async_run_server(opts: &VaultOpts) -> anyhow::Result<()> {
    if opts.verbose > 0 {
        tide::log::start();
    }

    let stopper = stop_token::StopSource::new();
    let stop = stopper.token();

    let state = AppState {
        templates: TeraEmbed::new(),
        stopper: Arc::new(Mutex::new(stopper)),
        nav: vec![
            NavItem::new("Write", "/"),
            NavItem::new("Bob", "/Bob"),
            NavItem::new("Sally", "/Sally"),
            NavItem::hidden("Log In", "/log_in"),
            NavItem::new("Shutdown", "/shutdown"),
        ],
    };

    let mut app = tide::with_state(state);

    app.at("/:name").get(|req: AppRequest| async move {
        let tera = req.state().templates.tera()?;
        tera.body("hello.html", Greet {
            name: req.param("name")?.into(),
            page: Page::new(&req, "Greeting")
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
            page: Page::new(&req, "Shutting Down"),
            message: "The server will now shut down.".into()
        })
    });

    app.at("/static/*path").get(statics::serve::<Statics, AppState>);

    let host_and_port = "127.0.0.1:8080";

    let server = app.listen(host_and_port);

    let url = format!("http://{}/", host_and_port);

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
    page: Page,
    name: String,
}

#[derive(Serialize)]
struct Message {
    page: Page,
    message: String,
}

#[derive(Serialize)]
struct Page {
    rel_path: Cow<'static, str>,
    title: Cow<'static, str>,
    nav: Vec<NavItem>,
    // TODO: flash
}

impl Page {
    fn new(request: &AppRequest, title: impl Into<Cow<'static,str>>) -> Self {
        Self {
            rel_path: request.url().path().to_string().into(),
            nav: request.state().nav.clone(),
            title: title.into()
        }
    }
}


#[derive(Serialize, Clone)]
pub(crate) struct NavItem {
    title: Cow<'static, str>,
    link: Cow<'static, str>,
    hidden: bool,
}

impl NavItem {
    fn new(title: impl Into<Cow<'static, str>>, link: impl Into<Cow<'static, str>>) -> Self {
        Self { title: title.into(), link: link.into(), hidden: false }
    }

    fn hidden(title: impl Into<Cow<'static, str>>, link: impl Into<Cow<'static, str>>) -> Self {
        Self { hidden: true, .. Self::new(title, link)  }
    }
}