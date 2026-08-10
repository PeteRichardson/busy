//! The only module that names `busylib`.
//!
//! Everything else imports the model and value types from here, so an upstream
//! module reshuffle is a one-file fix rather than a shotgun edit.

// `busy-cli` is a binary crate, so these `pub use` re-exports are not public
// API in the way they would be for a library: an item with no in-crate
// consumer is just an unused import as far as clippy is concerned. This
// module is a deliberate façade that concentrates all `busylib` coupling in
// one file, so its re-export list is written for the whole crate up front
// and may briefly outrun its first consumer as later tasks land. Silence
// that lag here rather than trimming the list or reaching for a
// crate-level allow.
#[allow(unused_imports)]
pub use busylib::model::assets::{
    Align, DisplayElement, DisplayElements, Font, Lifetime, Screen, TextElement,
};
#[allow(unused_imports)]
pub use busylib::types::app_name::AppName;
pub use busylib::types::color::Color;
#[allow(unused_imports)]
pub use busylib::types::element_id::ElementId;
#[allow(unused_imports)]
pub use busylib::types::priority::Priority;
#[allow(unused_imports)]
pub use busylib::types::text::Text;
