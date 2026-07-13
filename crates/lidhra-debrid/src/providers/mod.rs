//! Provider adapters. One module per debrid service; each implements
//! [`crate::DebridProvider`]. Real-Debrid is fully wired and tested; AllDebrid,
//! TorBox, and Premiumize are implemented against their public APIs (verify field
//! paths against a live account). Add more the same way, then list them in
//! [`crate::registry`].

mod alldebrid;
mod premiumize;
mod real_debrid;
mod torbox;

pub use alldebrid::AllDebrid;
pub use premiumize::Premiumize;
pub use real_debrid::RealDebrid;
pub use torbox::TorBox;
