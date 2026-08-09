//! The only module that names `busylib`.
//!
//! Everything else imports the model and value types from here, so an upstream
//! module reshuffle is a one-file fix rather than a shotgun edit.

pub use busylib::types::color::Color;
