(** * DurableResume — durable identities and committed-prefix recovery

    This constructive theory is the preimplementation semantic contract for
    schedlib persistence.  Recursive definitions in this file are finite
    denotations used by the prover.  The production refinement is required to
    use heap-resident cursors, journals, and worklists.
*)

From Stdlib Require Import Arith Bool Lia List PeanoNat.
From Stdlib Require Import Sorting.Permutation.

Import ListNotations.

Section DurableResume.

Context {Key Effect Semantic : Type}.
Variable key_eq_dec : forall left right : Key, {left = right} + {left <> right}.

(** ** Stable external keys and dense internal identifiers *)

Record key_map : Type := {
  dense_keys : list Key;
  key_to_dense : Key -> option nat;
  dense_to_key : nat -> option Key
}.

Record key_map_valid (mapping : key_map) : Prop := {
  key_map_keys_unique : NoDup (dense_keys mapping);
  key_map_forward_complete : forall key,
    In key (dense_keys mapping) -> exists dense,
      key_to_dense mapping key = Some dense;
  key_map_forward_bounded : forall key dense,
    key_to_dense mapping key = Some dense -> dense < length (dense_keys mapping);
  key_map_backward_is_canonical : forall dense,
    dense_to_key mapping dense = nth_error (dense_keys mapping) dense;
  key_map_forward_round_trip : forall key dense,
    key_to_dense mapping key = Some dense ->
    dense_to_key mapping dense = Some key;
  key_map_backward_round_trip : forall dense key,
    dense_to_key mapping dense = Some key ->
    key_to_dense mapping key = Some dense
}.

Theorem valid_key_map_has_no_dense_alias : forall mapping key_left key_right dense,
  key_map_valid mapping ->
  key_to_dense mapping key_left = Some dense ->
  key_to_dense mapping key_right = Some dense ->
  key_left = key_right.
Proof.
  intros mapping key_left key_right dense valid left_lookup right_lookup.
  pose proof (key_map_forward_round_trip mapping valid key_left dense left_lookup)
    as left_round_trip.
  pose proof (key_map_forward_round_trip mapping valid key_right dense right_lookup)
    as right_round_trip.
  rewrite left_round_trip in right_round_trip.
  inversion right_round_trip.
  reflexivity.
Qed.

Theorem valid_key_map_has_no_dense_orphan : forall mapping dense,
  key_map_valid mapping ->
  dense < length (dense_keys mapping) ->
  exists key, dense_to_key mapping dense = Some key.
Proof.
  intros mapping dense valid bounded.
  rewrite (key_map_backward_is_canonical mapping valid dense).
  apply nth_error_Some in bounded.
  destruct (nth_error (dense_keys mapping) dense) as [key |] eqn:lookup.
  - now exists key.
  - contradiction.
Qed.

Theorem valid_key_map_dense_round_trip : forall mapping dense key,
  key_map_valid mapping ->
  dense_to_key mapping dense = Some key ->
  key_to_dense mapping key = Some dense.
Proof.
  intros mapping dense key valid lookup.
  exact (key_map_backward_round_trip mapping valid dense key lookup).
Qed.

(** ** Structural plan identity

    The proof identity is the complete canonical semantic manifest.  A later
    schedlib-interop refinement may encode and digest this value, but only an
    injective, domain-separated encoding may stand in for structural equality.
*)

Record plan_identity : Type := {
  identity_schema : nat;
  identity_keys : list Key;
  identity_dependencies : list (nat * nat);
  identity_effects : list Effect;
  identity_costs : list nat;
  identity_budget : nat;
  identity_semantics : Semantic
}.

Record plan_identity_total (identity : plan_identity) : Prop := {
  identity_key_count_matches_effects :
    length (identity_keys identity) = length (identity_effects identity);
  identity_key_count_matches_costs :
    length (identity_keys identity) = length (identity_costs identity);
  identity_keys_are_unique : NoDup (identity_keys identity);
  identity_dependency_endpoints_bounded : forall source target,
    In (source, target) (identity_dependencies identity) ->
    source < length (identity_keys identity) /\
    target < length (identity_keys identity)
}.

Theorem equal_plan_identity_binds_dependencies : forall left right,
  left = right -> identity_dependencies left = identity_dependencies right.
