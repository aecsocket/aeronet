use web_sys::WebTransport;

pub struct Frontend {

}

impl Frontend {
    pub async fn new() -> Self {
        let s = WebTransport::new();
    }
}
