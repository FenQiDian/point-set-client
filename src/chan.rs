use crate::base::{KCPError, COMMANDS_CAP, HASH_CAP, PLAYERS_CAP};
use crate::codec::{Command, CommandEx};
use crate::message::{NetFinishCause, NetPlayerState};
use anyhow::Result;
use fn_error_context::context;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::sync::Mutex;

#[derive(Debug)]
pub struct NetInput {
    pub frame: u32,
    pub commands: Vec<Command>,
    pub hash: Vec<u8>,
}

impl NetInput {
    fn new() -> NetInput {
        return NetInput {
            frame: 0,
            commands: Vec::with_capacity(COMMANDS_CAP),
            hash: Vec::with_capacity(HASH_CAP),
        };
    }

    fn clear(&mut self) {
        self.frame = 0;
        self.commands.clear();
        self.hash.clear();
    }
}

#[derive(Debug)]
enum NetInputWrap {
    Input(NetInput),
    Finish,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetInputState {
    Empty,
    NonEmpty,
    Finish,
}

#[derive(Debug)]
pub struct NetOutput {
    pub commands: Vec<CommandEx>,
    pub states: HashMap<u32, NetPlayerState>,
}

impl NetOutput {
    fn new() -> NetOutput {
        return NetOutput {
            commands: Vec::with_capacity(PLAYERS_CAP * 2),
            states: HashMap::with_capacity(COMMANDS_CAP),
        };
    }

    fn clear(&mut self) {
        self.commands.clear();
        self.states.clear();
    }
}

#[derive(Debug)]
pub struct NetChanImpl {
    cache_stack: Vec<NetInput>,
    input_queue: VecDeque<NetInputWrap>,
    output: NetOutput,
    finish_cause: Option<NetFinishCause>,
}

#[derive(Debug, Clone)]
pub struct NetChan(Arc<Mutex<NetChanImpl>>);

impl NetChan {
    pub fn new() -> NetChan {
        return NetChan(Arc::new(Mutex::new(NetChanImpl {
            cache_stack: Vec::with_capacity(3),
            input_queue: VecDeque::with_capacity(3),
            output: NetOutput::new(),
            finish_cause: None,
        })));
    }

    pub fn send_input(
        &self,
        frame: u32,
        commands: &[Command],
        hash: &[u8],
    ) -> Result<(), NetFinishCause> {
        let chan = &mut self.0.lock().unwrap();
        if let Some(cause) = chan.finish_cause {
            return Err(cause);
        }

        let mut input = chan.cache_stack.pop().unwrap_or(NetInput::new());
        input.frame = frame;
        input.commands.extend_from_slice(commands);
        input.hash.extend_from_slice(hash);
        chan.input_queue.push_back(NetInputWrap::Input(input));
        return Ok(());
    }

    pub fn recv_input(
        &self,
        frame: &mut u32,
        commands: &mut Vec<Command>,
        hash: &mut Vec<u8>,
    ) -> NetInputState {
        let chan = &mut self.0.lock().unwrap();
        let mut input = match chan.input_queue.pop_front() {
            Some(NetInputWrap::Input(input)) => input,
            Some(NetInputWrap::Finish) => return NetInputState::Finish,
            None => return NetInputState::Empty,
        };

        *frame = input.frame;
        commands.extend_from_slice(&input.commands);
        hash.extend_from_slice(&input.hash);
        input.clear();
        if chan.cache_stack.capacity() > chan.cache_stack.len() {
            chan.cache_stack.push(input);
        }
        return NetInputState::NonEmpty;
    }

    pub fn send_output_commands(&self, commands: &[CommandEx]) {
        let chan = &mut self.0.lock().unwrap();
        chan.output.commands.extend_from_slice(commands);
    }

    pub fn send_output_states(&self, conv: u32, state: NetPlayerState) {
        let chan = &mut self.0.lock().unwrap();
        chan.output.states.insert(conv, state);
    }

    pub fn recv_output(
        &self,
        commands: &mut Vec<CommandEx>,
        states: &mut HashMap<u32, NetPlayerState>,
    ) -> Result<(), NetFinishCause> {
        let chan = &mut self.0.lock().unwrap();
        if let Some(cause) = chan.finish_cause {
            return Err(cause);
        }

        commands.extend_from_slice(&chan.output.commands);
        states.clone_from(&chan.output.states);
        chan.output.clear();
        return Ok(());
    }

    pub fn game_over(&self) -> Result<(), NetFinishCause> {
        let chan = &mut self.0.lock().unwrap();
        if let Some(cause) = chan.finish_cause {
            return Err(cause);
        }

        chan.input_queue.push_back(NetInputWrap::Finish);
        return Ok(());
    }

    pub fn finish(&self, cause: NetFinishCause) {
        let chan = &mut self.0.lock().unwrap();
        chan.finish_cause = Some(cause);
    }
}

#[cfg(test)]
mod test {
    use super::*;
}
