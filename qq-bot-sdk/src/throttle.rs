use crate::error::{Error, Result};
use parking_lot::Mutex;
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::time::sleep;

/// 限流器 - 滑动窗口算法
#[derive(Clone)]
pub struct RateLimiter {
    inner: Arc<Mutex<RateLimiterInner>>,
}

struct RateLimiterInner {
    window: VecDeque<Instant>,
    max_requests: usize,
    window_duration: Duration,
}

impl RateLimiter {
    /// 创建限流器
    /// - max_requests: 窗口内最大请求数
    /// - window_duration: 时间窗口长度
    pub fn new(max_requests: usize, window_duration: Duration) -> Self {
        Self {
            inner: Arc::new(Mutex::new(RateLimiterInner {
                window: VecDeque::new(),
                max_requests,
                window_duration,
            })),
        }
    }

    /// QQ 官方限流：每分钟 5 条私信，20 条频道消息
    pub fn default_private() -> Self {
        Self::new(5, Duration::from_secs(60))
    }

    pub fn default_channel() -> Self {
        Self::new(20, Duration::from_secs(60))
    }

    pub fn default_group() -> Self {
        Self::new(20, Duration::from_secs(60))
    }

    /// 等待直到可以发送请求
    pub async fn acquire(&self) -> Result<()> {
        loop {
            let wait_time = {
                let mut inner = self.inner.lock();
                let now = Instant::now();

                // 清理过期的请求记录
                while let Some(&front) = inner.window.front() {
                    if now.duration_since(front) > inner.window_duration {
                        inner.window.pop_front();
                    } else {
                        break;
                    }
                }

                // 检查是否可以发送
                if inner.window.len() < inner.max_requests {
                    inner.window.push_back(now);
                    return Ok(());
                }

                // 计算需要等待的时间
                if let Some(&oldest) = inner.window.front() {
                    let elapsed = now.duration_since(oldest);
                    Some(inner.window_duration.saturating_sub(elapsed))
                } else {
                    None
                }
            };

            if let Some(duration) = wait_time {
                // 添加小的随机抖动，避免所有请求同时发送
                let jitter = Duration::from_millis((duration.as_millis() as u64 % 100) + 50);
                sleep(duration + jitter).await;
            }
        }
    }

    /// 尝试获取许可（不等待）
    pub fn try_acquire(&self) -> bool {
        let mut inner = self.inner.lock();
        let now = Instant::now();

        // 清理过期的请求记录
        while let Some(&front) = inner.window.front() {
            if now.duration_since(front) > inner.window_duration {
                inner.window.pop_front();
            } else {
                break;
            }
        }

        // 检查是否可以发送
        if inner.window.len() < inner.max_requests {
            inner.window.push_back(now);
            true
        } else {
            false
        }
    }
}

/// 重试策略
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    pub max_retries: usize,
    pub initial_delay: Duration,
    pub max_delay: Duration,
    pub multiplier: f64,
    pub jitter: bool,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(30),
            multiplier: 2.0,
            jitter: true,
        }
    }
}

impl RetryPolicy {
    /// 使用指数退避算法执行操作
    pub async fn execute<F, Fut, T>(&self, mut operation: F) -> Result<T>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<T>>,
    {
        let mut attempt = 0;
        let mut delay = self.initial_delay;

        loop {
            match operation().await {
                Ok(result) => return Ok(result),
                Err(e) if attempt >= self.max_retries => return Err(e),
                Err(e) => {
                    attempt += 1;

                    // 判断错误是否可重试
                    if !self.is_retryable_error(&e) {
                        return Err(e);
                    }

                    // 计算延迟时间
                    let mut actual_delay = delay;
                    if self.jitter {
                        let jitter_ms = (delay.as_millis() as f64 * 0.2) as u64;
                        let jitter = Duration::from_millis(
                            rand::random::<u64>() % (jitter_ms * 2).max(1),
                        );
                        actual_delay = delay.saturating_add(jitter);
                    }

                    tracing::warn!(
                        "Operation failed (attempt {}/{}): {}. Retrying in {:?}",
                        attempt,
                        self.max_retries,
                        e,
                        actual_delay
                    );

                    sleep(actual_delay).await;

                    // 指数退避
                    delay = Duration::from_millis(
                        ((delay.as_millis() as f64) * self.multiplier) as u64,
                    );
                    delay = delay.min(self.max_delay);
                }
            }
        }
    }

