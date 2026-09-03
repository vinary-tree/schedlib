use std::collections::BTreeSet;
use std::thread;

use schedlib::durable::{
    Checkpoint, CodecLimits, CodecUsage, CrashPoint, DurableEvent, DurableJournal, DurableOutcome,
    ExternalKeyMap, PlanIdentity, ProtocolInput, PublicationLedger, ReplayClass, ResumeMachine,
    TerminalPhase,
};

type Identity = PlanIdentity<u64, &'static str>;
type Event = DurableEvent<u64, u64, u64, u64>;

fn identity(task_count: u64) -> Identity {
    let keys: Vec<_> = (0..task_count).map(|key| 10_000 + key).collect();
    let dependencies: Vec<_> = (1..task_count)
        .map(|target| (10_000 + target - 1, 10_000 + target))
        .collect();
    let effects: Vec<_> = (0..task_count)
        .map(|task| (vec![task], vec![task + 1]))
        .collect();
    let costs: Vec<_> = (0..task_count).map(|task| task + 1).collect();
    PlanIdentity::new(
        1,
        keys,
        dependencies,
        effects,
        costs,
        task_count.saturating_mul(task_count + 1) / 2 + 1,
        "schedlib-v1",
    )
    .expect("the canonical generated identity is valid")
}

fn successes(task_count: u64) -> Vec<DurableOutcome<u64, u64, u64>> {
    (0..task_count).map(DurableOutcome::Success).collect()
}

fn uninterrupted(task_count: u64) -> schedlib::durable::ProtocolReport<u64, u64, u64, u64> {
    let identity = identity(task_count);
    let input = ProtocolInput::new(identity, successes(task_count));
    ResumeMachine::run(input).expect("generated serial protocol is valid")
}

#[test]
fn prop_durable_protocol_state_is_typed() {
    for task_count in 0..=8 {
        let report = uninterrupted(task_count);
        assert!(report.snapshots().iter().all(|state| state.is_well_typed()));
    }
}

#[test]
fn prop_external_keys_have_no_dense_alias() {
    for task_count in 0..=64 {
        let map = ExternalKeyMap::new((0..task_count).map(|key| 10_000 + key))
            .expect("generated keys are unique");
        let dense: BTreeSet<_> = map.iter().map(|(_, dense)| dense).collect();
        assert_eq!(dense.len(), task_count as usize);
    }
}

#[test]
fn prop_external_key_dense_round_trip() {
    for task_count in 0..=64 {
        let map = ExternalKeyMap::new((0..task_count).map(|key| 10_000 + key))
            .expect("generated keys are unique");
        for (key, dense) in map.iter() {
            assert_eq!(map.key(dense), Some(key));
            assert_eq!(map.dense(key), Some(dense));
        }
    }
}

#[test]
fn prop_plan_identity_binds_every_semantic_field() {
    let base = identity(3);
    let variants = base.single_field_variants();
    assert_eq!(
        variants.len(),
        PlanIdentity::<u64, &'static str>::FIELD_COUNT
    );
    assert!(variants.iter().all(|variant| variant != &base));
}

#[test]
fn prop_durable_inputs_are_immutable() {
    let expected = identity(8);
    let report = uninterrupted(8);
    assert!(report
        .snapshots()
        .iter()
        .all(|snapshot| snapshot.plan_identity() == &expected));
}

#[test]
fn prop_journal_ordinals_are_exact() {
    for task_count in 0..=64 {
        let report = uninterrupted(task_count);
        for (index, event) in report.journal().events().iter().enumerate() {
            assert_eq!(event.ordinal().get(), index as u64 + 1);
        }
    }
}

#[test]
fn prop_journal_event_ids_are_unique() {
    let report = uninterrupted(64);
    let ids: BTreeSet<_> = report.journal().events().iter().map(Event::id).collect();
    assert_eq!(ids.len(), report.journal().events().len());
}

#[test]
fn prop_journal_length_is_bounded() {
    for task_count in 0..=64 {
        let report = uninterrupted(task_count);
        assert!(report.journal().events().len() <= task_count as usize + 1);
        assert!(report.journal().is_terminal());
        assert!(report
            .journal()
            .clone()
            .append_terminal(DurableOutcome::Completed)
            .is_err());
    }
}

#[test]
fn prop_terminal_reason_matches_last_event() {
    for outcome in [
        DurableOutcome::Failure(7),
        DurableOutcome::Incomplete(11),
        DurableOutcome::Cancelled,
        DurableOutcome::ResourceLimited,
        DurableOutcome::Completed,
    ] {
        let report = ResumeMachine::run(ProtocolInput::single_terminal(identity(1), outcome))
            .expect("terminal control is valid");
        assert_eq!(
            report.phase(),
            report.journal().last().unwrap().terminal_phase()
        );
    }
}

#[test]
fn prop_journal_only_grows() {
    let report = ResumeMachine::run(
        ProtocolInput::new(identity(8), successes(8))
            .with_crashes(CrashPoint::all_single_crashes()),
    )
    .expect("safe crash matrix completes");
    assert!(report
        .snapshots()
        .windows(2)
        .all(|pair| pair[1].journal().starts_with(pair[0].journal())));
}

