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

mod embed;
pub use embed::TeraEmbed;

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