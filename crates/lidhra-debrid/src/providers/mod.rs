//! Provider adapters. One module per debrid service; each implements
//! [`crate::DebridProvider`]. Real-Debrid is fully wired and tested; the others
//! are implemented against their public APIs (verify field paths against a live
//! account). Add more the same way, then list them in [`crate::registry`].

mod alldebrid;
mod debrid_link;
mod deepbrid;
mod high_way;
mod mega_debrid;
mod offcloud;
mod premiumize;
mod real_debrid;
mod torbox;
mod util;

pub use alldebrid::AllDebrid;
pub use debrid_link::DebridLink;
pub use deepbrid::Deepbrid;
pub use high_way::HighWay;
pub use mega_debrid::MegaDebrid;
pub use offcloud::Offcloud;
pub use premiumize::Premiumize;
pub use real_debrid::RealDebrid;
pub use torbox::TorBox;