#[test]
fn prop_checkpoint_acceptance_binds_plan() {
    let active = identity(3);
    let foreign = active.with_semantic_profile("different");
    let checkpoint = Checkpoint::empty(foreign);
    assert!(checkpoint.validate_for(&active).is_err());
}

#[test]
fn prop_stale_checkpoint_is_rejected() {
    let active = identity(3);
    let checkpoint = Checkpoint::empty(active.with_budget(active.budget() + 1));
    let error = ResumeMachine::run(ProtocolInput::resume(active, successes(3), checkpoint))
        .expect_err("stale identity must fail closed");
    assert!(error.observation().is_empty());
}

#[test]
fn prop_malformed_checkpoint_is_rejected() {
    let active = identity(3);
    for checkpoint in Checkpoint::malformed_variants(&Checkpoint::empty(active.clone())) {
        assert!(checkpoint.validate_for(&active).is_err());
    }
}

#[test]
fn prop_checkpoint_cursor_matches_prefix() {
    let complete = uninterrupted(8);
    for prefix in 0..=8 {
        let checkpoint = Checkpoint::from_committed_prefix(&complete, prefix)
            .expect("every success prefix is a checkpoint");
        assert_eq!(checkpoint.next_task_cursor(), prefix);
    }
}

#[test]
fn prop_rejection_has_no_observable_effects() {
    let active = identity(3);
    let input = ProtocolInput::resume(active.clone(), successes(3), Checkpoint::corrupt(active));
    let error = ResumeMachine::run(input).expect_err("corrupt checkpoint is rejected");
    assert!(error.observation().is_empty());
    assert_eq!(error.started_tasks(), 0);
}

#[test]
fn prop_publication_requires_durable_event() {
    let plan = identity(1);
    let journal = DurableJournal::new(plan.clone());
    let event = Event::task(&plan, 1, 10_000, DurableOutcome::Success(0));
    let mut receipts = PublicationLedger::new();
    assert!(receipts.publish(&journal, &event).is_err());
}

#[test]
fn prop_duplicate_publish_is_idempotent() {
    let report = uninterrupted(1);
    let event = &report.journal().events()[0];
    let mut receipts = PublicationLedger::new();
    assert!(receipts.publish(report.journal(), event).unwrap());
    assert!(!receipts.publish(report.journal(), event).unwrap());
    assert_eq!(receipts.len(), 1);
}

#[test]
fn prop_receipts_form_canonical_prefix() {
    let report = uninterrupted(16);
    assert_eq!(
        report.receipts().event_ids(),
        report
            .journal()
            .events()
            .iter()
            .map(Event::id)
            .collect::<Vec<_>>()
    );
}

#[test]
fn prop_receipts_reference_only_durable_events() {
    let report = uninterrupted(16);
    let durable: BTreeSet<_> = report.journal().events().iter().map(Event::id).collect();
    assert!(report
        .receipts()
        .event_ids()
        .iter()
        .all(|identifier| durable.contains(identifier)));
}

#[test]
fn prop_every_durable_event_is_eventually_published() {
    let baseline = uninterrupted(8);
    for published in 0..baseline.journal().events().len() {
        let checkpoint = Checkpoint::with_published_prefix(&baseline, published).unwrap();
        let resumed =
            ResumeMachine::run(ProtocolInput::resume(identity(8), successes(8), checkpoint))
                .unwrap();
        assert_eq!(resumed.receipts().len(), resumed.journal().events().len());
    }
}

#[test]
fn prop_receipts_only_grow() {
    let report = ResumeMachine::run(
        ProtocolInput::new(identity(8), successes(8))
            .with_crashes(CrashPoint::all_single_crashes()),
    )
    .unwrap();
    assert!(report
        .snapshots()
        .windows(2)
        .all(|pair| pair[0].receipts().is_subset_of(pair[1].receipts())));
}

#[test]
fn prop_crash_preserves_durable_state() {
    let baseline = uninterrupted(8);
    for point in CrashPoint::all_single_crashes() {
        let crashed = baseline.snapshot_before(point).unwrap().crash();
        assert_eq!(
            crashed.journal(),
            baseline.snapshot_before(point).unwrap().journal()
        );
        assert_eq!(
            crashed.receipts(),
            baseline.snapshot_before(point).unwrap().receipts()
        );
    }
}

#[test]
fn prop_unsafe_prejournal_replay_is_rejected() {
    let input = ProtocolInput::new(identity(1), successes(1))
        .with_replay_class(10_000, ReplayClass::Unsafe)
        .with_crashes([CrashPoint::BeforeJournal { ordinal: 1 }]);
    let report = ResumeMachine::run(input).expect_err("unsafe replay fails closed");
    assert!(report.journal().events().is_empty());
}

#[test]
fn prop_replay_safe_task_is_admissible() {
    for replay_class in [
        ReplayClass::Deterministic,
        ReplayClass::Idempotent,
        ReplayClass::Transactional,
    ] {
        let input = ProtocolInput::new(identity(1), successes(1))
            .with_replay_class(10_000, replay_class)
            .with_crashes([CrashPoint::BeforeJournal { ordinal: 1 }]);
        assert_eq!(
            ResumeMachine::run(input).unwrap().phase(),
            TerminalPhase::Completed
        );
    }
}

