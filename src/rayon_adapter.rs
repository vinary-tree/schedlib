use core::fmt;
use core::num::NonZeroUsize;

use rayon::prelude::*;
use rayon::{ThreadPool, ThreadPoolBuildError, ThreadPoolBuilder};

use crate::{BatchExecutor, BatchView, ControlModel, TaskCompletion, TaskExecution, TaskView};

/// Immutable task callback safe to share across Rayon workers.
///
/// The callback receives only an immutable task view. Any caller-owned shared
/// state must provide its own synchronization and must not violate the effect
/// independence declared when the plan was built.
pub trait ParallelTaskExecutor<T>: Sync {
    /// Successful output type.
    type Output;
    /// Terminal failure type.
    type Error;
    /// Structured incomplete reason type.
    type Incomplete;

    /// Executes one immutable task on a Rayon worker.
    fn execute(
        &self,
        task: TaskView<'_, T>,
    ) -> TaskExecution<Self::Output, Self::Error, Self::Incomplete>;
}

/// Configuration for one owned local Rayon pool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RayonConfig {
    worker_threads: NonZeroUsize,
    worker_stack_size: Option<NonZeroUsize>,
}

impl RayonConfig {
    /// Creates a configuration with an exact positive worker count and Rayon's
    /// default worker-stack size.
    #[must_use]
    pub const fn new(worker_threads: NonZeroUsize) -> Self {
        Self {
            worker_threads,
            worker_stack_size: None,
        }
    }

    /// Selects an exact positive native stack size for every worker thread.
    #[must_use]
    pub const fn with_worker_stack_size(mut self, worker_stack_size: NonZeroUsize) -> Self {
        self.worker_stack_size = Some(worker_stack_size);
        self
    }

    /// Returns the exact configured worker count.
    #[must_use]
    pub const fn worker_threads(self) -> NonZeroUsize {
        self.worker_threads
    }

    /// Returns the configured worker-stack size, or `None` for Rayon's
    /// platform default.
    #[must_use]
    pub const fn worker_stack_size(self) -> Option<NonZeroUsize> {
        self.worker_stack_size
    }
}

/// Failure to construct the owned local Rayon pool.
#[derive(Debug)]
pub struct RayonBuildError {
    source: ThreadPoolBuildError,
}

impl fmt::Display for RayonBuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "failed to construct schedlib Rayon pool: {}",
            self.source
        )
    }
}

impl std::error::Error for RayonBuildError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

/// Deterministic parallel adapter backed by one owned local Rayon pool.
///
/// Rayon may execute immutable tasks in any physical order. The indexed
/// parallel iterator collects completions in canonical batch order and does
/// not return until every current-batch worker has joined. schedlib's core then
/// revalidates exact coverage before ordered commit.
pub struct RayonExecutor<W> {
    worker: W,
    pool: ThreadPool,
    config: RayonConfig,
}

impl<W> RayonExecutor<W> {
    /// Constructs an owned local pool. No global Rayon configuration is read or
    /// mutated.
    ///
    /// # Errors
    ///
    /// Returns [`RayonBuildError`] when Rayon cannot create the configured
    /// worker pool.
    pub fn new(worker: W, config: RayonConfig) -> Result<Self, RayonBuildError> {
        let mut builder = ThreadPoolBuilder::new()
            .num_threads(config.worker_threads.get())
            .thread_name(|index| format!("schedlib-rayon-{index}"));
        if let Some(stack_size) = config.worker_stack_size {
            builder = builder.stack_size(stack_size.get());
        }
        let pool = builder
            .build()
            .map_err(|source| RayonBuildError { source })?;
        Ok(Self {
            worker,
            pool,
            config,
        })
    }

    /// Returns the immutable shared task callback.
    #[must_use]
    pub const fn worker(&self) -> &W {
        &self.worker
    }

    /// Returns the exact pool configuration.
    #[must_use]
    pub const fn config(&self) -> RayonConfig {
        self.config
    }

    /// Consumes the adapter, joins and drops its pool, and returns the callback.
    #[must_use]
    pub fn into_worker(self) -> W {
        self.worker
    }

    /// Returns the adapter's explicit control representation.
    #[must_use]
    pub const fn control_model(&self) -> ControlModel {
        ControlModel::ParallelIndexedJoin
    }
}

impl<T, W> BatchExecutor<T> for RayonExecutor<W>
where
    T: Sync,
    W: ParallelTaskExecutor<T>,
    W::Output: Send,
    W::Error: Send,
    W::Incomplete: Send,
{
    type Output = W::Output;
    type Error = W::Error;
    type Incomplete = W::Incomplete;

    fn execute_batch(
        &mut self,
        batch: BatchView<'_, T>,
    ) -> Vec<TaskCompletion<Self::Output, Self::Error, Self::Incomplete>> {
        let worker = &self.worker;
        self.pool.install(|| {
            batch
                .task_ids()
                .par_iter()
                .map(|task_id| {
                    let task = batch.known_task(*task_id);
                    TaskCompletion::new(*task_id, worker.execute(task))
                })
                .collect()
        })
    }
}
