//! Linux process adapter: bounded wall time, CPU, address space and output files.
//! Commands are host-configured immutable scripts, never worker-supplied argv.
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::PathBuf,
    process::{Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
#[derive(Clone)]
pub struct BoundedProcess {
    pub interpreter: PathBuf,
    pub script: PathBuf,
    pub work_root: PathBuf,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessError {
    InvalidLimits,
    Unavailable,
    Failed,
    Output,
}
struct Cleanup(PathBuf);
impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
impl BoundedProcess {
    pub fn run(
        &self,
        operation: &str,
        input: &[u8],
        wall_ms: u64,
        memory_bytes: u64,
        cpu_seconds: u64,
        output_bytes: usize,
    ) -> Result<Vec<u8>, ProcessError> {
        if wall_ms == 0
            || wall_ms > 3_600_000
            || memory_bytes < 16_777_216
            || cpu_seconds == 0
            || output_bytes == 0
            || output_bytes > 16_777_216
            || input.len() > 16_777_216
            || !self.interpreter.is_absolute()
            || !self.script.is_absolute()
        {
            return Err(ProcessError::InvalidLimits);
        }
        let path = self.work_root.join(format!(
            "cg-job-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let mut directory = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            directory.mode(0o700);
        }
        directory
            .create(&path)
            .map_err(|_| ProcessError::Unavailable)?;
        let _cleanup = Cleanup(path.clone());
        let mut stdin =
            File::create(path.join("input.json")).map_err(|_| ProcessError::Unavailable)?;
        stdin
            .write_all(input)
            .map_err(|_| ProcessError::Unavailable)?;
        let stdin = File::open(path.join("input.json")).map_err(|_| ProcessError::Unavailable)?;
        let stdout =
            File::create(path.join("output.json")).map_err(|_| ProcessError::Unavailable)?;
        let status = Command::new("/usr/bin/prlimit")
            .args([
                format!("--as={memory_bytes}"),
                format!("--cpu={cpu_seconds}"),
                format!("--fsize={output_bytes}"),
                "--".into(),
                "/usr/bin/timeout".into(),
                "--signal=KILL".into(),
                format!("{}s", wall_ms as f64 / 1000.0),
            ])
            .arg(&self.interpreter)
            .arg("-I")
            .arg(&self.script)
            .arg(operation)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .current_dir(&path)
            .stdin(Stdio::from(stdin))
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::null())
            .status()
            .map_err(|_| ProcessError::Unavailable)?;
        if !status.success() {
            return Err(ProcessError::Failed);
        }
        let mut output = Vec::new();
        File::open(path.join("output.json"))
            .map_err(|_| ProcessError::Output)?
            .take(output_bytes as u64 + 1)
            .read_to_end(&mut output)
            .map_err(|_| ProcessError::Output)?;
        if output.is_empty() || output.len() > output_bytes {
            return Err(ProcessError::Output);
        }
        Ok(output)
    }
}
