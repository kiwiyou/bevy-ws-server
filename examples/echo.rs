use bevy::app::{Startup, Update};
use bevy::prelude::{App, Commands, Entity, Query, Res};
use bevy::MinimalPlugins;
use bevy_ws_server::{ReceiveError, WsConnection, WsListener, WsPlugin};

fn main() {
    App::new()
        .add_plugins((MinimalPlugins, WsPlugin))
        .add_systems(Startup, startup)
        .add_systems(Update, receive_message)
        .run();
}

fn startup(listener: Res<WsListener>) {
    if let Err(e) = listener.listen(([127, 0, 0, 1], 6669), None) {
        log::error!("error starting WS listener: {e}");
    };
}

fn receive_message(mut commands: Commands, connections: Query<(Entity, &WsConnection)>) {
    for (entity, conn) in connections.iter() {
        loop {
            match conn.receive() {
                Ok(message) => {
                    conn.send(message);
                }
                Err(ReceiveError::Empty) => break,
                Err(ReceiveError::Closed) => {
                    commands.entity(entity).despawn();
                    break;
                }
            }
        }
    }
}
