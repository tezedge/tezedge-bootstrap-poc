use std::{net::SocketAddr, mem};
use tokio::net::TcpStream;
use tezos_messages::p2p::encoding::ack::AckMessage;
use slog::Logger;
use super::{
    error::SocketError, handshake_state::HandshakeState,
    bootstrap_state::BootstrapState,
};

/// The state of peer communication
pub enum SocketState {
    Connecting(SocketAddr),
    Handshake(TcpStream, HandshakeState),
    BootstrapState(TcpStream, BootstrapState),
    // TODO: report reason
    Finish,
    Awaiting,
}

impl SocketState {
    pub fn outgoing(address: SocketAddr) -> Self {
        SocketState::Connecting(address)
    }

    pub async fn run(&mut self, logger: &Logger) -> Result<(), SocketError> {
        let state = mem::replace(self, SocketState::Awaiting);
        let state = match state {
            SocketState::Connecting(address) => {
                let stream = TcpStream::connect(address.clone())
                    .await
                    .map_err(SocketError::Io)?;
                slog::info!(logger, "connected to {}", address);
                SocketState::Handshake(stream, HandshakeState::Connection)
            },
            SocketState::Handshake(mut stream, mut state) => {
                state.run(logger, &mut stream).await?;
                match state {
                    HandshakeState::Finish(decipher, ack) => {
                        slog::info!(logger, "complete handshake {}", stream.peer_addr().unwrap());
                        match ack {
                            AckMessage::Ack => {
                                slog::info!(logger, "ready to bootstrap");
                                SocketState::BootstrapState(stream, BootstrapState::new(decipher))
                            },
                            AckMessage::Nack(info) => {
                                slog::debug!(logger, "{:?}", info);
                                SocketState::Finish
                            }
                            AckMessage::NackV0 => SocketState::Finish,
                        }
                    },
                    incomplete => SocketState::Handshake(stream, incomplete),
                }
            },
            // TODO:
            SocketState::BootstrapState(mut stream, mut bootstrap) => {
                bootstrap.run(logger, &mut stream).await?;
                SocketState::BootstrapState(stream, bootstrap)
            },
            SocketState::Finish => SocketState::Finish,
            SocketState::Awaiting => SocketState::Awaiting,
        };
        let _ = mem::replace(self, state);
        Ok(())
    }
}