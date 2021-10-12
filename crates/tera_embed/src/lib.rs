//! Tera-Embed combines the power of Tera for Jinja-like templates, plus Rust-embed so that
//! you can embed your templates into your application binary.
//! 
//! During development mode requests will dynamically load your templates from disk
//! with each request, which allows you to edit and reload templates quickly.


pub use rust_embed;
pub use tera;

use rust_embed::RustEmbed;
use tera::Tera;
use std::marker::PhantomData;

#[cfg(feature="tide-tera")]
pub mod tide_tera_ext;
#[cfg(feature="tide-tera")]
pub use tide_tera_ext::*;

pub struct TeraEmbed<R: RustEmbed> {
    _embed: PhantomData<R>,
}

impl <R: RustEmbed> TeraEmbed<R> {
    pub fn new() -> Self {
        Self { _embed: PhantomData::default() }
    }

    pub fn tera(&self) -> tera::Result<Box<Tera>> {
        let mut t = Tera::default();
        let mut templates = vec![];

        for file_name in R::iter() {
            let file = R::get(&file_name).expect("RustEmbed file should exist");
            let file_str = String::from_utf8_lossy(file.data.as_ref()).to_string();
            templates.push(
                (file_name, file_str)
            );
        }
        t.add_raw_templates(templates)?;
        Ok(Box::new(t))
    }
}

impl <R: RustEmbed> Clone for TeraEmbed<R> {
    fn clone(&self) -> Self {
        Self { _embed: self._embed.clone() }
    }
}

/// Render templates based on a struct, similar to in Askama.
/// 
/// See: <https://github.com/djc/askama>
pub trait AskamaIsh {
    fn render(&self, template_name: &str, params: impl serde::Serialize) -> tera::Result<String>; 
}

impl AskamaIsh for tera::Tera {
    fn render(&self, template_name: &str, params: impl serde::Serialize) -> tera::Result<String> {
        let ctx = tera::Context::from_serialize(params)?;
        self.render(template_name, &ctx)
    }
}