Proof. intros left right equality; now subst right. Qed.

Theorem equal_plan_identity_binds_effects : forall left right,
  left = right -> identity_effects left = identity_effects right.
Proof. intros left right equality; now subst right. Qed.

Theorem equal_plan_identity_binds_costs_and_budget : forall left right,
  left = right ->
  identity_costs left = identity_costs right /\
  identity_budget left = identity_budget right.
Proof. intros left right equality; subst right; auto. Qed.

Theorem equal_plan_identity_binds_semantics : forall left right,
  left = right -> identity_semantics left = identity_semantics right.
Proof. intros left right equality; now subst right. Qed.

(** ** Canonical journal and terminal outcomes *)

Inductive task_outcome : Type :=
| OutcomeSuccess
| OutcomeFailure
| OutcomeIncomplete.

Inductive terminal_reason : Type :=
| TerminalCancelled
| TerminalResourceLimit
| TerminalCompleted.

Inductive journal_event : Type :=
| TaskEvent (ordinal dense : nat) (outcome : task_outcome)
| TerminalEvent (ordinal : nat) (reason : terminal_reason).

Definition event_ordinal (event : journal_event) : nat :=
  match event with
  | TaskEvent ordinal _ _ => ordinal
  | TerminalEvent ordinal _ => ordinal
  end.

Inductive journal_phase : Type :=
| JournalOpen
| JournalTerminal.

Inductive journal_trace (identity : plan_identity)
  : list journal_event -> journal_phase -> Prop :=
| TraceEmpty : journal_trace identity [] JournalOpen
| TraceSuccess : forall journal dense,
    journal_trace identity journal JournalOpen ->
    nth_error (identity_keys identity) (length journal) = Some dense ->
    journal_trace identity
      (journal ++ [TaskEvent (S (length journal)) (length journal) OutcomeSuccess])
      JournalOpen
| TraceFailure : forall journal dense,
    journal_trace identity journal JournalOpen ->
    nth_error (identity_keys identity) (length journal) = Some dense ->
    journal_trace identity
      (journal ++ [TaskEvent (S (length journal)) (length journal) OutcomeFailure])
      JournalTerminal
| TraceIncomplete : forall journal dense,
    journal_trace identity journal JournalOpen ->
    nth_error (identity_keys identity) (length journal) = Some dense ->
    journal_trace identity
      (journal ++ [TaskEvent (S (length journal)) (length journal) OutcomeIncomplete])
      JournalTerminal
| TraceCancelled : forall journal,
    journal_trace identity journal JournalOpen ->
    journal_trace identity
      (journal ++ [TerminalEvent (S (length journal)) TerminalCancelled])
      JournalTerminal
| TraceResourceLimit : forall journal,
    journal_trace identity journal JournalOpen ->
    journal_trace identity
      (journal ++ [TerminalEvent (S (length journal)) TerminalResourceLimit])
      JournalTerminal
| TraceCompleted : forall journal,
    journal_trace identity journal JournalOpen ->
    length journal = length (identity_keys identity) ->
    journal_trace identity
      (journal ++ [TerminalEvent (S (length journal)) TerminalCompleted])
      JournalTerminal.

Lemma map_event_ordinal_snoc : forall journal event,
  map event_ordinal (journal ++ [event]) =
  map event_ordinal journal ++ [event_ordinal event].
Proof. intros journal event; now rewrite map_app. Qed.

Theorem journal_trace_ordinals_are_exact : forall identity journal phase,
  journal_trace identity journal phase ->
  map event_ordinal journal = seq 1 (length journal).
