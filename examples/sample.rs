#[derive(Event)]
struct ServerOpened {
    pub server: Entity,
}

#[derive(Event)]
struct ServerClosed {
    pub server: Entity,
    pub reason: CloseReason,
}

enum CloseReason {
    Local(String),
    Error(anyhow::Error),
}

#[derive(Event)]
struct RemoteClientConnecting {
    pub server: Entity,
    pub client: Entity,
}

#[derive(Event)]
struct RemoteClientConnected {
    pub server: Entity,
    pub client: Entity,
}

#[derive(Event)]
struct RemoteClientDisconnected {
    pub server: Entity,
    pub client: Entity,
    pub reason: DisconnectReason,
}

#[derive(Component)]
struct Connecting {
    pub server: Entity,
}

#[derive(Component)]
struct Connected {
    pub server: Entity,
}

enum DisconnectReason {
    Local(String),
    Remote(String),
    Error(anyhow::Error),
}

// ---

#[derive(Component)]
struct ClientHeaders(pub HashMap<String, String>);

// ---

fn setup(mut commands: Commands) {
    commands.spawn((
        WebTransportServer::new(server_config()),
        Name::new("WebTransport Server"),
    ));
}

fn on_opened(mut events: EventReader<ServerOpened>, names: Query<&Name>) {
    for event in events.read() {
        let name = names.get(event.server).unwrap();
        info!("Server `{name}` opened");
    }
}

fn on_closed(mut events: EventReader<ServerClosed>, names: Query<&Name>) {
    for event in events.read() {
        let name = names.get(event.server).unwrap();
        info!("Server `{name}` closed: {:#}", event.reason);
    }
}

fn on_connecting(
    mut events: EventReader<RemoteClientConnecting>,
    mut servers: Query<&mut WebTransportServer>,
    headers: Query<&ClientHeaders>,
) {
    for event in events.read() {
        let client = event.client;
        let server = event.server;
        info!("Client {client:?} connecting to {server:?}");

        let mut webtransport = servers.get(server).unwrap();
        let headers = headers.get(client).unwrap();
        if check_header_auth(&headers) {
            webtransport.accept(client);
        } else {
            webtransport.reject(client);
        }
    }
}

fn on_connected(mut events: EventReader<RemoteClientConnected>) {
    for event in events.read() {
        info!("Client {:?} connected to {:?}", event.client, event.server);
    }
}

fn on_disconnected(mut events: EventReader<RemoteClientDisconnected>) {
    for event in events.read() {
        info!(
            "Client {:?} disconnected from {:?}: {:#}",
            event.client, event.server, event.reason
        );
    }
}
