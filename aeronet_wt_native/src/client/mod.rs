pub struct Frontend {}

impl Frontend {
    pub async fn new(url: &str) {
        let web_transport = WebTransport::new();
    }
}

pub fn create<C: TransportConfig>(config: ClientConfig) -> (Frontend, Backend) {}