Proof.
  intros identity journal phase trace.
  induction trace; simpl.
  - reflexivity.
  - rewrite map_event_ordinal_snoc, IHtrace.
    rewrite length_app; simpl.
    replace (length journal + 1) with (S (length journal)) by lia.
    rewrite seq_S.
    f_equal; lia.
  - rewrite map_event_ordinal_snoc, IHtrace.
    rewrite length_app; simpl.
    replace (length journal + 1) with (S (length journal)) by lia.
    rewrite seq_S.
    f_equal; lia.
  - rewrite map_event_ordinal_snoc, IHtrace.
    rewrite length_app; simpl.
    replace (length journal + 1) with (S (length journal)) by lia.
    rewrite seq_S.
    f_equal; lia.
  - rewrite map_event_ordinal_snoc, IHtrace.
    rewrite length_app; simpl.
    replace (length journal + 1) with (S (length journal)) by lia.
    rewrite seq_S.
    f_equal; lia.
  - rewrite map_event_ordinal_snoc, IHtrace.
    rewrite length_app; simpl.
    replace (length journal + 1) with (S (length journal)) by lia.
    rewrite seq_S.
    f_equal; lia.
  - rewrite map_event_ordinal_snoc, IHtrace.
    rewrite length_app; simpl.
    replace (length journal + 1) with (S (length journal)) by lia.
    rewrite seq_S.
    f_equal; lia.
Qed.

Theorem journal_trace_has_unique_event_ids : forall identity journal phase,
  journal_trace identity journal phase ->
  NoDup (map event_ordinal journal).
Proof.
  intros identity journal phase trace.
  rewrite (journal_trace_ordinals_are_exact identity journal phase trace).
  apply seq_NoDup.
Qed.

Theorem open_journal_is_bounded_by_plan : forall identity journal,
  journal_trace identity journal JournalOpen ->
  length journal <= length (identity_keys identity).
Proof.
  intros identity journal trace.
  remember JournalOpen as phase eqn:phase_eq.
  induction trace; inversion phase_eq; subst; simpl.
  - lia.
  - rewrite length_app; simpl.
    assert (length journal < length (identity_keys identity)) as bounded.
    {
      apply nth_error_Some.
      rewrite H.
      discriminate.
    }
    lia.
Qed.

Definition event_is_success (event : journal_event) : Prop :=
  exists ordinal dense,
    event = TaskEvent ordinal dense OutcomeSuccess.

Theorem open_journal_contains_only_successes : forall identity journal,
  journal_trace identity journal JournalOpen ->
  Forall event_is_success journal.
Proof.
  intros identity journal trace.
  remember JournalOpen as phase eqn:phase_eq.
  induction trace; inversion phase_eq; subst.
  - constructor.
  - apply Forall_app.
    split.
    + exact (IHtrace eq_refl).
    + constructor.
      * exists (S (length journal)), (length journal); reflexivity.
      * constructor.
Qed.

Theorem completed_journal_is_total : forall identity journal,
  journal_trace identity journal JournalOpen ->
  length journal = length (identity_keys identity) ->
  journal_trace identity
    (journal ++ [TerminalEvent (S (length journal)) TerminalCompleted])
    JournalTerminal /\
  Forall event_is_success journal /\
  length (journal ++ [TerminalEvent (S (length journal)) TerminalCompleted]) =
    S (length (identity_keys identity)).
Proof.
  intros identity journal trace complete.
  split.
  - now apply TraceCompleted.
  - split.
    + now apply open_journal_contains_only_successes with (identity := identity).
    + rewrite length_app; simpl; lia.
Qed.

Inductive event_is_terminal : journal_event -> Prop :=
| FailureEventIsTerminal : forall ordinal dense,
    event_is_terminal (TaskEvent ordinal dense OutcomeFailure)
| IncompleteEventIsTerminal : forall ordinal dense,
    event_is_terminal (TaskEvent ordinal dense OutcomeIncomplete)
| CancellationEventIsTerminal : forall ordinal,
    event_is_terminal (TerminalEvent ordinal TerminalCancelled)
| ResourceEventIsTerminal : forall ordinal,
    event_is_terminal (TerminalEvent ordinal TerminalResourceLimit)
| CompletionEventIsTerminal : forall ordinal,
    event_is_terminal (TerminalEvent ordinal TerminalCompleted).

Theorem terminal_journal_ends_with_terminal_event : forall identity journal,
  journal_trace identity journal JournalTerminal ->
  exists prefix event,
    journal = prefix ++ [event] /\ event_is_terminal event.
Proof.
  intros identity journal terminal.
  inversion terminal; subst;
    eauto 8 using FailureEventIsTerminal, IncompleteEventIsTerminal,
      CancellationEventIsTerminal, ResourceEventIsTerminal,
      CompletionEventIsTerminal.
Qed.

(** ** Checkpoint validation *)

