//! Utility for pretty-printing errors.
//!
//! In some situations, such as when reading an error from a Bevy event reader,
//! you may only have access to an error behind a shared reference. Use
//! [`as_pretty`] to wrap that reference in a [`PrettyError`], making the
//! alternative [`fmt::Display`] impl format the entire error chain, in the
//! same style as [`anyhow`](https://docs.rs/anyhow).
//!
//! Use `{:#}` to print the error in the "pretty" style.

use std::{error::Error, fmt};

/// Helper struct to display errors in a pretty-printed style.
///
/// See the [module-level docs](self).
pub struct PrettyError<'a, E>(&'a E);

impl<E> fmt::Display for PrettyError<'_, E>
where
    E: Error,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)?;

        if f.alternate() {
            let mut cur = self.0.source();
            while let Some(source) = cur {
                write!(f, ": {source}")?;
                cur = source.source();
            }
        }

        Ok(())
    }
}

/// Wraps a shared reference to an error in order to make its [`fmt::Display`]
/// impl write the entire error chain.
///
/// See the [module-level docs](self).
pub fn as_pretty<E>(err: &E) -> PrettyError<'_, E>
where
    E: Error,
{
    PrettyError(err)
}
