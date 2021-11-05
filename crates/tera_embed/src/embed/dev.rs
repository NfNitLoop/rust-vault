//! Used in dev mode.
//! Reloads all Tera templates with every request.

use std::{marker::PhantomData, sync::Arc};

use rust_embed::RustEmbed;
use tera::Tera;


pub struct TeraEmbed<R: RustEmbed> {
    _embed: PhantomData<R>,
}

impl <R: RustEmbed> TeraEmbed<R> {
    pub fn new() -> Self {
        Self { _embed: PhantomData::default() }
    }

    pub fn tera(&self) -> tera::Result<Arc<Tera>> {
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
        Ok(Arc::new(t))
    }
}

impl <R: RustEmbed> Clone for TeraEmbed<R> {
    fn clone(&self) -> Self {
        Self { _embed: self._embed.clone() }
    }
}