Record checkpoint : Type := {
  checkpoint_plan : plan_identity;
  checkpoint_journal : list journal_event;
  checkpoint_cursor : nat;
  checkpoint_integrity_valid : bool;
  checkpoint_encoded_events : nat;
  checkpoint_encoded_bytes : nat
}.

Fixpoint successful_task_prefix_length (journal : list journal_event) : nat :=
  match journal with
  | TaskEvent _ _ OutcomeSuccess :: tail =>
      S (successful_task_prefix_length tail)
  | _ => 0
  end.

Definition checkpoint_accepts
  (active : plan_identity) (candidate : checkpoint) : Prop :=
  checkpoint_plan candidate = active /\
  checkpoint_integrity_valid candidate = true /\
  checkpoint_cursor candidate =
    successful_task_prefix_length (checkpoint_journal candidate) /\
  checkpoint_encoded_events candidate = length (checkpoint_journal candidate) /\
  exists phase, journal_trace active (checkpoint_journal candidate) phase.

Theorem accepted_checkpoint_binds_plan : forall active candidate,
  checkpoint_accepts active candidate -> checkpoint_plan candidate = active.
Proof. intros active candidate [bound _]; exact bound. Qed.

Theorem stale_checkpoint_is_rejected : forall active candidate,
  checkpoint_plan candidate <> active -> ~ checkpoint_accepts active candidate.
Proof. intros active candidate stale [bound _]; contradiction. Qed.

Theorem corrupt_checkpoint_is_rejected : forall active candidate,
  checkpoint_integrity_valid candidate = false ->
  ~ checkpoint_accepts active candidate.
Proof.
  intros active candidate corrupt [_ [integrity _]].
  rewrite corrupt in integrity.
  discriminate.
Qed.

Theorem malformed_checkpoint_cursor_is_rejected : forall active candidate,
  checkpoint_cursor candidate <>
    successful_task_prefix_length (checkpoint_journal candidate) ->
  ~ checkpoint_accepts active candidate.
Proof. intros active candidate malformed [_ [_ [cursor _]]]; contradiction. Qed.

Theorem accepted_checkpoint_cursor_is_success_prefix : forall active candidate,
  checkpoint_accepts active candidate ->
  checkpoint_cursor candidate =
    successful_task_prefix_length (checkpoint_journal candidate).
Proof. intros active candidate [_ [_ [cursor _]]]; exact cursor. Qed.

Theorem malformed_checkpoint_event_count_is_rejected : forall active candidate,
  checkpoint_encoded_events candidate <> length (checkpoint_journal candidate) ->
  ~ checkpoint_accepts active candidate.
Proof.
  intros active candidate malformed [_ [_ [_ [event_count _]]]].
  contradiction.
Qed.

Theorem accepted_checkpoint_has_exact_event_ids : forall active candidate,
  checkpoint_accepts active candidate ->
  map event_ordinal (checkpoint_journal candidate) =
  seq 1 (length (checkpoint_journal candidate)).
Proof.
  intros active candidate [_ [_ [_ [_ [phase trace]]]]].
  exact (journal_trace_ordinals_are_exact active _ phase trace).
Qed.

(** ** Journal-first, idempotency-keyed publication *)

Variable plan_identity_eq_dec : forall left right : plan_identity,
  {left = right} + {left <> right}.

Record event_identifier : Type := {
  identifier_plan : plan_identity;
  identifier_ordinal : nat
}.

Definition event_identifier_eq_dec : forall left right : event_identifier,
  {left = right} + {left <> right}.
Proof.
  intros [left_plan left_ordinal] [right_plan right_ordinal].
  destruct (plan_identity_eq_dec left_plan right_plan) as [same_plan | other_plan].
  - subst right_plan.
    destruct (Nat.eq_dec left_ordinal right_ordinal)
      as [same_ordinal | other_ordinal].
    + subst right_ordinal; now left.
    + right; intros equality; inversion equality; contradiction.
  - right; intros equality; inversion equality; contradiction.
Defined.

Definition event_id (identity : plan_identity) (event : journal_event)
  : event_identifier :=
  {| identifier_plan := identity;
     identifier_ordinal := event_ordinal event |}.

