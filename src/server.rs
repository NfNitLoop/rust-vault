use std::{borrow::Cow, sync::Arc, time::Duration};

use anyhow::{Context};
use async_std::sync::Mutex;
use async_trait::async_trait;
use chrono::{FixedOffset, Offset};
use comrak::{ComrakOptions, markdown_to_html};
use serde::{Serialize, Deserialize};

use stop_token::future::FutureExt as _;
use tera_embed::{TeraEmbed, TideTeraRender, rust_embed::{self, RustEmbed}};
use tide::{Response, http::{Cookie}};

use crate::{OpenCommand, VaultOpts, crypto::{
        SealedBoxPrivateKey,
        SealedBoxPublicKey,
        SecretBox
    }, db::{self, Entry, VaultExt}, statics};

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
    fn get_priv_key(&self) -> anyhow::Result<Option<SealedBoxPrivateKey>>;
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

    fn get_priv_key(&self) -> anyhow::Result<Option<SealedBoxPrivateKey>> {
        let cookie = match self.cookie(PRIV_KEY_COOKIE) {
            Some(c) => c,
            None => return Ok(None),
        };
        let key_bytes = match self.decrypt_bytes(&cookie)? {
            None => return Ok(None),
            Some(b) => b,
        };

        Ok(Some(SealedBoxPrivateKey::from_bytes(&key_bytes)?))
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

    let pool = db::pool(db::options(&command.opts.sqlite_file));

    if pool.needs_upgrade().await? {
        anyhow::bail!("Database needs an upgrade");
    }

    let public_key = pool.public_key().await.context("getting public key")?;

    let stopper = stop_token::StopSource::new();
    let stop = stopper.token();

    sodiumoxide::init().map_err(|_| anyhow::format_err!("Error initializing sodiumoxide."))?;
    let state = AppState {
        db: pool,
        templates: TeraEmbed::new(),
        markdown_opts: ComrakOptions::default(),
        stopper: Arc::new(Mutex::new(stopper)),
        secret_box: SecretBox::generate(),
        public_key,
        nav: vec![
            NavItem::new("Write", "/"),
            NavItem::hidden("Log In", "/login"),
            NavItem::new("Read", "/read"),
            NavItem::new("Shutdown", "/shutdown"),
        ],
    };


    let mut app = tide::with_state(state);
    app.with(NoStore{});

    app.at("/").get(|req: AppRequest| async move {
        req.render("write.html", Write {
            page: req.page("Write"),
            post: String::new(),
            preview_html: String::new(),
        })
    });

    app.at("/").post(|mut req: AppRequest| async move {
        let WritePost{mut post, preview, submit} = req.body_form().await?;

        let mut page = req.page("Write");
        let mut preview_html = String::new();

        if submit.is_some() {
            let db = &req.state().db;
            let key = &req.state().public_key;
            let now = chrono::Local::now();
            let entry = Entry{
                timestamp_ms_utc: now.timestamp_millis(),
                offset_utc_mins: now.offset().fix().local_minus_utc() / 60,
                contents: key.encrypt(post.as_bytes()),
            };
            db.write_entry(entry).await?;
            post = String::new();
            page.flash_success("Post saved.");

        } else if preview.is_some() {
            preview_html = req.render_markdown(&post)
        } 

        req.render("write.html", Write { page, post, preview_html })
    });

    app.at("/read")
    .get(read_posts);

    app.at("/login")
    .get(|req: AppRequest| async move {
        req.render("login.html", LogIn{
            page: req.page("Log In")
        })
    })
    .post(|mut req: AppRequest| async move {
        let form: LogInForm = req.body_form().await?;
        let secret = SealedBoxPrivateKey::from_base58(&form.secret);

        match secret {
            Err(err) => println!("Bad secret. {:?}", err),
            Ok(secret) => {
                let server_key = &req.state().public_key;

                if secret.public() == server_key {
                    let mut res: Response = tide::Redirect::see_other("/read").into();
                    let cookie = req.set_priv_key(secret.bytes());
                    res.insert_cookie(cookie);
                    return Ok(res);
                } 
                println!("Login attempt with incorrect private key.");

                // TRY treating the private key as a seed.
                // The Deno version used to hand out the seed.
                if let Ok(secret) = SealedBoxPrivateKey::from_base58_seed(&form.secret) {
                    if secret.public() == server_key {
                        println!("You supplied the seed for the private key.");
                        println!("Instead, use the private key: {}", &secret);
                    }
                }
            }
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

    let host_and_port = format!("127.0.0.1:{port}", port=command.opts.port);

    let server = app.listen(&host_and_port);

    let url = format!("http://{}/", host_and_port);

    if !command.opts.no_browser {
        match webbrowser::open(&url) {
            Ok(_) => {},
            Err(_) => {
                println!("Couldn't open browser.");
            }
        }
    }

    println!("Server running at: {}", &url);

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

async fn read_posts(req: AppRequest) -> tide::Result<tide::Response> {
    if !req.logged_in() {
        let res: Response = tide::Redirect::temporary("/login").into();
        return Ok(res);
    }

    let key = req.get_priv_key()?.expect("User is logged in");

    let query: ReadQuery = req.query()?;

    let db = &req.state().db;
    let posts: anyhow::Result<Vec<Post>> = db
        .get_posts(&query)
        .await?
        .into_iter()
        .map(|e| entry_to_post(e, &req, &key))
        .collect();
    let posts = posts?;

    let mut page = req.page("Read Posts");
    let offset = query.offset.unwrap_or(0);
    let limit = query.limit.unwrap_or(50);
    if offset > 0 {
        page.previous.replace(NavItem::new(
            "Previous", 
            format!("{}?offset={}&limit={}", req.url().path(), offset.saturating_sub(limit), limit)
        ));
    }
    if !posts.is_empty() {
        page.next.replace(NavItem::new(
            "Next",
            format!("{}?offset={}&limit={}", req.url().path(), offset+limit, limit)
        ));
    }

    let posts = Posts{page, posts};
    let res: Response = match req.render("posts.html", posts) {
        Ok(body) => body.into(),
        Err(err) => err.into(),
    };
    Ok(res)
}

fn entry_to_post(entry: db::Entry, req: &AppRequest, key: &SealedBoxPrivateKey) -> anyhow::Result<Post> {
    use chrono::TimeZone;

    let markdown = key.decrypt_string(&entry.contents)?;
    let html = req.render_markdown(&markdown);

    let offset_secs = entry.offset_utc_mins * 60;
    let timestamp = FixedOffset::east(offset_secs).timestamp_millis(entry.timestamp_ms_utc);
    let timestamp = timestamp.format("%a %B %e, %Y - %T %z").to_string();

    Ok(Post{
        html,
        timestamp,
    })
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

/// The HTTP query params for the /read page.
#[derive(Deserialize)]
pub(crate) struct ReadQuery {
    pub(crate) offset: Option<usize>,

    pub(crate) limit: Option<usize>,

    // TODO:
    // #[serde(default)]
    // chronological: bool,
}

#[derive(Serialize)]
struct Message {
    page: Page,
    message: String,
}

#[derive(Serialize)]
pub(crate) struct Post {
    pub(crate) timestamp: String,
    pub(crate) html: String,
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
    flash: Option<Flash>,
    previous: Option<NavItem>,
    next: Option<NavItem>,
}

impl Page {
    fn new(request: &AppRequest, title: impl Into<Cow<'static,str>>) -> Self {
        Self {
            rel_path: request.url().path().to_string().into(),
            nav: request.state().nav.clone(),
            title: title.into(),
            flash: None,
            next: None,
            previous: None,
        }
    }

    fn flash_success(&mut self, message: impl Into<String>) {

        self.flash.replace(Flash { message: message.into(), flash_type: FlashType::SUCCESS });
    }
}

#[derive(Serialize)]
struct Flash {
    message: String,
    #[serde(rename="type")]
    flash_type: FlashType,
}

#[allow(dead_code)]
#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
enum FlashType {
    SUCCESS,
    WARNING,
    ERROR,
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


// See: https://github.com/http-rs/tide/issues/854
struct NoStore {}

#[async_trait]
impl <State: Clone + Send + Sync + 'static> tide::Middleware<State> for NoStore {
    async fn handle<'a, 'b>(&'a self, req: tide::Request<State>, next: tide::Next<'b, State>) -> tide::Result<Response>
    {
        use tide::http::cache::{CacheControl, CacheDirective};
        let mut response = next.run(req).await;

        if let None = response.header("Cache-Control") {
            let mut header = CacheControl::new();
            header.push(CacheDirective::NoStore);
            header.push(CacheDirective::MaxAge(Duration::from_secs(0)));

            response.insert_header(header.name(), header.value());
        }
        Ok(response)
    }
}
