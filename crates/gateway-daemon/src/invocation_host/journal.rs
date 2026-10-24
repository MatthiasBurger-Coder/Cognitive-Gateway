//! Durable conservative reservation before each model call; replay never calls
//! a transport. The existing session coordinator still owns task lifecycle.
use super::*;
use gateway_application::invocation_resilience::{
    BoundedInvocationPort, BoundedInvocationRequest, InvocationAttemptResponse,
};
use gateway_domain::invocation_resilience::*;
use std::{
    cell::RefCell,
    fs::{File, OpenOptions},
    io::Write,
    sync::atomic::{AtomicBool, Ordering},
    thread,
};

#[derive(Default, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Usage {
    attempts: u16,
    input_tokens: u64,
    output_tokens: u64,
    cost_microunits: u64,
    output_bytes: u64,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Ledger {
    schema_version: SchemaVersion,
    authority: ContentDigest,
    deadline_ms: u64,
    usage: Usage,
    records: BTreeMap<String, Value>,
}
pub(super) struct Journal<'a> {
    host: &'a InvocationHost,
    path: PathBuf,
    _lock: File,
    data: RefCell<Ledger>,
    raw: RefCell<Option<Vec<u8>>>,
}
fn location(host: &InvocationHost) -> PathBuf {
    let a = &host.admission;
    let key = context_content_digest(
        &serde_json::to_vec(&(&a.principal, &a.scope, &a.budget_group))
            .expect("budget identity JSON"),
    );
    a.journal_directory.join(key.as_str())
}
fn directory(host: &InvocationHost) -> Result<()> {
    let dir = &host.admission.journal_directory;
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    input(builder.create(dir))?;
    if input(std::fs::symlink_metadata(dir))?
        .file_type()
        .is_symlink()
    {
        return Err("CG_INVALID_INPUT");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if input(std::fs::metadata(dir))?.permissions().mode() & 0o077 != 0 {
            return Err("CG_PRIVATE_STORE_REQUIRED");
        }
    }
    Ok(())
}
fn cancelled(host: &InvocationHost) -> bool {
    location(host).with_extension("cancel").exists()
}
pub(super) fn cancel(host: &InvocationHost) -> Result<Value> {
    directory(host)?;
    let file = input(
        OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(location(host).with_extension("cancel")),
    )?;
    input(file.sync_all())?;
    Ok(json!({"status":"CANCELLED","budget_group":host.admission.budget_group}))
}
impl<'a> Journal<'a> {
    pub fn open(host: &'a InvocationHost) -> Result<Self> {
        directory(host)?;
        let path = location(host).with_extension("json");
        let lock = input(
            OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(path.with_extension("lock")),
        )?;
        input(lock.lock())?;
        let data = if path.exists() {
            input(serde_json::from_slice::<Ledger>(&read(&path)?))?
        } else {
            Ledger {
                schema_version: SchemaVersion::V1,
                authority: host.digest.clone(),
                deadline_ms: now()
                    .saturating_add(u64::from(host.admission.limits.timeout_ms))
                    .min(host.admission.expires_at_ms),
                usage: Usage::default(),
                records: BTreeMap::new(),
            }
        };
        if data.schema_version != SchemaVersion::V1 || data.authority != host.digest {
            return Err("CG_STALE_REVISION");
        }
        let journal = Self {
            host,
            path,
            _lock: lock,
            data: RefCell::new(data),
            raw: RefCell::new(None),
        };
        journal.save()?;
        Ok(journal)
    }
    fn save(&self) -> Result<()> {
        let bytes = input(serde_json::to_vec(&*self.data.borrow()))?;
        if bytes.len() > 1_048_576 {
            return Err("CG_LIMIT_EXCEEDED");
        }
        let temp = self
            .path
            .with_extension(format!("{}.tmp", std::process::id()));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = input(options.open(&temp))?;
        let result = (|| {
            input(file.write_all(&bytes))?;
            input(file.sync_all())?;
            input(std::fs::rename(&temp, &self.path))?;
            #[cfg(unix)]
            {
                input(
                    input(File::open(self.path.parent().expect("journal directory")))?.sync_all(),
                )?;
            }
            Ok(())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(temp);
        }
        result
    }
    pub fn remaining_ms(&self) -> Result<u64> {
        if cancelled(self.host) {
            return Err("CG_CANCELLED");
        }
        self.data
            .borrow()
            .deadline_ms
            .checked_sub(now())
            .filter(|n| *n > 0)
            .ok_or("CG_DEADLINE_EXCEEDED")
    }
    pub fn remaining(&self) -> Result<InvocationRunLimits> {
        self.remaining_ms()?;
        let mut l = self.host.admission.limits.clone();
        let data = self.data.borrow();
        let used = &data.usage;
        l.max_attempts = l
            .max_attempts
            .checked_sub(used.attempts)
            .ok_or("CG_BUDGET_EXCEEDED")?;
        l.run_input_tokens = l
            .run_input_tokens
            .checked_sub(used.input_tokens)
            .ok_or("CG_BUDGET_EXCEEDED")?;
        l.run_output_tokens = l
            .run_output_tokens
            .checked_sub(used.output_tokens)
            .ok_or("CG_BUDGET_EXCEEDED")?;
        l.cost_microunits = l
            .cost_microunits
            .checked_sub(used.cost_microunits)
            .ok_or("CG_BUDGET_EXCEEDED")?;
        l.output_bytes = l
            .output_bytes
            .checked_sub(used.output_bytes)
            .ok_or("CG_BUDGET_EXCEEDED")?;
        l.validate().map_err(|_| "CG_BUDGET_EXCEEDED")?;
        Ok(l)
    }
    pub fn summary(&self) -> Value {
        let data = self.data.borrow();
        json!({"budget_group":self.host.admission.budget_group,"usage":data.usage,"deadline_ms":data.deadline_ms,"cancelled":cancelled(self.host),"invocations":data.records.keys().collect::<Vec<_>>()})
    }
    pub fn unused(&self, id: &str) -> Result<()> {
        if self.data.borrow().records.contains_key(id) {
            Err("CG_DUPLICATE_INVOCATION")
        } else {
            self.remaining_ms().map(|_| ())
        }
    }
    pub fn record(&self, id: &str) -> Result<Value> {
        self.data
            .borrow()
            .records
            .get(id)
            .cloned()
            .ok_or("CG_UNAVAILABLE")
    }
    pub fn raw(&self) -> Option<Vec<u8>> {
        self.raw.borrow().clone()
    }
    pub fn finish(&mut self, id: &str, value: Value) -> Result<()> {
        self.data.borrow_mut().records.insert(id.into(), value);
        self.save()
    }
    pub fn transport<'b>(
        &'b self,
        adapter: &'b dyn BoundedInvocationPort,
        id: &'b str,
    ) -> Transport<'b, 'a> {
        Transport {
            journal: self,
            adapter,
            id,
        }
    }
}
pub(super) struct Transport<'a, 'h> {
    journal: &'a Journal<'h>,
    adapter: &'a dyn BoundedInvocationPort,
    id: &'a str,
}
impl BoundedInvocationPort for Transport<'_, '_> {
    fn invoke(&self, r: BoundedInvocationRequest<'_>) -> InvocationAttemptResponse {
        let fail = || InvocationAttemptResponse {
            output: Err(InvocationAttemptFailure::Hard),
            usage: None,
        };
        let j = self.journal;
        if j.remaining_ms().is_err() || j.host.current().is_err() {
            return fail();
        }
        let input = r.rendered.rendered().accounting().total_input_tokens;
        let output = r.rendered.rendered().accounting().reserved_output_tokens;
        let Ok(left) = j.remaining() else {
            return fail();
        };
        if input > left.run_input_tokens
            || output > left.run_output_tokens
            || r.reserved_cost_microunits > left.cost_microunits
            || r.max_output_bytes as u64 > left.output_bytes
        {
            return fail();
        }
        {
            let mut data = j.data.borrow_mut();
            let u = &mut data.usage;
            u.attempts = u.attempts.saturating_add(1);
            u.input_tokens = u.input_tokens.saturating_add(input);
            u.output_tokens = u.output_tokens.saturating_add(output);
            u.cost_microunits = u.cost_microunits.saturating_add(r.reserved_cost_microunits);
            u.output_bytes = u.output_bytes.saturating_add(r.max_output_bytes as u64);
            data.records.insert(
                self.id.into(),
                json!({"status":"IN_FLIGHT","binding":r.binding,"ir":r.ir,"rendered":r.rendered}),
            );
        }
        if j.save().is_err() {
            return fail();
        }
        let byte_reserve = r.max_output_bytes as u64;
        let stop = AtomicBool::new(false);
        let response = thread::scope(|scope| {
            let context = r.context.clone();
            let stop = &stop;
            let cancellation = location(j.host).with_extension("cancel");
            scope.spawn(move || {
                while !stop.load(Ordering::Acquire) {
                    if cancellation.exists() {
                        context.cancel();
                        break;
                    }
                    thread::sleep(Duration::from_millis(5));
                }
            });
            let response = self.adapter.invoke(r);
            stop.store(true, Ordering::Release);
            response
        });
        if let Ok(raw) = &response.output {
            *j.raw.borrow_mut() = Some(raw.clone());
        }
        // Refund byte reservation only when exact returned bytes are available.
        // Token/output/cost reservations remain conservative across process restarts.
        {
            let mut data = j.data.borrow_mut();
            if let Ok(raw) = &response.output {
                data.usage.output_bytes = data
                    .usage
                    .output_bytes
                    .saturating_sub(byte_reserve)
                    .saturating_add(raw.len() as u64);
            }
            if let Some(usage) = &response.usage {
                data.usage.input_tokens = data
                    .usage
                    .input_tokens
                    .saturating_add(usage.input_tokens.saturating_sub(input));
                data.usage.output_tokens = data
                    .usage
                    .output_tokens
                    .saturating_add(usage.output_tokens.saturating_sub(output));
                data.usage.cost_microunits = data.usage.cost_microunits.saturating_add(
                    usage
                        .cost_microunits
                        .saturating_sub(j.host.admission.cost_per_attempt_microunits),
                );
            }
        }
        if j.save().is_err() {
            return fail();
        }
        response
    }
}