Fixpoint insert_once
  (identifier : event_identifier)
  (published : list event_identifier) : list event_identifier :=
  match published with
  | [] => [identifier]
  | head :: tail =>
      if event_identifier_eq_dec identifier head
      then head :: tail
      else head :: insert_once identifier tail
  end.

Lemma insert_once_contains_identifier : forall identifier published,
  In identifier (insert_once identifier published).
Proof.
  intros identifier published.
  induction published as [|head tail IH]; simpl.
  - auto.
  - destruct (event_identifier_eq_dec identifier head) as [equal | different].
    + subst head; exact (or_introl eq_refl).
    + right; exact IH.
Qed.

Lemma insert_once_preserves_membership : forall identifier member published,
  In member published -> In member (insert_once identifier published).
Proof.
  intros identifier member published membership.
  induction published as [|head tail IH]; simpl in *.
  - contradiction.
  - destruct (event_identifier_eq_dec identifier head); simpl.
    + exact membership.
    + destruct membership as [same | in_tail].
      * now left.
      * right; now apply IH.
Qed.

Lemma insert_once_adds_only_identifier : forall identifier member published,
  In member (insert_once identifier published) ->
  member = identifier \/ In member published.
Proof.
  intros identifier member published.
  induction published as [|head tail IH]; simpl.
  - intros [same | impossible].
    + now left.
    + contradiction.
  - destruct (event_identifier_eq_dec identifier head) as [equal | different].
    + intros membership; right; exact membership.
    + intros [same | in_tail].
      * right; now left.
      * destruct (IH in_tail) as [is_identifier | was_present].
        -- now left.
        -- right; now right.
Qed.

Lemma insert_once_preserves_no_duplicates : forall identifier published,
  NoDup published -> NoDup (insert_once identifier published).
Proof.
  intros identifier published unique.
  induction unique as [|head tail head_absent tail_unique IH]; simpl.
  - constructor.
    + simpl; tauto.
    + constructor.
  - destruct (event_identifier_eq_dec identifier head) as [equal | different].
    + exact (NoDup_cons head head_absent tail_unique).
    + constructor.
      * intro head_inserted.
        destruct (insert_once_adds_only_identifier identifier head tail head_inserted)
          as [head_is_identifier | head_in_tail].
        -- exfalso; apply different; symmetry; exact head_is_identifier.
        -- exact (head_absent head_in_tail).
      * exact IH.
Qed.

Fixpoint publish_events
  (identity : plan_identity)
  (journal : list journal_event)
  (published : list event_identifier) : list event_identifier :=
  match journal with
  | [] => published
  | event :: tail =>
      publish_events identity tail (insert_once (event_id identity event) published)
  end.

Lemma publish_events_preserves_no_duplicates : forall identity journal published,
  NoDup published -> NoDup (publish_events identity journal published).
Proof.
  intros identity journal.
  induction journal as [|event tail IH]; intros published unique; simpl.
  - exact unique.
  - apply IH.
    now apply insert_once_preserves_no_duplicates.
Qed.

Lemma publish_events_preserves_prior_receipts : forall identity journal published identifier,
  In identifier published ->
  In identifier (publish_events identity journal published).
Proof.
  intros identity journal.
  induction journal as [|event tail IH]; intros published identifier prior; simpl.
  - exact prior.
  - apply IH.
    now apply insert_once_preserves_membership.
Qed.

Lemma publish_events_contains_each_journal_event :
  forall identity journal published event,
  In event journal ->
  In (event_id identity event) (publish_events identity journal published).
Proof.
  intros identity journal.
  induction journal as [|head tail IH]; intros published event membership; simpl in *.
  - contradiction.
  - destruct membership as [same | in_tail].
    + subst head.
      apply publish_events_preserves_prior_receipts.
      apply insert_once_contains_identifier.
    + apply IH with (published := insert_once (event_id identity head) published).
      exact in_tail.
Qed.

Lemma publish_events_adds_only_journal_ids :
  forall identity journal published identifier,
  In identifier (publish_events identity journal published) ->
  In identifier published \/ In identifier (map (event_id identity) journal).
Proof.
  intros identity journal.
  induction journal as [|event tail IH]; intros published identifier membership; simpl in *.
  - now left.
  - destruct (IH (insert_once (event_id identity event) published) identifier membership)
      as [in_inserted | in_tail].
    + destruct (insert_once_adds_only_identifier _ _ _ in_inserted)
        as [is_event | was_published].
      * right; left; symmetry; exact is_event.
      * now left.
    + right; now right.
