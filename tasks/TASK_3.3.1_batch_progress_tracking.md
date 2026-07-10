# TASK 3.3.1: Batch Job Progress Tracking

**状态**: 🔄 待执行 (0%)  
**优先级**: P1  
**预计工作量**: 4-5 小时  
**依赖**: 无

## 📋 任务目标

为批处理作业添加详细的进度跟踪功能，支持实时查询作业状态、进度百分比和阶段信息。

## 🎯 成功标准

- [x] 批处理作业支持进度跟踪
- [x] 提供进度查询 API
- [x] 支持多阶段进度报告
- [x] 实时进度更新（WebSocket）
- [x] 进度持久化到数据库
- [x] 单元测试覆盖 ≥ 90%

## 🔧 实现步骤

### 1. 扩展批处理作业状态

**文件**: `crates/hakimi-batch/src/job.rs`

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchJob {
    pub id: String,
    pub name: String,
    pub status: JobStatus,
    pub progress: JobProgress,
    pub created_at: i64,
    pub started_at: Option<i64>,
    pub completed_at: Option<i64>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobProgress {
    pub current_step: usize,
    pub total_steps: usize,
    pub current_stage: String,
    pub percentage: f64,
    pub items_processed: usize,
    pub items_total: usize,
    pub stages: Vec<StageProgress>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageProgress {
    pub name: String,
    pub status: StageStatus,
    pub started_at: Option<i64>,
    pub completed_at: Option<i64>,
    pub items_processed: usize,
    pub items_total: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum StageStatus {
    Pending,
    Running,
    Completed,
    Failed,
}

impl JobProgress {
    pub fn new(total_steps: usize, stages: Vec<String>) -> Self {
        Self {
            current_step: 0,
            total_steps,
            current_stage: stages.first().cloned().unwrap_or_default(),
            percentage: 0.0,
            items_processed: 0,
            items_total: 0,
            stages: stages.into_iter().map(|name| StageProgress {
                name,
                status: StageStatus::Pending,
                started_at: None,
                completed_at: None,
                items_processed: 0,
                items_total: 0,
            }).collect(),
        }
    }
    
    pub fn update_step(&mut self, step: usize) {
        self.current_step = step;
        self.percentage = (step as f64 / self.total_steps as f64) * 100.0;
    }
    
    pub fn start_stage(&mut self, stage_name: &str) {
        self.current_stage = stage_name.to_string();
        
        if let Some(stage) = self.stages.iter_mut().find(|s| s.name == stage_name) {
            stage.status = StageStatus::Running;
            stage.started_at = Some(chrono::Utc::now().timestamp());
        }
    }
    
    pub fn complete_stage(&mut self, stage_name: &str) {
        if let Some(stage) = self.stages.iter_mut().find(|s| s.name == stage_name) {
            stage.status = StageStatus::Completed;
            stage.completed_at = Some(chrono::Utc::now().timestamp());
        }
    }
    
    pub fn update_stage_items(&mut self, stage_name: &str, processed: usize, total: usize) {
        if let Some(stage) = self.stages.iter_mut().find(|s| s.name == stage_name) {
            stage.items_processed = processed;
            stage.items_total = total;
        }
        
        self.items_processed = processed;
        self.items_total = total;
    }
}
```

### 2. 实现进度存储

**文件**: `crates/hakimi-batch/src/progress_store.rs` (新建)

```rust
use sqlx::SqlitePool;

pub struct ProgressStore {
    pool: SqlitePool,
}

impl ProgressStore {
    pub async fn new(db_path: &str) -> Result<Self> {
        let pool = SqlitePool::connect(db_path).await?;
        Self::init_schema(&pool).await?;
        Ok(Self { pool })
    }
    
    async fn init_schema(pool: &SqlitePool) -> Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS job_progress (
                job_id TEXT PRIMARY KEY,
                progress_json TEXT NOT NULL,
                updated_at INTEGER NOT NULL
            )
            "#
        )
        .execute(pool)
        .await?;
        
        Ok(())
    }
    
    pub async fn save_progress(&self, job_id: &str, progress: &JobProgress) -> Result<()> {
        let progress_json = serde_json::to_string(progress)?;
        let updated_at = chrono::Utc::now().timestamp();
        
        sqlx::query(
            r#"
            INSERT INTO job_progress (job_id, progress_json, updated_at)
            VALUES (?, ?, ?)
            ON CONFLICT(job_id) DO UPDATE SET
                progress_json = excluded.progress_json,
                updated_at = excluded.updated_at
            "#
        )
        .bind(job_id)
        .bind(progress_json)
        .bind(updated_at)
        .execute(&self.pool)
        .await?;
        
        Ok(())
    }
    
    pub async fn get_progress(&self, job_id: &str) -> Result<Option<JobProgress>> {
        let row = sqlx::query(
            "SELECT progress_json FROM job_progress WHERE job_id = ?"
        )
        .bind(job_id)
        .fetch_optional(&self.pool)
        .await?;
        
        if let Some(row) = row {
            let progress_json: String = row.get(0);
            let progress = serde_json::from_str(&progress_json)?;
            Ok(Some(progress))
        } else {
            Ok(None)
        }
    }
}
```

### 3. 集成进度跟踪到批处理执行器

**文件**: `crates/hakimi-batch/src/executor.rs`

```rust
pub struct BatchExecutor {
    progress_store: Arc<ProgressStore>,
    progress_notifier: Arc<ProgressNotifier>,
}

impl BatchExecutor {
    pub async fn execute_job(&self, job: &mut BatchJob) -> Result<()> {
        job.status = JobStatus::Running;
        job.started_at = Some(chrono::Utc::now().timestamp());
        
        // 初始化进度
        job.progress = JobProgress::new(
            job.total_steps(),
            job.stage_names(),
        );
        
        self.save_and_notify_progress(job).await?;
        
        // 执行各阶段
        for (idx, stage_name) in job.stage_names().iter().enumerate() {
            job.progress.start_stage(stage_name);
            self.save_and_notify_progress(job).await?;
            
            // 执行阶段
            match self.execute_stage(job, stage_name).await {
                Ok(_) => {
                    job.progress.complete_stage(stage_name);
                    job.progress.update_step(idx + 1);
                }
                Err(e) => {
                    job.status = JobStatus::Failed;
                    job.error = Some(e.to_string());
                    self.save_and_notify_progress(job).await?;
                    return Err(e);
                }
            }
            
            self.save_and_notify_progress(job).await?;
        }
        
        job.status = JobStatus::Completed;
        job.completed_at = Some(chrono::Utc::now().timestamp());
        job.progress.percentage = 100.0;
        
        self.save_and_notify_progress(job).await?;
        
        Ok(())
    }
    
    async fn save_and_notify_progress(&self, job: &BatchJob) -> Result<()> {
        self.progress_store.save_progress(&job.id, &job.progress).await?;
        self.progress_notifier.notify(&job.id, &job.progress).await?;
        Ok(())
    }
}
```

### 4. 实现 WebSocket 进度通知

**文件**: `crates/hakimi-batch/src/progress_notifier.rs` (新建)

```rust
use tokio::sync::broadcast;

pub struct ProgressNotifier {
    tx: broadcast::Sender<ProgressUpdate>,
}

#[derive(Clone, Serialize)]
pub struct ProgressUpdate {
    pub job_id: String,
    pub progress: JobProgress,
    pub timestamp: i64,
}

impl ProgressNotifier {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(100);
        Self { tx }
    }
    
    pub async fn notify(&self, job_id: &str, progress: &JobProgress) -> Result<()> {
        let update = ProgressUpdate {
            job_id: job_id.to_string(),
            progress: progress.clone(),
            timestamp: chrono::Utc::now().timestamp(),
        };
        
        let _ = self.tx.send(update);
        Ok(())
    }
    
    pub fn subscribe(&self) -> broadcast::Receiver<ProgressUpdate> {
        self.tx.subscribe()
    }
}
```

### 5. 添加进度查询 API

**文件**: `crates/hakimi-server/src/routes/batch.rs`

```rust
// GET /api/batch/jobs/:id/progress
pub async fn get_job_progress(
    Path(job_id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<JobProgress>, StatusCode> {
    let progress_store = &state.batch_executor.progress_store;
    
    let progress = progress_store.get_progress(&job_id).await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    
    Ok(Json(progress))
}

// WebSocket /api/batch/jobs/:id/progress/stream
pub async fn stream_job_progress(
    ws: WebSocketUpgrade,
    Path(job_id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Response {
    ws.on_upgrade(move |socket| handle_progress_stream(socket, job_id, state))
}

async fn handle_progress_stream(
    socket: WebSocket,
    job_id: String,
    state: Arc<AppState>,
) {
    let mut rx = state.batch_executor.progress_notifier.subscribe();
    let (mut tx, _) = socket.split();
    
    while let Ok(update) = rx.recv().await {
        if update.job_id == job_id {
            let msg = serde_json::to_string(&update).unwrap();
            if tx.send(Message::Text(msg)).await.is_err() {
                break;
            }
        }
    }
}
```

### 6. 单元测试

**文件**: `crates/hakimi-batch/src/progress_test.rs`

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_progress_initialization() {
        // 测试进度初始化
    }
    
    #[tokio::test]
    async fn test_stage_progression() {
        // 测试阶段推进
    }
    
    #[tokio::test]
    async fn test_progress_persistence() {
        // 测试进度持久化
    }
    
    #[tokio::test]
    async fn test_progress_notification() {
        // 测试进度通知
    }
    
    #[tokio::test]
    async fn test_percentage_calculation() {
        // 测试百分比计算
    }
}
```

## 🔍 验证清单

- [ ] 所有单元测试通过
- [ ] 进度正确跟踪各阶段状态
- [ ] WebSocket 实时推送进度更新
- [ ] 进度持久化到数据库
- [ ] 进度查询 API 返回正确数据
- [ ] 百分比计算准确
- [ ] 并发作业进度互不干扰

## 📊 性能指标

- 进度更新延迟: < 100ms
- WebSocket 推送延迟: < 50ms
- 进度查询响应: < 20ms
- 并发通知数: > 100 个作业

## 🔗 相关文件

- `crates/hakimi-batch/src/job.rs`
- `crates/hakimi-batch/src/progress_store.rs` (新建)
- `crates/hakimi-batch/src/progress_notifier.rs` (新建)
- `crates/hakimi-batch/src/executor.rs`
- `crates/hakimi-server/src/routes/batch.rs`

## 📝 注意事项

1. 进度更新应批量持久化以减少数据库写入
2. WebSocket 连接应限制数量避免资源耗尽
3. 长时间运行的作业应定期保存检查点
4. 支持作业失败后从检查点恢复
5. 考虑进度数据的清理策略（保留最近 N 天）
