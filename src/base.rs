use thiserror::Error;

use crate::message::NetFinishCause;

pub const KCP_INTERVAL: u64 = 10;
pub const KCP_MTU: usize = 470;
pub const KCP_MIN_PACKET: usize = 1 + 2;
pub const KCP_MAX_PACKET: usize = 470 * 4;
pub const KCP_WINDOW_SIZE: usize = 256;

pub const PLAYERS_CAP: usize = 16;
pub const COMMANDS_CAP: usize = 256;
pub const HASH_CAP: usize = 128;

pub const CONNECT_TIMEOUT: u64 = 10;
pub const START_TIMEOUT: u64 = 20;
pub const UPDATE_TIMEOUT: u64 = 7;
pub const FINISH_TIMEOUT: u64 = 5;

#[derive(Error, Debug)]
pub enum KCPError {
    // network broken
    #[error("io error")]
    IO(#[from] std::io::Error),
    #[error("timeout")]
    Timeout,
    #[error("window exhausted")]
    WindowExhausted,

    // InvalidPacket
    #[error("packet broken")]
    PacketBroken,
    #[error("packet too short")]
    PacketTooShort,
    #[error("packet too long")]
    PacketTooLong,
    #[error("unexpected packet")]
    UnexpectedPacket,

    #[error("game over")]
    GameOver,

    #[error("remote finished")]
    RemoteFinished(NetFinishCause),

    // client error
    #[error("protobuf error")]
    Protobuf(#[from] protobuf::ProtobufError),
    #[error("bincode error")]
    Bincode(#[from] bincode::Error),
    #[error("kcp error {0}")]
    KCP(i32),
    #[error("unexpected error")]
    Unexpected,
    #[error("invalid frame")]
    InvalidFrame,
    #[error("message too long")]
    MessageTooLong,
}

impl KCPError {
    pub fn cause(&self) -> NetFinishCause {
        return match self {
            Self::IO(_) => NetFinishCause::NetworkBroken,
            Self::Timeout => NetFinishCause::NetworkBroken,
            Self::WindowExhausted => NetFinishCause::NetworkBroken,
            Self::PacketBroken => NetFinishCause::InvalidPacket,
            Self::PacketTooShort => NetFinishCause::InvalidPacket,
            Self::PacketTooLong => NetFinishCause::InvalidPacket,
            Self::UnexpectedPacket => NetFinishCause::InvalidPacket,
            Self::GameOver => NetFinishCause::GameOver,
            Self::RemoteFinished(cause) => *cause,
            Self::Protobuf(_) => NetFinishCause::ClientError,
            Self::Bincode(_) => NetFinishCause::ClientError,
            Self::KCP(_) => NetFinishCause::ClientError,
            Self::Unexpected => NetFinishCause::ClientError,
            Self::InvalidFrame => NetFinishCause::ClientError,
            Self::MessageTooLong => NetFinishCause::ClientError,
        };
    }
}