Qed.

Definition recover_publications (candidate : checkpoint)
  : list event_identifier :=
  publish_events
    (checkpoint_plan candidate) (checkpoint_journal candidate) [].

Theorem recovery_publication_is_exactly_once : forall candidate,
  NoDup (recover_publications candidate).
Proof.
  intros candidate.
  apply publish_events_preserves_no_duplicates.
  constructor.
Qed.

Theorem recovery_publishes_every_durable_event : forall candidate event,
  In event (checkpoint_journal candidate) ->
  In (event_id (checkpoint_plan candidate) event)
    (recover_publications candidate).
Proof.
  intros candidate event membership.
  apply publish_events_contains_each_journal_event.
  exact membership.
Qed.

Theorem recovery_never_invents_an_event : forall candidate identifier,
  In identifier (recover_publications candidate) ->
  In identifier
    (map (event_id (checkpoint_plan candidate))
      (checkpoint_journal candidate)).
Proof.
  intros candidate identifier membership.
  unfold recover_publications in membership.
  destruct (publish_events_adds_only_journal_ids _ _ [] _ membership)
    as [impossible | durable].
  - contradiction.
  - exact durable.
Qed.

Record volatile_state : Type := {
  volatile_cursor : nat;
  volatile_staged_event : option journal_event
}.

Record recovery_state : Type := {
  recovery_checkpoint : checkpoint;
  recovery_receipts : list event_identifier;
  recovery_volatile : volatile_state
}.

Definition crash (state : recovery_state) : recovery_state :=
  {| recovery_checkpoint := recovery_checkpoint state;
     recovery_receipts := recovery_receipts state;
     recovery_volatile := {| volatile_cursor := 0;
       volatile_staged_event := None |} |}.

Theorem crash_preserves_durable_journal : forall state,
  checkpoint_journal (recovery_checkpoint (crash state)) =
  checkpoint_journal (recovery_checkpoint state).
Proof. intros state; reflexivity. Qed.

Theorem crash_preserves_sink_receipts : forall state,
  recovery_receipts (crash state) = recovery_receipts state.
Proof. intros state; reflexivity. Qed.

(** ** Replay safety and resume equivalence *)

Inductive replay_class : Type :=
| ReplayDeterministic
| ReplayIdempotent
| ReplayTransactional
| ReplayUnsafe.

Definition replay_class_is_safe (class : replay_class) : bool :=
  match class with
  | ReplayUnsafe => false
  | _ => true
  end.

Definition resume_admissible
  (in_flight : option nat) (class : replay_class) : Prop :=
  in_flight = None \/ replay_class_is_safe class = true.

Theorem unsafe_in_flight_task_is_rejected : forall task,
  ~ resume_admissible (Some task) ReplayUnsafe.
Proof.
  intros task [not_in_flight | safe].
  - discriminate.
  - discriminate.
Qed.

Theorem deterministic_in_flight_task_is_admissible : forall task,
  resume_admissible (Some task) ReplayDeterministic.
Proof. intros task; right; reflexivity. Qed.

Definition journal_prefix (prefix complete : list journal_event) : Prop :=
  exists suffix, complete = prefix ++ suffix.

Definition resume_journal
  (prefix suffix : list journal_event) : list journal_event := prefix ++ suffix.

Theorem resume_reconstructs_uninterrupted_journal : forall prefix complete,
  journal_prefix prefix complete ->
  exists suffix, resume_journal prefix suffix = complete.
Proof.
  intros prefix complete [suffix equality].
  exists suffix.
  unfold resume_journal.
  symmetry; exact equality.
Qed.

Theorem resumed_observation_equals_serial : forall prefix complete,
  journal_prefix prefix complete ->
  exists suffix,
    map event_ordinal (resume_journal prefix suffix) =
    map event_ordinal complete.
Proof.
  intros prefix complete prefix_of.
  destruct (resume_reconstructs_uninterrupted_journal prefix complete prefix_of)
    as [suffix equality].
  exists suffix.
  now rewrite equality.
Qed.

