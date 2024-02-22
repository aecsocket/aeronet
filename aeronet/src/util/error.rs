use std::{error::Error, fmt};

/// Helper struct to display errors in a pretty-printed style.
///
/// In some situations, such as when reading an error from a Bevy event reader,
/// you may only have access to an error behind a shared reference. Use
/// [`pretty_error`] to wrap that reference a [`PrettyError`], making the
/// alternative [`fmt::Display`] impl format the entire error chain, in the
/// same style as [`anyhow`](https://docs.rs/anyhow).
pub struct PrettyError<'a, E>(&'a E);

impl<E: Error> fmt::Display for PrettyError<'_, E> {
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
/// See [`PrettyError`].
pub fn pretty_error<E: Error>(err: &E) -> PrettyError<'_, E> {
    PrettyError(err)
}
