use {
    std::{convert::Infallible, error::Error, fmt},
    wasm_bindgen::JsValue,
};

/// Error obtained from a JavaScript execution context.
#[derive(Debug, Clone)]
pub struct JsError(pub String);

impl fmt::Display for JsError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl Error for JsError {}

impl From<JsValue> for JsError {
    fn from(value: JsValue) -> Self {
        let msg = value
            .as_string()
            .or_else(|| {
                let msg = js_sys::Reflect::get(&value, &"message".into()).ok()?;
                msg.as_string()
            })
            .unwrap_or_else(|| format!("{value:?}"));
        Self(msg)
    }
}

impl From<xwt_web_sys::Error> for JsError {
    fn from(value: xwt_web_sys::Error) -> Self {
        Self::from(value.0)
    }
}

impl From<Infallible> for JsError {
    fn from(_: Infallible) -> Self {
        unreachable!()
    }
}
