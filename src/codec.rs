use crate::base::{KCPError, HASH_CAP, KCP_MAX_PACKET, KCP_MIN_PACKET};
use crate::message::{
    NetAccept, NetCommand, NetConnect, NetFinish, NetHash, NetStart, NetState, NetType,
};
use anyhow::Result;
use bincode::config::{DefaultOptions, Options};
use byteorder::{BigEndian, ByteOrder};
use fn_error_context::context;
use protobuf::{Message, ProtobufEnum};
use serde::de::{DeserializeSeed, Deserializer, SeqAccess, Visitor};
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum NetMessage {
    Connect(NetConnect),
    Accept(NetAccept),
    State(NetState),
    Start(NetStart),
    Finish(NetFinish),
    Command(NetCommand),
    Hash(NetHash),
}

impl NetMessage {
    #[context("NetMessage::decode()")]
    pub fn decode(bytes: &[u8]) -> Result<(NetMessage, usize)> {
        if bytes.len() < KCP_MIN_PACKET {
            return Err(KCPError::PacketTooShort.into());
        }
        if bytes.len() > KCP_MAX_PACKET {
            return Err(KCPError::PacketTooLong.into());
        }

        let msg_size = BigEndian::read_u16(&bytes[1..]) as usize;
        let offset = KCP_MIN_PACKET + msg_size;
        if offset > bytes.len() {
            return Err(KCPError::PacketBroken.into());
        }

        let typ = NetType::from_i32(bytes[0] as i32).unwrap_or(NetType::Unknown);
        let pb_bytes = &bytes[KCP_MIN_PACKET..(offset)];
        let msg = match typ {
            NetType::Connect => {
                let connect = NetConnect::parse_from_bytes(pb_bytes).map_err(KCPError::Protobuf)?;
                NetMessage::Connect(connect)
            }
            NetType::Accept => {
                let accept = NetAccept::parse_from_bytes(pb_bytes).map_err(KCPError::Protobuf)?;
                NetMessage::Accept(accept)
            }
            NetType::State => {
                let state = NetState::parse_from_bytes(pb_bytes).map_err(KCPError::Protobuf)?;
                NetMessage::State(state)
            }
            NetType::Start => {
                let start = NetStart::parse_from_bytes(pb_bytes).map_err(KCPError::Protobuf)?;
                NetMessage::Start(start)
            }
            NetType::Finish => {
                let finish = NetFinish::parse_from_bytes(pb_bytes).map_err(KCPError::Protobuf)?;
                NetMessage::Finish(finish)
            }
            NetType::Command => {
                let command = NetCommand::parse_from_bytes(pb_bytes).map_err(KCPError::Protobuf)?;
                NetMessage::Command(command)
            }
            NetType::Hash => {
                let hash = NetHash::parse_from_bytes(pb_bytes).map_err(KCPError::Protobuf)?;
                NetMessage::Hash(hash)
            }
            _ => return Err(KCPError::PacketBroken.into()),
        };

        return Ok((msg, offset));
    }

