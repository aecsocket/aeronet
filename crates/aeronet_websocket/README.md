[`aeronet_io`] implementation using [WebSockets] for reliable-ordered data transfer over TCP between
peers, compatible with both native and WASM.

This uses [`tokio-tungstenite`] on native targets, and [`web-sys`] on WASM targets, for WebSocket
usage. The server implementation is only available on native targets.

[`aeronet_io`]: https://docs.rs/aeronet_io
[WebSockets]: https://web.dev/articles/websockets-basics
[`tokio-tungstenite`]: https://docs.rs/tokio-tungstenite
[`web-sys`]: https://docs.rs/web-sys