    fn is_retryable_error(&self, error: &Error) -> bool {
        match error {
            Error::Http(e) => {
                // 网络错误通常可重试
                if e.is_timeout() || e.is_connect() {
                    return true;
                }

                // 5xx 服务器错误可重试
                if let Some(status) = e.status() {
                    return status.is_server_error() || status == reqwest::StatusCode::TOO_MANY_REQUESTS;
                }

                false
            }
            Error::Io(_) => true,
            Error::Auth(_) => false, // 认证错误不重试
            Error::Api(_) => false,  // API 错误不重试
            Error::WebSocket(_) => true,
            Error::Json(_) => false,
            Error::Other(_) => false,
            Error::InvalidPayload(_) => false,
            Error::ConnectionClosed => true,
            Error::ReconnectRequired => true,
        }
    }
}

/// 带限流和重试的请求包装器
#[derive(Clone)]
pub struct ThrottledClient {
    rate_limiter: RateLimiter,
    retry_policy: RetryPolicy,
}

impl ThrottledClient {
    pub fn new(rate_limiter: RateLimiter, retry_policy: RetryPolicy) -> Self {
        Self {
            rate_limiter,
            retry_policy,
        }
    }

    pub fn for_channel() -> Self {
        Self::new(RateLimiter::default_channel(), RetryPolicy::default())
    }

    pub fn for_private() -> Self {
        Self::new(RateLimiter::default_private(), RetryPolicy::default())
    }

    pub fn for_group() -> Self {
        Self::new(RateLimiter::default_group(), RetryPolicy::default())
    }

    /// 执行带限流和重试的操作
    pub async fn execute<F, Fut, T>(&self, operation: F) -> Result<T>
    where
        F: Fn() -> Fut + Send,
        Fut: std::future::Future<Output = Result<T>> + Send,
        T: Send,
    {
        self.retry_policy
            .execute(|| async {
                self.rate_limiter.acquire().await?;
                operation().await
            })
            .await
    }
}

// 简单的伪随机数生成（避免引入 rand 依赖）
mod rand {
    use std::cell::Cell;
    use std::time::{SystemTime, UNIX_EPOCH};

    thread_local! {
        static SEED: Cell<u64> = Cell::new(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos() as u64
        );
    }

    pub fn random<T: From<u64>>() -> T {
        SEED.with(|seed| {
            let mut s = seed.get();
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            seed.set(s);
            T::from(s)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_rate_limiter() {
        let limiter = RateLimiter::new(3, Duration::from_secs(1));

        // 前 3 个请求应该立即通过
        for _ in 0..3 {
            assert!(limiter.try_acquire());
        }

        // 第 4 个请求应该被拒绝
        assert!(!limiter.try_acquire());

        // 等待窗口过期
        sleep(Duration::from_secs(1)).await;

        // 现在应该可以再次获取
        assert!(limiter.try_acquire());
    }

    #[tokio::test]
    async fn test_retry_policy() {
        let policy = RetryPolicy {
            max_retries: 3,
            initial_delay: Duration::from_millis(10),
            max_delay: Duration::from_secs(1),
            multiplier: 2.0,
            jitter: false,
        };

        let mut attempt = 0;
        let result = policy
            .execute(|| async {
                attempt += 1;
                if attempt < 3 {
                    Err(Error::Other("temporary error".to_string()))
                } else {
                    Ok(42)
                }
            })
            .await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
        assert_eq!(attempt, 3);
    }
}
