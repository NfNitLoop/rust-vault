//! Used in release mode. 
//! Loads tera files once, and re-uses them forever.

use std::{marker::PhantomData, sync::Arc};

use rust_embed::RustEmbed;
use tera::Tera;


pub struct TeraEmbed<R: RustEmbed> {
    arc_tera: Arc<Tera>,
    _embed: PhantomData<R>
}

impl <R: RustEmbed> TeraEmbed<R> {
    pub fn new() -> Self {
        // Presumably, you've tested this during dev so it won't panic here.
        // Can't just save the Result, because it doesn't impl Clone.
        let arc_tera = Self::init_tera().expect("Error initializing Tera templates");

        Self { 
            arc_tera,
            _embed: PhantomData::default(),
        }
    }

    pub fn tera(&self) -> tera::Result<Arc<Tera>> {
        Ok(self.arc_tera.clone())
    }

    fn init_tera() -> tera::Result<Arc<Tera>> {
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

// Can't derive clone because R: doesn't implement Clone. 
// And there's no reason for that restriction.
impl <R: RustEmbed> Clone for TeraEmbed<R> {
    fn clone(&self) -> Self {
        Self { 
            _embed: self._embed.clone(),
            arc_tera: self.arc_tera.clone(),
        }
    }
}