//! Compile-time edition identity. Shared safety code is independent of this feature.

#[cfg(feature = "paranoid-preview")]
mod paranoid_preview;

#[cfg(feature = "paranoid-preview")]
pub(crate) const NAME: &str = paranoid_preview::NAME;

#[cfg(not(feature = "paranoid-preview"))]
pub(crate) const NAME: &str = "standard";