    #[context("NetMessage::encode()")]
    pub fn encode(&self, bytes: &mut Vec<u8>) -> Result<usize> {
        let base = bytes.len();
        bytes.extend_from_slice(&[0, 0, 0]);

        match self {
            NetMessage::Connect(msg) => {
                bytes[base] = NetType::Connect.value() as u8;
                msg.write_to_vec(bytes).map_err(KCPError::Protobuf)?;
            }
            NetMessage::Accept(msg) => {
                bytes[base] = NetType::Accept.value() as u8;
                msg.write_to_vec(bytes).map_err(KCPError::Protobuf)?;
            }
            NetMessage::State(msg) => {
                bytes[base] = NetType::State.value() as u8;
                msg.write_to_vec(bytes).map_err(KCPError::Protobuf)?;
            }
            NetMessage::Start(msg) => {
                bytes[base] = NetType::Start.value() as u8;
                msg.write_to_vec(bytes).map_err(KCPError::Protobuf)?;
            }
            NetMessage::Finish(msg) => {
                bytes[base] = NetType::Finish.value() as u8;
                msg.write_to_vec(bytes).map_err(KCPError::Protobuf)?;
            }
            NetMessage::Command(msg) => {
                bytes[base] = NetType::Command.value() as u8;
                msg.write_to_vec(bytes).map_err(KCPError::Protobuf)?;
            }
            NetMessage::Hash(msg) => {
                bytes[base] = NetType::Hash.value() as u8;
                msg.write_to_vec(bytes).map_err(KCPError::Protobuf)?;
            }
        };

        let offset = bytes.len() - base;
        if offset > KCP_MAX_PACKET {
            return Err(KCPError::MessageTooLong.into());
        }
        let msg_size = offset - KCP_MIN_PACKET;
        BigEndian::write_u16(&mut bytes[(base + 1)..], msg_size as u16);

        return Ok(offset);
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Command {
    Aaa(i32, i32),
    Bbb(f32, f32, f32),
}

#[derive(Debug, Clone, PartialEq)]
pub struct CommandEx {
    pub conv: u32,
    pub frame: u32,
    pub command: Command,
}

#[derive(Debug, Clone)]
pub struct CommandEncoder {
    net_command: NetMessage,
    net_hash: NetMessage,
    commands: Vec<Command>,
    hash_bytes: Vec<u8>,
    command_bytes: Vec<u8>,
}

impl CommandEncoder {
    pub fn new(cap: usize) -> CommandEncoder {
        return CommandEncoder {
            net_command: NetMessage::Command(NetCommand::default()),
            net_hash: NetMessage::Hash(NetHash::default()),
            commands: Vec::with_capacity(cap),
            hash_bytes: Vec::with_capacity(HASH_CAP * 2),
            command_bytes: Vec::with_capacity(KCP_MAX_PACKET),
        };
    }

    pub fn commands(&mut self) -> &mut Vec<Command> {
        return &mut self.commands;
    }

    pub fn hash(&mut self) -> &mut Vec<u8> {
        return match &mut self.net_hash {
            NetMessage::Hash(hash) => &mut hash.hash,
            _ => unreachable!(),
        };
    }

    pub fn buffers(&mut self) -> (&mut Vec<Command>, &mut Vec<u8>) {
        return match &mut self.net_hash {
            NetMessage::Hash(hash) => (&mut self.commands, &mut hash.hash),
            _ => unreachable!(),
        };
    }

    #[context("CommandEncoder::encode()")]
    pub fn encode(&mut self, frame: u32) -> Result<()> {
        match &mut self.net_hash {
            NetMessage::Hash(hash) => hash.frame = frame,
            _ => unreachable!(),
        };

        self.hash_bytes.clear();
        let _ = self.net_hash.encode(&mut self.hash_bytes)?;

        match &mut self.net_command {
            NetMessage::Command(cmd) => cmd.frame = frame,
            _ => unreachable!(),
        };

        self.command_bytes.clear();
        let _ = self.net_command.encode(&mut self.command_bytes)?;

        DefaultOptions::default()
            .with_fixint_encoding()
            .serialize_into(&mut self.command_bytes, &self.commands)
            .map_err(KCPError::Bincode)?;

        self.hash().clear();
        self.commands().clear();
        return Ok(());
    }

    pub fn command_bytes(&self) -> &[u8] {
        return &self.command_bytes;
    }

    pub fn hash_bytes(&self) -> &[u8] {
        return &self.hash_bytes;
    }
}

pub struct CommandDecoder {
    commands: Vec<CommandEx>,
}

impl CommandDecoder {
    pub fn new(cap: usize) -> CommandDecoder {
        return CommandDecoder {
            commands: Vec::with_capacity(cap),
        };
    }

    #[context("CommandDecoder::decode()")]
    pub fn decode(&mut self, bytes: &[u8]) -> Result<()> {
        let (command, offset) = match NetMessage::decode(bytes)? {
            (NetMessage::Command(command), offset) => (command, offset),
            _ => return Err(KCPError::PacketBroken.into()),
        };

        // size was checked in NetMessage::decode()
        let size = BigEndian::read_u16(&bytes[1..]) as usize;

        self.commands.clear();
        let visiter = CommandsVisitor {
            frame: command.frame,
            conv: command.conv,
            commands: &mut self.commands,
        };
        DefaultOptions::default()
            .with_fixint_encoding()
            .deserialize_seed(visiter, &bytes[offset..])
            .map_err(KCPError::Bincode)?;

        return Ok(());
    }

    pub fn len(&self) -> usize {
        return self.commands.len();
    }

    pub fn command(&self, idx: usize) -> &CommandEx {
        return &self.commands[idx];
    }

    pub fn commands(&self) -> &[CommandEx] {
        return &self.commands;
    }
}

struct CommandsVisitor<'t> {
    frame: u32,
    conv: u32,
    commands: &'t mut Vec<CommandEx>,
}

impl<'de, 't> DeserializeSeed<'de> for CommandsVisitor<'t> {
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        return deserializer.deserialize_seq(self);
    }
}

