//! Bounded newline framing. No application or policy semantics live here.
use std::io::{BufRead, BufReader, Read, Write};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportError {
    Io,
    Frame,
    Limit,
    Timeout,
}

impl TransportError {
    pub fn code(self) -> &'static str {
        match self {
            Self::Io => "CG_TRANSPORT_IO",
            Self::Frame => "CG_TRANSPORT_FRAME",
            Self::Limit => "CG_LIMIT_EXCEEDED",
            Self::Timeout => "CG_TRANSPORT_TIMEOUT",
        }
    }
}

/// Replaceable transport boundary; implementations must honor the supplied deadline.
pub trait Transport {
    fn receive(&mut self, timeout: Duration) -> Result<Option<Vec<u8>>, TransportError>;
    fn send(&mut self, frame: Vec<u8>, timeout: Duration) -> Result<(), TransportError>;
}

/// Private parent/child pipes only. Worker queues contain at most one frame.
/// Workers are detached: blocked OS reads/writes cannot prevent process shutdown.
pub struct StdioTransport {
    incoming: Receiver<Result<Option<Vec<u8>>, TransportError>>,
    outgoing: SyncSender<Vec<u8>>,
    written: Receiver<Result<(), TransportError>>,
    max_frame: usize,
}

pub fn read_frame(
    reader: &mut impl BufRead,
    max: usize,
) -> Result<Option<Vec<u8>>, TransportError> {
    let mut frame = Vec::new();
    reader
        .take(max.saturating_add(1) as u64)
        .read_until(b'\n', &mut frame)
        .map_err(|_| TransportError::Io)?;
    if frame.is_empty() {
        return Ok(None);
    }
    if frame.len() > max {
        return Err(TransportError::Limit);
    }
    if frame.pop() != Some(b'\n') {
        return Err(TransportError::Frame);
    }
    Ok(Some(frame))
}

impl StdioTransport {
    pub fn new(
        input: impl Read + Send + 'static,
        output: impl Write + Send + 'static,
        max_frame: usize,
    ) -> Self {
        let (incoming_tx, incoming) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let mut reader = BufReader::new(input);
            loop {
                let frame = read_frame(&mut reader, max_frame);
                let terminal = !matches!(frame, Ok(Some(_)));
                if incoming_tx.send(frame).is_err() || terminal {
                    break;
                }
            }
        });
        let (outgoing, output_rx) = mpsc::sync_channel::<Vec<u8>>(1);
        let (written_tx, written) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let mut writer = output;
            while let Ok(frame) = output_rx.recv() {
                let result = writer
                    .write_all(&frame)
                    .and_then(|()| writer.write_all(b"\n"))
                    .and_then(|()| writer.flush())
                    .map_err(|_| TransportError::Io);
                let terminal = result.is_err();
                if written_tx.send(result).is_err() || terminal {
                    break;
                }
            }
        });
        Self {
            incoming,
            outgoing,
            written,
            max_frame,
        }
    }
}

fn channel_error(error: mpsc::RecvTimeoutError) -> TransportError {
    match error {
        mpsc::RecvTimeoutError::Timeout => TransportError::Timeout,
        mpsc::RecvTimeoutError::Disconnected => TransportError::Io,
    }
}

impl Transport for StdioTransport {
    fn receive(&mut self, timeout: Duration) -> Result<Option<Vec<u8>>, TransportError> {
        self.incoming.recv_timeout(timeout).map_err(channel_error)?
    }
    fn send(&mut self, frame: Vec<u8>, timeout: Duration) -> Result<(), TransportError> {
        if frame.len().saturating_add(1) > self.max_frame {
            return Err(TransportError::Limit);
        }
        self.outgoing
            .try_send(frame)
            .map_err(|_| TransportError::Io)?;
        self.written.recv_timeout(timeout).map_err(channel_error)?
    }
}
