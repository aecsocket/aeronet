use std::fmt::Display;

use proc_macro2::TokenStream;
use quote::ToTokens;
use syn::{Attribute, Error, Ident, Meta, Result};

pub fn parse_attr<'a, I, R>(
    ident: &I,
    attrs: &'a [Attribute],
    map: impl FnOnce(&'a Attribute) -> Result<R>,
) -> Result<Option<R>>
where
    I: ?Sized + Display,
    Ident: PartialEq<I>,
{
    enum State<F, R> {
        None(F),
        Done(R),
    }

    let mut state = State::None(map);
    for attr in attrs.iter().filter(|attr| attr.path().is_ident(ident)) {
        let State::None(map) = state else {
            return Err(Error::new_spanned(
                attr,
                format!("duplicate #[{ident}] attribute"),
            ));
        };

        let result = map(attr)?;
        state = State::Done(result);
    }

    Ok(match state {
        State::None(_) => None,
        State::Done(r) => Some(r),
    })
}

pub fn parse_attr_with_one_arg<'a, I>(
    ident: &I,
    attrs: &'a [Attribute],
) -> Result<Option<&'a TokenStream>>
where
    I: ?Sized + Display,
    Ident: PartialEq<I>,
{
    parse_attr(ident, attrs, |attr| {
        let Meta::List(list) = &attr.meta else {
            return Err(Error::new_spanned(
                attr,
                format!("missing `value` in #[{ident}(value)]"),
            ));
        };

        Ok(&list.tokens)
    })
}

pub fn require_attr_with_one_arg<'a, I>(
    ident: &I,
    tokens: impl ToTokens,
    attrs: &'a [Attribute],
) -> Result<&'a TokenStream>
where
    I: ?Sized + Display,
    Ident: PartialEq<I>,
{
    parse_attr_with_one_arg(ident, attrs)?
        .ok_or_else(|| Error::new_spanned(tokens, format!("missing #[{ident}] attribute")))
}
