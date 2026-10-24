//! Bounded newline JSON-RPC transport. No inherited environment or stderr export.
use super::{CallContext, Error, Limits};
use std::{collections::BTreeMap, path::PathBuf, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::{Child, ChildStdin, ChildStdout, Command},
    runtime::{Builder, Runtime},
};

/// Infrastructure-only port: implementations must honor the context on every IO.
pub trait Transport: Send {
    fn send(&mut self, frame: &[u8], context: &CallContext) -> Result<(), Error>;
    fn receive(&mut self, context: &CallContext) -> Result<Vec<u8>, Error>;
    fn close(&mut self);
}
pub trait TransportFactory: Send + Sync {
    fn open(&self, context: &CallContext, limits: &Limits) -> Result<Box<dyn Transport>, Error>;
}

/// Trusted operator configuration; arguments/environment are never taken from proposals.
/// No Debug implementation: configuration may hold adapter-resolved credentials.
pub struct StdioConfig {
    pub executable: PathBuf,
    pub arguments: Vec<String>,
    pub directory: PathBuf,
    pub environment: BTreeMap<String, String>,
}
struct StdioTransport {
    executor: Runtime,
    child: Child,
    input: Option<ChildStdin>,
    output: ChildStdout,
    pending: Vec<u8>,
    max_frame: usize,
    shutdown: Duration,
}
impl TransportFactory for StdioConfig {
    fn open(&self, context: &CallContext, limits: &Limits) -> Result<Box<dyn Transport>, Error> {
        context.check()?;
        if !self.executable.is_absolute() || !self.directory.is_absolute() {
            return Err(Error::Configuration);
        }
        let executor = Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| Error::Unavailable)?;
        let child = {
            let _entered = executor.enter();
            Command::new(&self.executable)
                .args(&self.arguments)
                .current_dir(&self.directory)
                .env_clear()
                .envs(&self.environment)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .kill_on_drop(true)
                .spawn()
                .map_err(|_| Error::Unavailable)?
        };
        let mut child = child;
        let input = child.stdin.take();
        let output = child.stdout.take().ok_or(Error::Unavailable)?;
        Ok(Box::new(StdioTransport {
            executor,
            child,
            input,
            output,
            pending: Vec::new(),
            max_frame: limits.frame_bytes,
            shutdown: limits.shutdown,
        }))
    }
}
impl Transport for StdioTransport {
    fn send(&mut self, frame: &[u8], context: &CallContext) -> Result<(), Error> {
        if frame.len() > self.max_frame || frame.contains(&b'\n') {
            return Err(Error::Limit);
        }
        let input = self.input.as_mut().ok_or(Error::Unavailable)?;
        let mut bytes = frame.to_vec();
        bytes.push(b'\n');
        let mut offset = 0;
        while offset < bytes.len() {
            context.check()?;
            match self.executor.block_on(async {
                tokio::time::timeout(context.slice(), input.write(&bytes[offset..])).await
            }) {
                Ok(Ok(0)) | Ok(Err(_)) => return Err(Error::Unavailable),
                Ok(Ok(n)) => offset += n,
                Err(_) => (),
            }
        }
        Ok(())
    }
    fn receive(&mut self, context: &CallContext) -> Result<Vec<u8>, Error> {
        loop {
            context.check()?;
            if let Some(end) = self.pending.iter().position(|b| *b == b'\n') {
                let mut line: Vec<u8> = self.pending.drain(..=end).collect();
                line.pop();
                if line.len() > self.max_frame {
                    return Err(Error::Limit);
                }
                return Ok(line);
            }
            if self.pending.len() > self.max_frame {
                return Err(Error::Limit);
            }
            let mut chunk = [0; 4096];
            match self.executor.block_on(async {
                tokio::time::timeout(context.slice(), self.output.read(&mut chunk)).await
            }) {
                Ok(Ok(0)) | Ok(Err(_)) => return Err(Error::Unavailable),
                Ok(Ok(n)) => self.pending.extend_from_slice(&chunk[..n]),
                Err(_) => (),
            }
        }
    }
    fn close(&mut self) {
        self.input.take();
        self.executor.block_on(async {
            if tokio::time::timeout(self.shutdown, self.child.wait())
                .await
                .is_err()
            {
                let _ = self.child.start_kill();
                let _ = tokio::time::timeout(self.shutdown, self.child.wait()).await;
            }
        });
    }
}
impl Drop for StdioTransport {
    fn drop(&mut self) {
        self.close();
    }
}
