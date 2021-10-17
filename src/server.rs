use std::{borrow::Cow, sync::Arc, time::Duration};

use async_std::sync::Mutex;
use comrak::{ComrakOptions, markdown_to_html};
use serde::{Serialize, Deserialize};

use stop_token::future::FutureExt as _;
use tera_embed::{TeraEmbed, TideTeraRender, rust_embed::{self, RustEmbed}};
use tide::{Response, http::{Cookie}};

use crate::{OpenCommand, VaultOpts, crypto::{SealedBoxPrivateKey, SealedBoxPublicKey, SecretBox}, statics};

#[derive(Clone)]
struct AppState {
    templates: TeraEmbed<Templates>,

    // Used to (less-than-gracefully) stop the server.
    // See: https://github.com/http-rs/tide/issues/528
    stopper: Arc<Mutex<stop_token::StopSource>>,
    nav: Vec<NavItem>,
    markdown_opts: ComrakOptions,
    db: sqlx::SqlitePool,
    secret_box: SecretBox,

    // TODO: Just for testing. Store public key in the DB.
    public_key: SealedBoxPublicKey,
}

type AppRequest = tide::Request<AppState>;

const PRIV_KEY_COOKIE: &'static str = "login";

trait RequestExt {
    fn page(&self, title: impl Into<Cow<'static,str>>) -> Page;
    fn render(&self, template_name: &str, params: impl serde::Serialize) -> tide::Result<tide::Body>;
    fn render_markdown(&self, md: &str) -> String;

    /// An encrypted cookie. 😆
    /// Returns Err if we couldn't decrypt.
    fn decrypt_bytes(&self, cookie: &Cookie) -> anyhow::Result<Option<Vec<u8>>>;
    fn encrypt_bytes(&self, cookie: &mut Cookie, data: &[u8] );

    // If the user is logged in w/ their private key, we can decrypt posts:
    fn get_priv_key(&self) -> anyhow::Result<Option<Vec<u8>>>;
    fn logged_in(&self) -> bool;
    fn set_priv_key(&self, key: &[u8]) -> Cookie<'static>;
    // fn db(&self) -> sqlx::SqliteConnection;
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

    fn decrypt_bytes(&self, cookie: &Cookie) -> anyhow::Result<Option<Vec<u8>>> {
        let cypher = bs58::decode(cookie.value()).into_vec()?;
        let decrypted = self.state().secret_box.decrypt(&cypher)?;
        Ok(Some(decrypted))
    }

    fn encrypt_bytes(&self, cookie: &mut Cookie, data: &[u8] ) {
        let cypher = self.state().secret_box.encrypt(data);
        cookie.set_value(bs58::encode(cypher).into_string());
    }

    fn get_priv_key(&self) -> anyhow::Result<Option<Vec<u8>>> {
        let cookie = match self.cookie(PRIV_KEY_COOKIE) {
            Some(c) => c,
            None => return Ok(None),
        };
        self.decrypt_bytes(&cookie)
    }

    fn set_priv_key(&self, key: &[u8]) -> Cookie<'static> {
        let mut cookie = Cookie::build(PRIV_KEY_COOKIE, "").finish();
        self.encrypt_bytes(&mut cookie, key);
        cookie
    }

    fn logged_in(&self) -> bool {
        match self.get_priv_key() {
            Ok(Some(_key)) => true,
            _ => false,
        }
    }

    
}

#[derive(RustEmbed)]
#[folder = "templates"]
struct Templates;

#[derive(RustEmbed)]
#[folder = "static"]
struct Statics;

pub(crate) async fn async_run_server(opts: &VaultOpts, command: &OpenCommand) -> anyhow::Result<()> {
    if opts.verbose > 0 {
        tide::log::start();
    }

    let file_name = command.sqlite_file.to_str().ok_or_else(|| anyhow::format_err!("Invalid SQLite file name"))?;
    let pool = sqlx::SqlitePool::connect(file_name).await?;
    // TODO: pool.check_version()?;

    let stopper = stop_token::StopSource::new();
    let stop = stopper.token();

    let secret_key = SealedBoxPrivateKey::generate();
    println!("pub key: {}", secret_key.public());
    println!("priv key: {}", secret_key);


    sodiumoxide::init().map_err(|_| anyhow::format_err!("Error initializing sodiumoxide."))?;
    let state = AppState {
        db: pool,
        templates: TeraEmbed::new(),
        markdown_opts: ComrakOptions::default(),
        stopper: Arc::new(Mutex::new(stopper)),
        secret_box: SecretBox::generate(),
        public_key: secret_key.public().clone(),
        nav: vec![
            NavItem::new("Write", "/"),
            NavItem::hidden("Log In", "/login"),
            NavItem::new("Read", "/read"),
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

    app.at("/read")
    .get(|req: AppRequest| async move {
        if !req.logged_in() {
            let res: Response = tide::Redirect::temporary("/login").into();
            return Ok(res);
        }

        // TODO: Pagination.
        let posts = Posts{
            page: req.page("Read Posts"),
            posts: vec![]
        };
        let res: Response = match req.render("posts.html", posts) {
            Ok(body) => body.into(),
            Err(err) => err.into(),
        };
        Ok(res)
    });

    app.at("/login")
    .get(|req: AppRequest| async move {
        req.render("login.html", LogIn{
            page: req.page("Log In")
        })
    })
    .post(|mut req: AppRequest| async move {
        let form: LogInForm = req.body_form().await?;
        let secret = SealedBoxPrivateKey::from_base58(&form.secret);

        if let Ok(secret) = secret {
            if secret.public() == &req.state().public_key {
                let mut res: Response = tide::Redirect::see_other("/read").into();
                let cookie = req.set_priv_key(secret.bytes());
                res.insert_cookie(cookie);
                return Ok(res);
            } else {
                println!("Login attempt with incorrect private key.");
            }
        } else {
            println!("Bad secret.");
        }
        
        

        let body = req.render("login.html", LogIn{
            page: req.page("Log In")
        })?;
        Ok(body.into())
    }) ;

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
struct Post {
    timestamp: String,
    html: String,
}

#[derive(Serialize)]
struct Posts {
    page: Page,
    posts: Vec<Post>
}

#[derive(Serialize)]
struct LogIn {
    page: Page,
}

#[derive(Deserialize)]
struct LogInForm {
    secret: String,
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