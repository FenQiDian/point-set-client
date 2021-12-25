use crate::base::{
    KCPError, COMMANDS_CAP, CONNECT_TIMEOUT, FINISH_TIMEOUT, KCP_INTERVAL, KCP_MAX_PACKET,
    KCP_MIN_PACKET, START_TIMEOUT, UPDATE_TIMEOUT,
};
use crate::chan::{NetChan, NetInputState};
use crate::codec::{CommandDecoder, CommandEncoder, NetMessage};
use crate::kcp::NetKCP;
use crate::message::{NetConnect, NetFinishCause, NetPlayerState, NetType};
use anyhow::{Error, Result};
use fn_error_context::context;
use protobuf::{Clear, ProtobufEnum};
use std::net::SocketAddr;
use std::time::{Duration, SystemTime};

pub struct NetWorker {
    chan: NetChan,
    kcp: Box<NetKCP>,
    kcp_buffer: Vec<u8>,
    conv: u32,
    room_id: String,
    player_id: String,
    password: String,

    cmd_encoder: CommandEncoder,
    cmd_decoder: CommandDecoder,

    state: NetPlayerState,
    frame: u32,
    started_at: SystemTime,
    stopped_at: SystemTime,
    updated_at: SystemTime,
}

unsafe impl Send for NetWorker {}

impl NetWorker {
    #[context("NetWorker::new()")]
    pub fn new(
        addr: SocketAddr,
        conv: u32,
        room_id: &str,
        player_id: &str,
        password: &str,
        chan: NetChan,
    ) -> Result<NetWorker> {
        return Ok(NetWorker {
            chan,
            kcp: NetKCP::new(addr, conv)?,
            kcp_buffer: Vec::with_capacity(KCP_MAX_PACKET),
            conv,
            room_id: room_id.to_string(),
            player_id: player_id.to_string(),
            password: password.to_string(),

            cmd_encoder: CommandEncoder::new(COMMANDS_CAP),
            cmd_decoder: CommandDecoder::new(COMMANDS_CAP * 2),

            state: NetPlayerState::Initing,
            frame: 0,
            started_at: SystemTime::now() + Duration::from_secs(60 * 60 * 24 * 3650),
            stopped_at: SystemTime::now() + Duration::from_secs(60 * 60 * 24 * 3650),
            updated_at: SystemTime::now(),
        });
    }

    pub fn run(&mut self) {
        self.started_at = SystemTime::now();
        if let Err(err) = self.connect() {
            self.finish(err, false);
        } else {
            if let Err(err) = self.update() {
                self.finish(err, true);
            }
        }
    }

    #[context("NetWorker::update()")]
    pub fn connect(&mut self) -> Result<()> {
        let mut connect = NetConnect::default();
        connect.room_id = self.room_id.clone();
        connect.player_id = self.player_id.clone();
        connect.password = self.password.clone();

        self.kcp_buffer.clear();
        NetMessage::Connect(connect).encode(&mut self.kcp_buffer)?;
        self.kcp.send_kcp(&self.kcp_buffer)?;
        self.kcp_buffer.clear();

        return Ok(());
    }

    #[context("NetWorker::update()")]
    pub fn update(&mut self) -> Result<()> {
        loop {
            let current = match SystemTime::now().duration_since(self.started_at) {
                Ok(current) => current.as_millis() as u64,
                Err(_) => return Err(KCPError::Unexpected.into()),
            };

            let next = (current + KCP_INTERVAL) / KCP_INTERVAL * KCP_INTERVAL;
            let next_at = self.started_at + Duration::from_millis(next);

            self.handle_input()?;
            self.kcp.update_kcp(current);
            self.handle_output()?;
            self.kcp.update_udp(next_at)?;
            self.handle_timeout()?;
        }
    }

    pub fn finish(&mut self, err: Error, delay: bool) {
        println!("{:?}", err);

        let cause = match err.downcast::<KCPError>() {
            Ok(err) => err.cause(),
            Err(_) => NetFinishCause::ClientError,
        };
        self.chan.finish(cause);

        if !delay {
            return;
        }

        let deadline = SystemTime::now() + Duration::from_secs(FINISH_TIMEOUT);
        while SystemTime::now() < deadline {
            let now = SystemTime::now();
            let current = now.duration_since(self.started_at).unwrap().as_millis() as u64;

            self.kcp.update_kcp(current);
            let _ = self
                .kcp
                .update_udp(now + Duration::from_millis(KCP_INTERVAL));
        }
    }

