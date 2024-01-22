use async_net::SocketAddr;

use bevy::prelude::*;
use bevy::tasks::{IoTaskPool, Task};
use crossbeam_channel::{Receiver, Sender};
use futures::{pin_mut, select, FutureExt};

use std::net::{TcpListener, TcpStream};
use std::pin::Pin;
use std::task::{Context, Poll};

use anyhow::Result;
pub use async_channel::TryRecvError as ReceiveError;
use async_native_tls::{TlsAcceptor, TlsStream};
use async_tungstenite::{tungstenite, WebSocketStream};
use futures::sink::{Sink, SinkExt};
use smol::{prelude::*, Async};
use tungstenite::Message;

// use tungstenite::Message;

pub struct WsPlugin;

impl Plugin for WsPlugin {
    fn build(&self, app: &mut App) {
        let (ws_tx, ws_rx) = crossbeam_channel::unbounded();
        app.insert_resource(WsListener::new(ws_tx))
            .insert_resource(WsAcceptQueue { ws_rx })
            .add_systems(Update, accept_ws_from_queue);
    }
}

#[derive(Resource)]
pub struct WsListener {
    ws_tx: Sender<WsStream>,
}

#[derive(Resource)]
pub struct WsAcceptQueue {
    ws_rx: Receiver<WsStream>,
}

/// A WebSocket or WebSocket+TLS connection.
enum WsStream {
    /// A plain WebSocket connection.
    Plain(WebSocketStream<Async<TcpStream>>),

    /// A WebSocket connection secured by TLS.
    Tls(WebSocketStream<TlsStream<Async<TcpStream>>>),
}

impl Sink<Message> for WsStream {
    type Error = tungstenite::Error;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        match &mut *self {
            WsStream::Plain(s) => Pin::new(s).poll_ready(cx),
            WsStream::Tls(s) => Pin::new(s).poll_ready(cx),
        }
    }

    fn start_send(mut self: Pin<&mut Self>, item: Message) -> Result<(), Self::Error> {
        match &mut *self {
            WsStream::Plain(s) => Pin::new(s).start_send(item),
            WsStream::Tls(s) => Pin::new(s).start_send(item),
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        match &mut *self {
            WsStream::Plain(s) => Pin::new(s).poll_flush(cx),
            WsStream::Tls(s) => Pin::new(s).poll_flush(cx),
        }
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        match &mut *self {
            WsStream::Plain(s) => Pin::new(s).poll_close(cx),
            WsStream::Tls(s) => Pin::new(s).poll_close(cx),
        }
    }
}

impl Stream for WsStream {
    type Item = tungstenite::Result<Message>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match &mut *self {
            WsStream::Plain(s) => Pin::new(s).poll_next(cx),
            WsStream::Tls(s) => Pin::new(s).poll_next(cx),
        }
    }
}

impl WsListener {
    pub fn new(ws_tx: Sender<WsStream>) -> Self {
        Self { ws_tx }
    }

    pub fn listen(&self, bind_to: impl Into<SocketAddr>, tls: Option<TlsAcceptor>) -> Result<()> {
        let listener = Async::<TcpListener>::bind(bind_to).expect("cannot bind to the address");

        let host = match &tls {
            None => format!("ws://{}", listener.get_ref().local_addr().unwrap()),
            Some(_) => format!("wss://{}", listener.get_ref().local_addr().unwrap()),
        };

        log::info!("Listening on {}", host);

        let task_pool = IoTaskPool::get();
        let ws_tx = self.ws_tx.clone();
        let task = task_pool.spawn(async move {
            let tls = tls;
            loop {
                match listener.accept().await {
                    Ok((stream, addr)) => {
                        log::debug!("new connection from {}", addr);
                        let ws_tx = ws_tx.clone();

                        let tls = tls.clone();

                        let accept = async move {
                            let ws_stream = match &tls {
                                None => WsStream::Plain(
                                    async_tungstenite::accept_async(stream).await.unwrap(),
                                ),
                                Some(tls) => {
                                    let tls_stream = tls.accept(stream).await.unwrap();
                                    WsStream::Tls(
                                        async_tungstenite::accept_async(tls_stream).await.unwrap(),
                                    )
                                }
                            };
                            let _ = ws_tx.send(ws_stream);
                        };

                        task_pool.spawn(accept).detach();
                    }
                    Err(e) => {
                        log::error!("error accepting a new connection: {}", e);
                    }
                }
            }
        });

        task.detach();
        Ok(())
    }
}

#[derive(Component)]
pub struct WsConnection {
    _io: Task<()>,
    sender: async_channel::Sender<Message>,
    receiver: async_channel::Receiver<Message>,
}

impl WsConnection {
    pub fn send(&self, message: Message) -> bool {
        self.sender.try_send(message).is_ok()
    }

    pub fn receive(&self) -> Result<Message, ReceiveError> {
        self.receiver.try_recv()
    }
}

pub fn accept_ws_from_queue(mut commands: Commands, queue: ResMut<WsAcceptQueue>) {
    for mut websocket in queue.ws_rx.try_iter() {
        let (message_tx, io_message_rx) = async_channel::unbounded::<Message>();
        let (io_message_tx, message_rx) = async_channel::unbounded::<Message>();

        let io = IoTaskPool::get().spawn(async move {
            loop {
                let from_channel = io_message_rx.recv().fuse();
                let from_ws = websocket.next().fuse();

                pin_mut!(from_channel, from_ws);

                select! {
                    message = from_channel => if let Ok(message) = message {
                        let _ =  websocket.send(message).await;
                    } else {
                        break;
                    },
                    message = from_ws => if let Some(Ok(message)) = message {
                        let _ = io_message_tx.send(message).await;
                    } else {
                        break;
                    },
                    complete => break,
                }
            }
        });
        commands.spawn(WsConnection {
            _io: io,
            sender: message_tx,
            receiver: message_rx,
        });
    }
}
