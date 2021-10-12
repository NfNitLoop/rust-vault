//! Extensions to help with using tera_embed with Tide.

/// Lets you render a tide template to a 
pub trait TideTeraRender {
    fn body(&self, template_name: &str, params: impl serde::Serialize) -> tide::Result<tide::Body>; 

}

impl <T> TideTeraRender for T where T: tide_tera::TideTeraExt {
    fn body(&self, template_name: &str, params: impl serde::Serialize) -> tide::Result<tide::Body> {
        let ctx = tera::Context::from_serialize(params)?;
        self.render_body(template_name, &ctx)
    }
}