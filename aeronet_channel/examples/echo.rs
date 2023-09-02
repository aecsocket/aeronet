use std::time::Duration;

use bevy::{app::ScheduleRunnerPlugin, prelude::*, log::LogPlugin};
use crossbeam_channel::{Sender, Receiver, TryRecvError, unbounded};

#[derive(Debug, Resource)]
pub struct ChannelClientTransport {
    pub(crate) send: Sender<()>,
    pub(crate) recv: Receiver<()>,
}

impl ChannelClientTransport {
    fn recv(&mut self) -> Option<Result<(), ()>> {
        match self.recv.try_recv() {
            Ok(msg) => Some(Ok(msg)),
            Err(TryRecvError::Empty) => Some(Err(())),
            Err(TryRecvError::Disconnected) => Some(Err(())),
        }
    }
}

fn main() {
    App::new()
        .add_plugins(MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_millis(100))))
        .add_systems(Startup, setup)
        .add_systems(Update, recv)
        .add_systems(Update, move || println!("still works"))
        .run();
}

fn setup(mut commands: Commands) {
    // let mut server_tx = ServerTransport::new();
    // let (client_tx, client_id) = server_tx.connect();
    // server_tx.disconnect(client_id);
    let (s_c2s, r_c2s) = unbounded::<()>();
    let (s_s2c, r_s2c) = unbounded::<()>();

    let tx = ChannelClientTransport {
        send: s_c2s,
        recv: r_s2c,
    };
    //commands.insert_resource(server_tx);
    commands.insert_resource(tx);
}

fn recv(
    mut transport: ResMut<ChannelClientTransport>,
    //mut recv: EventWriter<ClientRecvEvent<S>>,
    //mut errors: EventWriter<ClientTransportError>,
) {
    while let Some(result) = transport.recv() {
        // match result {
        //     Ok(msg) => {},//recv.send(ClientRecvEvent { msg }),
        //     Err(err) => {},//errors.send(err),
        // }
    }
}