#[test]
fn prop_resumed_journal_equals_uninterrupted() {
    let baseline = uninterrupted(16);
    for prefix in 0..=16 {
        let checkpoint = Checkpoint::from_committed_prefix(&baseline, prefix).unwrap();
        let resumed = ResumeMachine::run(ProtocolInput::resume(
            identity(16),
            successes(16),
            checkpoint,
        ))
        .unwrap();
        assert_eq!(resumed.journal(), baseline.journal());
    }
}

#[test]
fn prop_resumed_observation_equals_serial() {
    let baseline = uninterrupted(16);
    for prefix in 0..=16 {
        let checkpoint = Checkpoint::from_committed_prefix(&baseline, prefix).unwrap();
        let resumed = ResumeMachine::run(ProtocolInput::resume(
            identity(16),
            successes(16),
            checkpoint,
        ))
        .unwrap();
        assert_eq!(resumed.observation(), baseline.observation());
    }
}

#[test]
fn prop_parallel_resume_equals_serial() {
    let serial = uninterrupted(8);
    for order in ProtocolInput::completion_permutations(8) {
        let parallel = ResumeMachine::run(
            ProtocolInput::new(identity(8), successes(8)).with_completion_order(order),
        )
        .unwrap();
        assert_eq!(parallel.observation(), serial.observation());
    }
}

#[test]
fn prop_terminal_outcomes_do_not_promote() {
    let cases = [
        (DurableOutcome::Failure(1), TerminalPhase::Failed),
        (DurableOutcome::Incomplete(2), TerminalPhase::Incomplete),
        (DurableOutcome::Cancelled, TerminalPhase::Cancelled),
        (
            DurableOutcome::ResourceLimited,
            TerminalPhase::ResourceLimited,
        ),
        (DurableOutcome::Completed, TerminalPhase::Completed),
    ];
    for (outcome, phase) in cases {
        let report =
            ResumeMachine::run(ProtocolInput::single_terminal(identity(1), outcome)).unwrap();
        assert_eq!(report.phase(), phase);
    }
}

#[test]
fn prop_completed_iff_all_tasks_succeeded() {
    for failing_task in 0..=8 {
        let mut outcomes = successes(8);
        if failing_task < 8 {
            outcomes[failing_task] = DurableOutcome::Failure(7);
        }
        let report = ResumeMachine::run(ProtocolInput::new(identity(8), outcomes)).unwrap();
        assert_eq!(
            report.phase() == TerminalPhase::Completed,
            failing_task == 8
        );
    }
}

#[test]
fn prop_codec_never_exceeds_limits() {
    for event_limit in 0..=8 {
        for byte_limit in 0..=64 {
            let limits = CodecLimits::new(event_limit, byte_limit);
            let mut usage = CodecUsage::default();
            while let Ok(next) = limits.checked_append(usage, 7) {
                usage = next;
                assert!(usage.events() <= event_limit);
                assert!(usage.bytes() <= byte_limit);
            }
            assert_eq!(limits.rejected_allocation_count(), 0);
        }
    }
}

#[test]
fn prop_durable_machine_work_is_linear() {
    for task_count in 0..=4096 {
        let report = uninterrupted(task_count);
        assert!(report.work().steps() <= 32 * (report.work().input_cells() + 1));
    }
}

#[test]
fn prop_durable_machine_heap_is_linear() {
    for task_count in 0..=4096 {
        let report = uninterrupted(task_count);
        assert!(report.work().heap_cells() <= 24 * (report.work().input_cells() + 1));
    }
}

#[test]
fn small_stack_durable_resume_lifecycle() {
    const TASKS: u64 = 100_000;
    thread::Builder::new()
        .stack_size(64 * 1024)
        .spawn(|| {
            let report = uninterrupted(TASKS);
            assert_eq!(report.phase(), TerminalPhase::Completed);
            assert_eq!(report.work().maximum_native_frames(), 1);
            drop(report);
        })
        .expect("small-stack lifecycle thread starts")
        .join()
        .expect("durable lifecycle remains stack-safe");
}

#[test]
fn prop_replay_cursor_strictly_decreases() {
    let report = uninterrupted(4096);
    assert!(report.snapshots().windows(2).all(|pair| {
        pair[1].remaining_replay_events() < pair[0].remaining_replay_events()
            || pair[1].remaining_replay_events() == 0
    }));
}

#[test]
fn prop_durable_protocol_eventually_terminates() {
    for task_count in 0..=32 {
        let report = ResumeMachine::run(
            ProtocolInput::new(identity(task_count), successes(task_count))
                .with_crashes(CrashPoint::all_bounded_subsets(task_count + 1, 2)),
        )
        .expect("every bounded safe crash trace terminates");
        assert!(report.phase().is_terminal());
        assert!(report.work().steps() <= report.work().proven_step_bound());
    }
}
