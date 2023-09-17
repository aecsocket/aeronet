//! Server-side transport API, handling incoming clients, and sending/receiving messages
//! to/from clients.

use std::fmt::Display;

use generational_arena::Index;

/// A unique identifier for a client connected to a server.
///
/// This uses an [`Index`] under the hood, as it is expected that transport layers use a
/// generational arena to store clients. Using a [`generational_arena::Arena`] avoids the problem
/// of one client disconnecting with an ID, and another client later connecting with the same ID,
/// causing some code to mistake client 2 for client 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClientId(Index);

impl ClientId {
    /// Creates an ID from the raw generational index.
    pub fn from_raw(index: Index) -> Self {
        Self(index)
    }

    /// Converts an ID into its raw generational index.
    pub fn into_raw(self) -> Index {
        self.0
    }
}

impl Display for ClientId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (index, gen) = self.0.into_raw_parts();
        write!(f, "{index}v{gen}")
    }
}
