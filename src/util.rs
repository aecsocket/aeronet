use std::fmt::Display;

use generational_arena::Index;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClientId(pub Index);

impl Display for ClientId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (index, gen) = self.0.into_raw_parts();
        write!(f, "{index}v{gen}")
    }
}

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

pub trait AsPrettyError: Sized {
    fn as_pretty(&self) -> PrettyError<Self>;
}

impl<E: std::error::Error> AsPrettyError for E {
    fn as_pretty(&self) -> PrettyError<Self> {
        PrettyError(self)
    }
}