impl<'de, 't> Visitor<'de> for CommandsVisitor<'t> {
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        return formatter.write_str(r#"expecting [Command, ...]"#);
    }

    fn visit_seq<S: SeqAccess<'de>>(self, mut seq: S) -> Result<Self::Value, S::Error> {
        while let Some(command) = seq.next_element::<Command>()? {
            self.commands.push(CommandEx {
                conv: self.conv,
                frame: self.frame,
                command,
            });
        }
        return Ok(());
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_message_encode() {
        let mut bytes = Vec::new();

        bytes.clear();
        NetMessage::Connect(NetConnect::default())
            .encode(&mut bytes)
            .unwrap();
        assert_eq!(bytes[0], NetType::Connect as u8);

        bytes.clear();
        NetMessage::Accept(NetAccept::default())
            .encode(&mut bytes)
            .unwrap();
        assert_eq!(bytes[0], NetType::Accept as u8);

        bytes.clear();
        NetMessage::State(NetState::default())
            .encode(&mut bytes)
            .unwrap();
        assert_eq!(bytes[0], NetType::State as u8);

        bytes.clear();
        NetMessage::Start(NetStart::default())
            .encode(&mut bytes)
            .unwrap();
        assert_eq!(bytes[0], NetType::Start as u8);

        bytes.clear();
        NetMessage::Finish(NetFinish::default())
            .encode(&mut bytes)
            .unwrap();
        assert_eq!(bytes[0], NetType::Finish as u8);

        bytes.clear();
        NetMessage::Command(NetCommand::default())
            .encode(&mut bytes)
            .unwrap();
        assert_eq!(bytes[0], NetType::Command as u8);

        let mut hash = NetHash::default();
        hash.frame = 3333;
        hash.hash.extend_from_slice(&[9, 9, 9, 9, 9]);
        bytes.clear();
        NetMessage::Hash(hash.clone()).encode(&mut bytes).unwrap();
        assert_eq!(hash.compute_size() as usize + 3, bytes.len());
        assert_eq!(bytes[0], NetType::Hash as u8);
        assert_eq!(BigEndian::read_u16(&bytes[1..]) as u32, hash.compute_size());

        let mut hash = NetHash::default();
        hash.hash = vec![0; KCP_MAX_PACKET + 1];
        let err = NetMessage::Hash(hash).encode(&mut bytes).unwrap_err();
        assert_eq!(
            err.downcast::<KCPError>().unwrap().to_string(),
            "message too long"
        );
    }

