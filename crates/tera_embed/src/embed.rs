// TODO: Is there a way to do this with fewer #[cfg]s?

#[cfg(debug_assertions)]
mod dev;
#[cfg(debug_assertions)]
pub use dev::TeraEmbed;

#[cfg(not(debug_assertions))]
mod release;
#[cfg(not(debug_assertions))]
pub use release::TeraEmbed;


