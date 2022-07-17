use bevy::prelude::{App, Commands, Entity, Query, Res};
use bevy::MinimalPlugins;
use bevy_ws_server::{ReceiveError, WsConnection, WsListener, WsPlugin};

fn main() {
    App::new()
        .add_plugins(MinimalPlugins)
        .add_plugin(WsPlugin)
        .add_startup_system(startup)
        .add_system(receive_message)
        .run();
}

fn startup(listener: Res<WsListener>) {
    listener.listen("127.0.0.1:8080");
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
