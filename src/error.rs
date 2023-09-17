//! Utility for pretty-printing errors.
//!
//! See [`PrettyError`].

use std::fmt::Display;

/// Helper struct to display errors in [`anyhow::Error`]'s style whilst only holding a shared
/// reference to the error.
///
/// In some situations, such as when reading an error from a Bevy event reader, you may only have
/// access to an error behind a shared reference. Use [`as_pretty`] to wrap that reference in this
/// struct, making the alternative [`Display`] impl format the entire error chain, in the same
/// style as [`anyhow::Error`].
pub struct PrettyError<'a, E>(&'a E);

impl<E: std::error::Error> Display for PrettyError<'_, E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)?;

        if f.alternate() {
            let mut cur = self.0.source();
            while let Some(source) = cur {
                write!(f, ": {}", source)?;
                cur = source.source();
            }
        }

        Ok(())
    }
}

/// Wraps a shared reference to an error in order to make its [`Display`] impl write the entire
/// error chain.
///
/// See [`PrettyError`] for more info.
pub fn as_pretty<E: std::error::Error>(err: &E) -> PrettyError<'_, E> {
    PrettyError(err)
}
