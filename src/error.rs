use std::fmt::Display;

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

pub fn as_pretty<E: std::error::Error>(err: &E) -> PrettyError<'_, E> {
    PrettyError(err)
}