Fixpoint normalize_completions
  (canonical physical : list Key) : list Key :=
  match canonical with
  | [] => []
  | key :: tail =>
      if in_dec key_eq_dec key physical
      then key :: normalize_completions tail physical
      else normalize_completions tail physical
  end.

Lemma normalize_complete_cover : forall canonical physical,
  (forall key, In key canonical -> In key physical) ->
  normalize_completions canonical physical = canonical.
Proof.
  induction canonical as [|key tail IH]; intros physical cover; simpl.
  - reflexivity.
  - destruct (in_dec key_eq_dec key physical) as [present | absent].
    + f_equal.
      apply IH.
      intros member in_tail.
      apply cover.
      now right.
    + exfalso.
      apply absent, cover.
      now left.
Qed.

Theorem physical_completion_order_is_unobservable :
  forall canonical serial_order parallel_order,
  Permutation serial_order canonical ->
  Permutation parallel_order canonical ->
  normalize_completions canonical serial_order =
  normalize_completions canonical parallel_order.
Proof.
  intros canonical serial_order parallel_order serial_perm parallel_perm.
  rewrite (normalize_complete_cover canonical serial_order).
  - symmetry.
    apply normalize_complete_cover.
    intros key in_canonical.
    eapply Permutation_in.
    + exact (Permutation_sym parallel_perm).
    + exact in_canonical.
  - intros key in_canonical.
    eapply Permutation_in.
    + exact (Permutation_sym serial_perm).
    + exact in_canonical.
Qed.

(** ** Bounded codec and explicit-machine resource contracts *)

Record codec_limits : Type := {
  maximum_events : nat;
  maximum_bytes : nat
}.

Record codec_usage : Type := {
  used_events : nat;
  used_bytes : nat
}.

Definition codec_usage_valid (limits : codec_limits) (usage : codec_usage) : Prop :=
  used_events usage <= maximum_events limits /\
  used_bytes usage <= maximum_bytes limits.

Definition codec_append_admissible
  (limits : codec_limits) (usage : codec_usage) (event_bytes : nat) : Prop :=
  S (used_events usage) <= maximum_events limits /\
  used_bytes usage + event_bytes <= maximum_bytes limits.

Theorem admissible_codec_append_preserves_limits : forall limits usage event_bytes,
  codec_append_admissible limits usage event_bytes ->
  codec_usage_valid limits
    {| used_events := S (used_events usage);
       used_bytes := used_bytes usage + event_bytes |}.
Proof. intros limits usage event_bytes admissibility; exact admissibility. Qed.

Record machine_account : Type := {
  account_input_cells : nat;
  account_work_steps : nat;
  account_heap_cells : nat;
  account_native_frames : nat
}.

Definition machine_account_valid (factor : nat) (account : machine_account) : Prop :=
  account_work_steps account <= factor * S (account_input_cells account) /\
  account_heap_cells account <= factor * S (account_input_cells account) /\
  account_native_frames account <= 1.

Theorem valid_machine_has_linear_work : forall factor account,
  machine_account_valid factor account ->
  account_work_steps account <= factor * S (account_input_cells account).
Proof. intros factor account [work _]; exact work. Qed.

Theorem valid_machine_has_linear_heap : forall factor account,
  machine_account_valid factor account ->
  account_heap_cells account <= factor * S (account_input_cells account).
Proof. intros factor account [_ [heap _]]; exact heap. Qed.

Theorem valid_machine_has_constant_native_stack : forall factor account,
  machine_account_valid factor account ->
  account_native_frames account <= 1.
Proof. intros factor account [_ [_ stack]]; exact stack. Qed.

Definition replay_remaining (journal : list journal_event) (cursor : nat) : nat :=
  length journal - cursor.

Theorem replay_cursor_step_strictly_decreases : forall journal cursor,
  cursor < length journal ->
  replay_remaining journal (S cursor) < replay_remaining journal cursor.
Proof.
  intros journal cursor bounded.
  unfold replay_remaining.
  lia.
Qed.

Theorem replay_cursor_is_bounded : forall journal cursor,
  cursor <= length journal ->
  replay_remaining journal cursor <= length journal.
Proof. intros journal cursor bounded; unfold replay_remaining; lia. Qed.

End DurableResume.
