use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tracing::{debug, warn};
use uuid::Uuid;
use std::process::Stdio;

pub struct ProcessManager {
    processes: Arc<Mutex<HashMap<String, ManagedProcess>>>,
    log_dir: PathBuf,
}

struct ManagedProcess {
    session_id: String,
    command: String,
    child: Child,
    log_path: PathBuf,
}

impl ProcessManager {
    pub fn new(log_dir: PathBuf) -> Self {
        Self {
            processes: Arc::new(Mutex::new(HashMap::new())),
            log_dir,
        }
    }

    pub async fn spawn(&self, command_str: &str, workdir: &str) -> anyhow::Result<String> {
        let session_id = Uuid::new_v4().to_string();
        let log_path = self.log_dir.join(format!("proc_{}.log", session_id));
        
        // Ensure log dir exists
        tokio::fs::create_dir_all(&self.log_dir).await?;

        let log_file = std::fs::File::create(&log_path)?;

        let mut child = Command::new("bash")
            .arg("-c")
            .arg(command_str)
            .current_dir(workdir)
            .stdout(Stdio::from(log_file.try_clone()?))
            .stderr(Stdio::from(log_file))
            .stdin(Stdio::piped())
            .spawn()?;

        let managed = ManagedProcess {
            session_id: session_id.clone(),
            command: command_str.to_string(),
            child,
            log_path,
        };

        self.processes.lock().await.insert(session_id.clone(), managed);
        Ok(session_id)
    }

    pub async fn read_logs(&self, session_id: &str, offset: u64, limit: usize) -> anyhow::Result<String> {
        let processes = self.processes.lock().await;
        let proc = processes.get(session_id).ok_or_else(|| anyhow::anyhow!("Process not found"))?;
        
        let mut file = File::open(&proc.log_path).await?;
        file.seek(std::io::SeekFrom::Start(offset)).await?;
        
        let mut buffer = vec![0; limit];
        let n = file.read(&mut buffer).await?;
        buffer.truncate(n);
        
        Ok(String::from_utf8_lossy(&buffer).to_string())
    }

    pub async fn write_stdin(&self, session_id: &str, data: &str) -> anyhow::Result<()> {
        let mut processes = self.processes.lock().await;
        let proc = processes.get_mut(session_id).ok_or_else(|| anyhow::anyhow!("Process not found"))?;
        
        if let Some(stdin) = proc.child.stdin.as_mut() {
            stdin.write_all(data.as_bytes()).await?;
            stdin.flush().await?;
            Ok(())
        } else {
            Err(anyhow::anyhow!("Stdin not available"))
        }
    }
    
    pub async fn kill(&self, session_id: &str) -> anyhow::Result<()> {
        let mut processes = self.processes.lock().await;
        if let Some(mut proc) = processes.remove(session_id) {
            proc.child.kill().await?;
        }
        Ok(())
    }
}