    #[context("NetWorker::handle_input()")]
    fn handle_input(&mut self) -> Result<()> {
        loop {
            let mut frame = 0;
            let (commands, hash) = self.cmd_encoder.buffers();
            let state = self.chan.recv_input(&mut frame, commands, hash);
            match state {
                NetInputState::NonEmpty => {}
                NetInputState::Empty => return Ok(()),
                NetInputState::Finish => {
                    self.set_self_state(NetPlayerState::Stopped);
                    return Err(KCPError::GameOver.into());
                }
            };
            self.handle_input_impl(frame)?;
        }
    }

    #[context("NetWorker::handle_input_impl()")]
    fn handle_input_impl(&mut self, frame: u32) -> Result<()> {
        match self.state {
            NetPlayerState::Initing | NetPlayerState::Waiting => {
                let ce = &mut self.cmd_encoder;
                if !ce.commands().is_empty() || !ce.hash().is_empty() {
                    return Err(KCPError::Unexpected.into());
                }
            }
            NetPlayerState::Running => {
                if frame <= self.frame {
                    return Err(KCPError::InvalidFrame.into());
                }
                self.frame = frame;
                self.cmd_encoder.encode(self.frame)?;
                self.kcp.send_kcp(self.cmd_encoder.hash_bytes())?;
                self.kcp.send_kcp(self.cmd_encoder.command_bytes())?;
            }
            NetPlayerState::Stopped => {}
        }
        return Ok(());
    }

    #[context("NetWorker::handle_output()")]
    fn handle_output(&mut self) -> Result<()> {
        loop {
            self.kcp_buffer.clear();
            let len = self.kcp.recv_kcp(&mut self.kcp_buffer)?;
            if len == 0 {
                return Ok(());
            }
            self.handle_output_impl()?;
        }
    }

    #[context("NetWorker::handle_output_impl()")]
    fn handle_output_impl(&mut self) -> Result<()> {
        match self.state {
            NetPlayerState::Initing => {
                let (msg, _) = NetMessage::decode(&self.kcp_buffer)?;
                match msg {
                    NetMessage::Accept(_) => {
                        self.set_self_state(NetPlayerState::Waiting);
                    }
                    NetMessage::Finish(finish) => {
                        return Err(KCPError::RemoteFinished(finish.cause).into());
                    }
                    _ => return Err(KCPError::UnexpectedPacket.into()),
                };
            }
            NetPlayerState::Waiting => {
                let (msg, _) = NetMessage::decode(&self.kcp_buffer)?;
                match msg {
                    NetMessage::State(state) => {
                        self.set_state(state.conv, state.state);
                    }
                    NetMessage::Start(_) => {
                        self.set_self_state(NetPlayerState::Running);
                    }
                    NetMessage::Finish(finish) => {
                        return Err(KCPError::RemoteFinished(finish.cause).into());
                    }
                    _ => return Err(KCPError::UnexpectedPacket.into()),
                };
            }
            NetPlayerState::Running => {
                if Self::is_message_command(&self.kcp_buffer) {
                    self.updated_at = SystemTime::now();
                    self.cmd_decoder.decode(&self.kcp_buffer)?;
                    self.chan.send_output_commands(self.cmd_decoder.commands());
                } else {
                    let (msg, _) = NetMessage::decode(&self.kcp_buffer)?;
                    match msg {
                        NetMessage::State(state) => {
                            self.set_state(state.conv, state.state);
                        }
                        NetMessage::Finish(finish) => {
                            return Err(KCPError::RemoteFinished(finish.cause).into());
                        }
                        _ => return Err(KCPError::UnexpectedPacket.into()),
                    };
                }
            }
            // ignore all data
            NetPlayerState::Stopped => {}
        }
        return Ok(());
    }

    #[context("CommandEncoder::handle_timeout()")]
    fn handle_timeout(&mut self) -> Result<()> {
        match self.state {
            NetPlayerState::Initing => {
                let dura = self.started_at.elapsed().unwrap_or(Duration::ZERO);
                if dura.as_secs() > CONNECT_TIMEOUT {
                    return Err(KCPError::Timeout.into());
                }
            }
            NetPlayerState::Waiting => {
                let dura = self.started_at.elapsed().unwrap_or(Duration::ZERO);
                if dura.as_secs() > START_TIMEOUT {
                    return Err(KCPError::Timeout.into());
                }
            }
            NetPlayerState::Running => {}
            NetPlayerState::Stopped => {
                let dura = self.stopped_at.elapsed().unwrap_or(Duration::ZERO);
                if dura.as_secs() > UPDATE_TIMEOUT {
                    return Err(KCPError::Timeout.into());
                }
            }
        };
        return Ok(());
    }

    fn set_state(&mut self, conv: u32, state: NetPlayerState) {
        if conv != self.conv {
            self.chan.send_output_states(conv, state);
        }
    }

    fn set_self_state(&mut self, state: NetPlayerState) {
        self.state = state;
        self.chan.send_output_states(self.conv, state);
    }

