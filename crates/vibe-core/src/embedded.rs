//! Embedded UI assets served at `/_vp/ui/*`.

use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "embedded/ui/"]
pub struct Ui;
