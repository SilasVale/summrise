//! The capability declarations: one module per capability, and nothing else in it.
//!
//! A capability's tools, their schemas and its manual entry live here, in `core`, because the
//! registry is read by parties that never link the device (the console's grant form, the plugin
//! host's name checks, the launcher's expectations). The implementation lives in the device's
//! `capabilities::<name>` module; this module is the declaration it implements.
//!
//! **One capability, one module.** Adding a tool means adding it to the capability's declaration and
//! its implementation — not to a shared table, a schema match and a dispatch match.

pub mod browser;
pub mod files;
pub mod memory;
pub mod terminal;

/// The capabilities whose declarations live here, in registry order.
pub const DECLARED: &[&crate::registry::Capability] = &[
    &files::CAPABILITY,
    &memory::CAPABILITY,
    &terminal::CAPABILITY,
    &browser::CAPABILITY,
];