    fn is_message_command(bytes: &[u8]) -> bool {
        if bytes.len() < KCP_MIN_PACKET {
            return false;
        }
        return NetType::from_i32(bytes[0] as i32) == Some(NetType::Command);
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::codec::{Command, CommandEx};
    use crate::message::{NetAccept, NetConnect, NetFinish, NetStart};
    use std::collections::HashMap;

    #[test]
    fn test_net_worker_input() {
        let chan = NetChan::new();
        let mut worker = NetWorker::new(
            SocketAddr::from(([138, 128, 196, 233], 33303)),
            6666,
            "",
            "",
            "",
            chan.clone(),
        )
        .unwrap();

        chan.send_input(1, &[], &[1, 2, 3]).unwrap();
        let err = worker.handle_input().unwrap_err();
        assert_eq!(
            err.downcast::<KCPError>().unwrap().to_string(),
            "unexpected error"
        );

        worker.state = NetPlayerState::Waiting;
        chan.send_input(2, &[], &[1, 2, 3]).unwrap();
        let err = worker.handle_input().unwrap_err();
        assert_eq!(
            err.downcast::<KCPError>().unwrap().to_string(),
            "unexpected error"
        );

        worker.state = NetPlayerState::Running;
        chan.send_input(0, &[], &[]).unwrap();
        let err = worker.handle_input().unwrap_err();
        assert_eq!(
            err.downcast::<KCPError>().unwrap().to_string(),
            "invalid frame"
        );

        chan.send_input(3, &[Command::Bbb(1.0, 1.0, 1.0)], &[9, 0, 9, 0])
            .unwrap();
        worker.handle_input().unwrap();
        worker.kcp.update_kcp(0);
        assert_eq!(worker.kcp.output_queue().len(), 1);

        chan.game_over().unwrap();
        let err = worker.handle_input().unwrap_err();
        assert_eq!(err.downcast::<KCPError>().unwrap().to_string(), "game over");
    }

    #[test]
    fn test_net_worker_output() {
        let chan = NetChan::new();
        let mut worker = NetWorker::new(
            SocketAddr::from(([138, 128, 196, 233], 33303)),
            6666,
            "",
            "",
            "",
            chan.clone(),
        )
        .unwrap();
        worker.handle_output().unwrap();

        let mut commands = Vec::<CommandEx>::new();
        let mut states = HashMap::<u32, NetPlayerState>::new();

        worker.kcp_buffer.clear();
        NetMessage::Accept(NetAccept::default())
            .encode(&mut worker.kcp_buffer)
            .unwrap();
        worker.handle_output_impl().unwrap();
        assert_eq!(worker.state, NetPlayerState::Waiting);
        chan.recv_output(&mut commands, &mut states).unwrap();
        assert_eq!(states[&worker.conv], NetPlayerState::Waiting);

        worker.kcp_buffer.clear();
        NetMessage::Start(NetStart::default())
            .encode(&mut worker.kcp_buffer)
            .unwrap();
        worker.handle_output_impl().unwrap();
        assert_eq!(worker.state, NetPlayerState::Running);
        chan.recv_output(&mut commands, &mut states).unwrap();
        assert_eq!(states[&worker.conv], NetPlayerState::Running);

        worker.kcp_buffer.clear();
        let mut ce = CommandEncoder::new(0);
        ce.commands().push(Command::Bbb(1.0, 1.0, 1.0));
        ce.encode(10).unwrap();
        worker.kcp_buffer.extend_from_slice(ce.command_bytes());
        worker.handle_output_impl().unwrap();
        chan.recv_output(&mut commands, &mut states).unwrap();
        assert_eq!(commands[0].command, Command::Bbb(1.0, 1.0, 1.0));
        assert_eq!(commands[0].frame, 10);

        for state in [
            NetPlayerState::Initing,
            NetPlayerState::Waiting,
            NetPlayerState::Running,
        ] {
            worker.state = state;
            worker.kcp_buffer.clear();
            NetMessage::Finish(NetFinish::default())
                .encode(&mut worker.kcp_buffer)
                .unwrap();
            let err = worker.handle_output_impl().unwrap_err();
            assert_eq!(
                err.downcast::<KCPError>().unwrap().to_string(),
                "remote finished"
            );
        }

        for state in [
            NetPlayerState::Initing,
            NetPlayerState::Waiting,
            NetPlayerState::Running,
        ] {
            worker.state = state;
            worker.kcp_buffer.clear();
            NetMessage::Connect(NetConnect::default())
                .encode(&mut worker.kcp_buffer)
                .unwrap();
            let err = worker.handle_output_impl().unwrap_err();
            assert_eq!(
                err.downcast::<KCPError>().unwrap().to_string(),
                "unexpected packet"
            );
        }
    }
}
