//! The UDAP setup in force. The server builds it from `[auth.udap]` at start and again whenever
//! the `auth` configuration changes, so UDAP (and Tiered OAuth) can be switched on, off or
//! re-keyed without a restart. Handlers read it per request; with UDAP off they answer 404, as the
//! Security IG asks of a server without UDAP.

use std::sync::Arc;

use arc_swap::ArcSwapOption;

use super::tiered::TieredOAuth;
use crate::http::udap::UdapState;

/// The current UDAP state and Tiered OAuth, shared by the endpoints that use them.
#[derive(Clone, Default)]
pub struct LiveUdap {
    state: Arc<ArcSwapOption<UdapState>>,
    tiered: Arc<ArcSwapOption<TieredOAuth>>,
}

impl LiveUdap {
    /// The UDAP state, when UDAP is on.
    #[must_use]
    pub fn current(&self) -> Option<Arc<UdapState>> {
        self.state.load_full()
    }

    /// Tiered OAuth, when it is on.
    #[must_use]
    pub fn tiered(&self) -> Option<Arc<TieredOAuth>> {
        self.tiered.load_full()
    }

    /// Puts a new setup in force; `None` switches UDAP off.
    pub fn replace(&self, state: Option<UdapState>, tiered: Option<Arc<TieredOAuth>>) {
        self.state.store(state.map(Arc::new));
        self.tiered.store(tiered);
    }
}
