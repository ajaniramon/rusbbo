//! Drives the bots against a server and collects what each step makes the server send.

use std::io::{Read, Write};
use std::net::{Shutdown, TcpStream};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use havana_protocol::frame::{Decoder, Message, encode_client};

use crate::scenario::{Action, Step};

/// Longest a step may wait for its `wait_for` messages.
const WAIT_TIMEOUT: Duration = Duration::from_secs(5);
/// Hard cap on one step, so a server that never goes quiet cannot hang the run.
const STEP_CAP: Duration = Duration::from_secs(10);

/// What one bot saw during one step.
#[derive(Debug, Default, Clone)]
pub struct BotOutput {
    pub messages: Vec<Message>,
    pub closed: bool,
}

/// Everything observed during one step, indexed by bot.
#[derive(Debug, Clone)]
pub struct StepOutput {
    pub bots: Vec<BotOutput>,
    /// `wait_for` messages that never arrived.
    pub missing: Vec<u16>,
}

enum Event {
    Msg(usize, Message),
    Closed(usize),
}

struct Bot {
    stream: Option<TcpStream>,
}

pub struct Runner {
    target: String,
    bots: Vec<Bot>,
    tx: Sender<Event>,
    rx: Receiver<Event>,
}

impl Runner {
    pub fn new(target: &str, bot_count: usize) -> Self {
        let (tx, rx) = channel();
        Self {
            target: target.to_string(),
            bots: (0..bot_count).map(|_| Bot { stream: None }).collect(),
            tx,
            rx,
        }
    }

    pub fn run(&mut self, steps: &[Step]) -> Result<Vec<StepOutput>> {
        steps
            .iter()
            .enumerate()
            .map(|(i, s)| self.step(s).with_context(|| format!("step {i}")))
            .collect()
    }

    fn step(&mut self, step: &Step) -> Result<StepOutput> {
        match &step.action {
            Action::Connect => self.connect(step.bot)?,
            Action::Disconnect => {
                if let Some(s) = self.bots[step.bot].stream.take() {
                    let _ = s.shutdown(Shutdown::Both);
                }
            }
            Action::Send { header, body } => {
                let stream = self.bots[step.bot]
                    .stream
                    .as_mut()
                    .context("bot is not connected")?;
                stream.write_all(&encode_client(&Message::new(*header, body.clone())))?;
            }
        }
        Ok(self.collect(step))
    }

    fn connect(&mut self, bot: usize) -> Result<()> {
        if self.bots[bot].stream.is_some() {
            bail!("bot already connected");
        }
        let stream = TcpStream::connect(&self.target)
            .with_context(|| format!("connecting to {}", self.target))?;
        stream.set_nodelay(true)?;
        let mut reader = stream.try_clone()?;
        let tx = self.tx.clone();
        thread::spawn(move || {
            let mut decoder = Decoder::default();
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => {
                        let _ = tx.send(Event::Closed(bot));
                        return;
                    }
                    Ok(n) => {
                        decoder.push(&buf[..n]);
                        while let Some(msg) = decoder.next_server() {
                            if tx.send(Event::Msg(bot, msg)).is_err() {
                                return;
                            }
                        }
                    }
                }
            }
        });
        self.bots[bot].stream = Some(stream);
        Ok(())
    }

    /// Gathers messages until every `wait_for` arrived and the server stayed quiet for `settle_ms`.
    fn collect(&mut self, step: &Step) -> StepOutput {
        let mut out = StepOutput {
            bots: vec![BotOutput::default(); self.bots.len()],
            missing: vec![],
        };
        let mut pending = step.wait_for.clone();
        let settle = Duration::from_millis(step.settle_ms);
        let start = Instant::now();
        let mut last = Instant::now();
        loop {
            let waited_out = start.elapsed() >= WAIT_TIMEOUT;
            if (pending.is_empty() || waited_out) && last.elapsed() >= settle
                || start.elapsed() >= STEP_CAP
            {
                break;
            }
            let timeout = if pending.is_empty() || waited_out {
                settle.saturating_sub(last.elapsed())
            } else {
                WAIT_TIMEOUT.saturating_sub(start.elapsed())
            };
            match self.rx.recv_timeout(timeout.max(Duration::from_millis(1))) {
                Ok(Event::Msg(bot, msg)) => {
                    if bot == step.bot {
                        pending.retain(|h| *h != msg.header);
                    }
                    out.bots[bot].messages.push(msg);
                    last = Instant::now();
                }
                Ok(Event::Closed(bot)) => {
                    out.bots[bot].closed = true;
                    self.bots[bot].stream = None;
                    last = Instant::now();
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
        out.missing = pending;
        out
    }
}

impl Drop for Runner {
    fn drop(&mut self) {
        for bot in &mut self.bots {
            if let Some(s) = bot.stream.take() {
                let _ = s.shutdown(Shutdown::Both);
            }
        }
    }
}
