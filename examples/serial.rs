use schedlib::{
    Budget, CommitSink, CommittedTask, Cost, EffectSet, NeverCancel, PlanBuilder, SerialExecutor,
    TaskEffects, TaskExecution, TaskExecutor, TaskId, TaskSpec, TaskView,
};

struct LengthWorker;

impl TaskExecutor<&'static str> for LengthWorker {
    type Output = usize;
    type Error = &'static str;
    type Incomplete = &'static str;

    fn execute(
        &mut self,
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
        .dependencies([(TaskId::new(10), TaskId::new(20))])
        .build()
        .expect("the dependency is acyclic and both tasks fit the budget");

    let mut executor = SerialExecutor::new(LengthWorker);
    let mut cancellation = NeverCancel;
    let mut published = Published::default();
    let report = plan
        .execute(&mut executor, &mut cancellation, &mut published)
        .expect("the serial executor returns exactly one completion per task");

    assert_eq!(published.0, vec![TaskId::new(10), TaskId::new(20)]);
    assert_eq!(report.committed().len(), 2);
}
