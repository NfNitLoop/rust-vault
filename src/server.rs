use std::{borrow::Cow, sync::Arc, time::Duration};

use async_std::sync::Mutex;
use comrak::{ComrakOptions, markdown_to_html};
use serde::{Serialize, Deserialize};

use stop_token::future::FutureExt as _;
use tera_embed::{TeraEmbed, TideTeraRender, rust_embed::{self, RustEmbed}};

use crate::{VaultOpts, statics};

#[derive(Clone)]
struct AppState {
    templates: TeraEmbed<Templates>,

    // Used to (less-than-gracefully) stop the server.
    // See: https://github.com/http-rs/tide/issues/528
    stopper: Arc<Mutex<stop_token::StopSource>>,
    nav: Vec<NavItem>,
    markdown_opts: ComrakOptions,
}

type AppRequest = tide::Request<AppState>;

trait RequestExt {
    fn page(&self, title: impl Into<Cow<'static,str>>) -> Page;
    fn render(&self, template_name: &str, params: impl serde::Serialize) -> tide::Result<tide::Body>;
    fn render_markdown(&self, md: &str) -> String;
}

impl RequestExt for AppRequest {
    fn page(&self, title: impl Into<Cow<'static,str>>) -> Page {
        Page::new(self, title)
    }

    fn render(&self, template_name: &str, params: impl serde::Serialize) -> tide::Result<tide::Body> {
        let tera = self.state().templates.tera()?;
        tera.body(template_name, params)
    }

    fn render_markdown(&self, md: &str) -> String {
        markdown_to_html(md, &self.state().markdown_opts)
    }
}

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
        markdown_opts: ComrakOptions::default(),
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

    app.at("/").get(|req: AppRequest| async move {
        req.render("write.html", Write {
            page: req.page("Write"),
            post: String::new(),
            preview_html: String::new(),
        })
    });

    app.at("/").post(|mut req: AppRequest| async move {
        let WritePost{post, preview, submit} = req.body_form().await?;
        println!("preview: {:?}", preview);
        println!("submit: {:?}", submit);

        let preview_html = if preview.is_some() {
            req.render_markdown(&post)
        } else {
            String::new()
        };

        req.render("write.html", Write {
            page: req.page("Write"),
            post: post,
            preview_html,
        })
    });

    app.at("/:name").get(|req: AppRequest| async move {
        req.render("hello.html", Greet {
            name: req.param("name")?.into(),
            page: req.page("Greeting")
        })
    });

    app.at("/shutdown").get(|req: AppRequest| async move {
        let stopper = req.state().stopper.clone();

        async_std::task::spawn(async move {
            async_std::task::sleep(Duration::from_millis(500)).await;
            println!("User requested shutdown.");
            let mut lock = stopper.lock().await;
            
            // Replace w/ a new, unrelated stopper, to let the old stopper stop.
            let stopper = std::mem::replace(&mut *lock, stop_token::StopSource::new());
            drop(stopper);
        });

        req.render("message.html", Message{
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

    match server.until(stop).await {
        Ok(server_result) => {
            println!("Server error.");
            return Ok(server_result?);
        },
        Err(_io_err) =>  {
            // User requested server stop.
            return Ok(())
        }
    }
}


#[derive(Serialize)]
struct Greet {
    page: Page,
    name: String,
}

#[derive(Serialize)]
struct Write {
    page: Page,
    preview_html: String,
    post: String,
}

#[derive(Deserialize)]
struct WritePost {
    post: String,
    preview: Option<String>,
    submit: Option<String>,
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