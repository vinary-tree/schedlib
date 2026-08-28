//! Formally refined deterministic scheduling for Vinary pipelines.
//!
//! schedlib separates immutable deterministic planning from execution. The
//! core is synchronous and runtime-neutral: callers inject either the serial
//! reference executor or a batch executor whose arbitrary completion order is
//! normalized by canonical ordered commit.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod effects;
mod execution;
mod plan;
#[cfg(feature = "rayon")]
mod rayon_adapter;
mod types;

pub use effects::{EffectSet, IndependenceWitness, TaskEffects};
pub use execution::{
    BatchExecutor, BatchView, CancelAfter, Cancellation, CommitSink, CommittedTask, ExecutionError,
    ExecutionPhase, ExecutionReport, ExecutionWorkProfile, NeverCancel, SerialExecutor,
    TaskCompletion, TaskExecution, TaskExecutor, TaskView,
};
pub use plan::{ControlModel, Plan, PlanBatch, PlanBuilder, PlanError, PlanWorkProfile, TaskSpec};
#[cfg(feature = "rayon")]
pub use rayon_adapter::{ParallelTaskExecutor, RayonBuildError, RayonConfig, RayonExecutor};
pub use types::{Budget, Cost, ResourceId, TaskId};