    #[test]
    fn test_message_decode() {
        let (msg, _) = NetMessage::decode(&[NetType::Connect as u8, 0, 0]).unwrap();
        assert_eq!(msg, NetMessage::Connect(NetConnect::default()));

        let (msg, _) = NetMessage::decode(&[NetType::Accept as u8, 0, 0]).unwrap();
        assert_eq!(msg, NetMessage::Accept(NetAccept::default()));

        let (msg, _) = NetMessage::decode(&[NetType::State as u8, 0, 0]).unwrap();
        assert_eq!(msg, NetMessage::State(NetState::default()));

        let (msg, _) = NetMessage::decode(&[NetType::Start as u8, 0, 0]).unwrap();
        assert_eq!(msg, NetMessage::Start(NetStart::default()));

        let (msg, _) = NetMessage::decode(&[NetType::Finish as u8, 0, 0]).unwrap();
        assert_eq!(msg, NetMessage::Finish(NetFinish::default()));

        let (msg, _) = NetMessage::decode(&[NetType::Hash as u8, 0, 0]).unwrap();
        assert_eq!(msg, NetMessage::Hash(NetHash::default()));

        let mut bytes = vec![NetType::Command as u8, 0, 7];
        let mut cmd = NetCommand::default();
        cmd.conv = 98765;
        cmd.frame = 545;
        cmd.write_to_vec(&mut bytes).unwrap();
        let (msg, offset) = NetMessage::decode(&bytes).unwrap();
        assert_eq!(msg, NetMessage::Command(cmd));
        assert_eq!(offset, bytes.len());

        let err = NetMessage::decode(&[0]).unwrap_err();
        assert_eq!(
            err.downcast::<KCPError>().unwrap().to_string(),
            "packet too short"
        );

        let err = NetMessage::decode(&vec![0; KCP_MAX_PACKET + 1]).unwrap_err();
        assert_eq!(
            err.downcast::<KCPError>().unwrap().to_string(),
            "packet too long"
        );

        let err = NetMessage::decode(&[NetType::Hash as u8, 0, 100]).unwrap_err();
        assert_eq!(
            err.downcast::<KCPError>().unwrap().to_string(),
            "packet broken"
        );
    }

    #[test]
    fn test_command_encoder() {
        let mut ce = CommandEncoder::new(0);
        ce.commands().push(Command::Aaa(47, 57));
        ce.commands().push(Command::Bbb(3.0, 3.0, 8.0));
        ce.hash().extend_from_slice(&[8, 7, 8, 6]);
        ce.encode(345).unwrap();

        let (msg, offset) = NetMessage::decode(ce.hash_bytes()).unwrap();
        let mut hash = NetHash::default();
        hash.frame = 345;
        hash.hash = vec![8, 7, 8, 6];
        assert_eq!(msg, NetMessage::Hash(hash));

        let (msg, offset) = NetMessage::decode(ce.command_bytes()).unwrap();
        let mut cmd = NetCommand::default();
        cmd.frame = 345;
        assert_eq!(msg, NetMessage::Command(cmd));

        let cmds: Vec<Command> = DefaultOptions::default()
            .with_fixint_encoding()
            .deserialize(&ce.command_bytes()[offset..])
            .unwrap();
        assert_eq!(cmds[0], Command::Aaa(47, 57));
        assert_eq!(cmds[1], Command::Bbb(3.0, 3.0, 8.0));
    }

    #[test]
    fn test_command_decoder() {
        let mut bytes = Vec::<u8>::new();
        let mut net_cmd = NetCommand::default();
        net_cmd.conv = 6666;
        net_cmd.frame = 123;
        NetMessage::Command(net_cmd).encode(&mut bytes).unwrap();

        let mut cmds = Vec::<Command>::new();
        cmds.push(Command::Aaa(22, 33));
        cmds.push(Command::Bbb(5.0, 6.0, 7.0));
        cmds.push(Command::Bbb(9.0, 8.0, 7.0));
        DefaultOptions::default()
            .with_fixint_encoding()
            .serialize_into(&mut bytes, &cmds)
            .unwrap();

        let mut cd = CommandDecoder::new(0);
        cd.decode(&bytes).unwrap();

        assert_eq!(cd.commands().len(), 3);
        assert_eq!(
            cd.command(0).clone(),
            CommandEx {
                conv: 6666,
                frame: 123,
                command: Command::Aaa(22, 33),
            }
        );
        assert_eq!(
            cd.command(2).clone(),
            CommandEx {
                conv: 6666,
                frame: 123,
                command: Command::Bbb(9.0, 8.0, 7.0),
            }
        );
    }
}
