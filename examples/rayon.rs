use std::num::NonZeroUsize;

use schedlib::{
    Budget, CommitSink, CommittedTask, Cost, EffectSet, NeverCancel, ParallelTaskExecutor,
    PlanBuilder, RayonConfig, RayonExecutor, TaskEffects, TaskExecution, TaskId, TaskSpec,
    TaskView,
};

struct LengthWorker;

impl ParallelTaskExecutor<&'static str> for LengthWorker {
    type Output = usize;
    type Error = &'static str;
    type Incomplete = &'static str;

    fn execute(
        &self,
        task: TaskView<'_, &'static str>,
    ) -> TaskExecution<Self::Output, Self::Error, Self::Incomplete> {
        TaskExecution::Success(task.payload().len())
    }
}

#[derive(Default)]
struct Published(Vec<TaskId>);

impl CommitSink<usize, &'static str, &'static str> for Published {
    fn commit(&mut self, result: &CommittedTask<usize, &'static str, &'static str>) {
        self.0.push(result.task_id());
    }
}

fn main() {
    let no_effects = || {
        TaskEffects::new(
            EffectSet::from_resources(std::iter::empty()),
            EffectSet::from_resources(std::iter::empty()),
        )
    };
    let tasks = vec![
        TaskSpec::new(
            TaskId::new(10),
            "parse",
            no_effects(),
            Cost::new(1).expect("one is positive"),
        ),
        TaskSpec::new(
            TaskId::new(20),
            "optimize",
            no_effects(),
            Cost::new(1).expect("one is positive"),
        ),
    ];
    let plan = PlanBuilder::new(Budget::new(2).expect("two is positive"))
        .tasks(tasks)
        .dependencies(std::iter::empty())
        .build()
        .expect("the tasks are independent and fit one batch");
    let config = RayonConfig::new(NonZeroUsize::new(2).expect("two is positive"));
    let mut executor =
        RayonExecutor::new(LengthWorker, config).expect("the local worker pool starts");
    let mut cancellation = NeverCancel;
    let mut published = Published::default();
    let report = plan
        .execute(&mut executor, &mut cancellation, &mut published)
        .expect("the adapter returns one completion per task");

    assert_eq!(published.0, vec![TaskId::new(10), TaskId::new(20)]);
    assert_eq!(report.committed().len(), 2);
}
