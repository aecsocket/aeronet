use std::time::Duration;

use bevy::{app::ScheduleRunnerPlugin, prelude::*, log::LogPlugin};
use crossbeam_channel::{Sender, Receiver, TryRecvError, unbounded};

#[derive(Debug, Resource)]
pub struct ChannelClientTransport {
    pub(crate) send: Sender<()>,
    pub(crate) recv: Receiver<()>,
}

impl ChannelClientTransport {
    fn recv(&mut self) -> Option<Result<(), TryRecvError>> {
        match self.recv.try_recv() {
            Ok(msg) => Some(Ok(msg)),
            Err(e) => Some(Err(e)),
        }
    }
}

#[derive(Resource)]
struct ServerStuff {
    send: Sender<()>,
    recv: Receiver<()>,
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
    commands.insert_resource(tx);
    commands.insert_resource(ServerStuff {
        send: s_s2c,
        recv: r_c2s,
    });
}

fn recv(
    mut transport: ResMut<ChannelClientTransport>,
) {
    while let Some(result) = transport.recv() {
        println!("r = {:?}", result);
    }
}
