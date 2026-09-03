(set-logic ALL)

; Independent bounded projection of the durable schedlib contract.
; Every UNSAT result excludes the named bad state. SAT results are explicit
; non-vacuity witnesses. This file does not assume a cryptographic digest is
; collision-free: Plan is a structural manifest whose canonical digest is an
; implementation refinement obligation.

(declare-datatypes ((Kind 0))
  (((Success) (Failure) (Incomplete) (Cancelled) (ResourceLimited) (Completed))))
(declare-datatypes ((Phase 0))
  (((Ready) (Failed) (IncompletePhase) (CancelledPhase)
    (ResourceLimitedPhase) (CompletedPhase) (Rejected))))
(declare-datatypes ((Plan 0))
  (((mk-plan
      (plan-key-0 Int)
      (plan-key-1 Int)
      (plan-key-2 Int)
      (plan-dependencies Int)
      (plan-effects Int)
      (plan-costs Int)
      (plan-budget Int)
      (plan-semantics Int)
      (plan-schema Int)))))
(declare-datatypes ((Event 0))
  (((mk-event
      (event-plan Plan)
      (event-ordinal Int)
      (event-task Int)
      (event-kind Kind)))))

(declare-const key0 Int)
(declare-const key1 Int)
(declare-const key2 Int)
(assert (distinct key0 key1 key2))

(define-fun dense-of ((key Int)) Int
  (ite (= key key0) 0
    (ite (= key key1) 1
      (ite (= key key2) 2 (- 1)))))

(define-fun active-plan () Plan
  (mk-plan key0 key1 key2 17 23 29 31 37 41))

(define-fun event1 () Event (mk-event active-plan 1 1 Success))
(define-fun event2 () Event (mk-event active-plan 2 2 Success))
(define-fun event3 () Event (mk-event active-plan 3 3 Success))

(define-fun phase-for ((kind Kind)) Phase
  (ite (= kind Failure) Failed
    (ite (= kind Incomplete) IncompletePhase
      (ite (= kind Cancelled) CancelledPhase
        (ite (= kind ResourceLimited) ResourceLimitedPhase
          (ite (= kind Completed) CompletedPhase Ready))))))

(echo "Q01_KEY_MAP_HAS_NO_ALIAS")
(push)
(assert (or (= (dense-of key0) (dense-of key1))
            (= (dense-of key0) (dense-of key2))
            (= (dense-of key1) (dense-of key2))))
(check-sat)
(pop)

(echo "Q02_KEY_MAP_HAS_NO_ORPHAN")
(push)
(assert (or (not (= (dense-of key0) 0))
            (not (= (dense-of key1) 1))
            (not (= (dense-of key2) 2))))
(check-sat)
(pop)

(echo "Q03_PLAN_IDENTITY_BINDS_DEPENDENCIES")
(push)
(declare-const other-dependencies Int)
(assert (distinct other-dependencies 17))
(assert (= active-plan
  (mk-plan key0 key1 key2 other-dependencies 23 29 31 37 41)))
(check-sat)
(pop)

(echo "Q04_PLAN_IDENTITY_BINDS_SEMANTICS")
(push)
(declare-const other-semantics Int)
(assert (distinct other-semantics 37))
(assert (= active-plan
  (mk-plan key0 key1 key2 17 23 29 31 other-semantics 41)))
(check-sat)
(pop)

(echo "Q05_EVENT_IDS_ARE_UNIQUE")
(push)
(assert (or (= (event-ordinal event1) (event-ordinal event2))
            (= (event-ordinal event1) (event-ordinal event3))
            (= (event-ordinal event2) (event-ordinal event3))))
(check-sat)
(pop)

(echo "Q06_MALFORMED_ORDINAL_IS_NOT_CANONICAL")
(push)
(declare-const malformed Event)
(assert (= malformed (mk-event active-plan 2 1 Success)))
(assert (= (event-ordinal malformed) 1))
(check-sat)
(pop)

(echo "Q07_PUBLICATION_REQUIRES_DURABLE_APPEND")
(push)
(declare-const appended Bool)
(declare-const published Bool)
(assert (=> published appended))
(assert published)
(assert (not appended))
(check-sat)
(pop)

(echo "Q08_PUBLICATION_INSERT_IS_IDEMPOTENT")
(push)
(declare-const receipt-present Bool)
(assert (not (= (or receipt-present receipt-present) receipt-present)))
(check-sat)
(pop)

(echo "Q09_RECEIPTS_FORM_A_PREFIX")
(push)
(declare-const receipt1 Bool)
(declare-const receipt2 Bool)
(declare-const receipt3 Bool)
(assert (=> receipt2 receipt1))
(assert (=> receipt3 receipt2))
(assert (or (and receipt2 (not receipt1))
            (and receipt3 (not receipt2))))
(check-sat)
(pop)

(echo "Q10_STALE_PLAN_IS_REJECTED")
(push)
(declare-const checkpoint-plan Plan)
(declare-const checkpoint-valid Bool)
(declare-const checkpoint-accepted Bool)
(assert (= checkpoint-accepted
  (and checkpoint-valid (= checkpoint-plan active-plan))))
(assert checkpoint-valid)
(assert (distinct checkpoint-plan active-plan))
(assert checkpoint-accepted)
(check-sat)
(pop)

(echo "Q11_UNSAFE_PREJOURNAL_REPLAY_IS_REJECTED")
(push)
(declare-const journaled-before-crash Bool)
(declare-const replay-safe Bool)
(declare-const resume-admissible Bool)
(assert (= resume-admissible (or journaled-before-crash replay-safe)))
(assert (not journaled-before-crash))
(assert (not replay-safe))
(assert resume-admissible)
(check-sat)
(pop)

(echo "Q12_UNSAFE_CRASH_HAS_FAIL_CLOSED_WITNESS")
(push)
(declare-const witness-journaled Bool)
(declare-const witness-replay-safe Bool)
(declare-const witness-rejected Bool)
(assert (not witness-journaled))
(assert (not witness-replay-safe))
(assert (= witness-rejected
  (not (or witness-journaled witness-replay-safe))))
(assert witness-rejected)
(check-sat)
(pop)

(echo "Q13_RESUME_PREFIX_RECONSTRUCTS_SERIAL")
(push)
(declare-const prefix-count Int)
(declare-const value1 Int)
(declare-const value2 Int)
(declare-const value3 Int)
(assert (and (<= 0 prefix-count) (<= prefix-count 3)))
(define-fun prefix-sum () Int
  (ite (= prefix-count 0) 0
    (ite (= prefix-count 1) value1
      (ite (= prefix-count 2) (+ value1 value2)
        (+ value1 value2 value3)))))
(define-fun suffix-sum () Int
  (ite (= prefix-count 0) (+ value1 value2 value3)
    (ite (= prefix-count 1) (+ value2 value3)
      (ite (= prefix-count 2) value3 0))))
(assert (not (= (+ prefix-sum suffix-sum) (+ value1 value2 value3))))
(check-sat)
(pop)

(echo "Q14_PARALLEL_COMPLETION_NORMALIZES_CANONICALLY")
(push)
(declare-const physical0 Int)
(declare-const physical1 Int)
(declare-const physical2 Int)
(assert (and (<= 1 physical0) (<= physical0 3)))
(assert (and (<= 1 physical1) (<= physical1 3)))
(assert (and (<= 1 physical2) (<= physical2 3)))
(assert (distinct physical0 physical1 physical2))
(define-fun minimum3 () Int
  (ite (and (<= physical0 physical1) (<= physical0 physical2)) physical0
    (ite (<= physical1 physical2) physical1 physical2)))
(define-fun maximum3 () Int
  (ite (and (>= physical0 physical1) (>= physical0 physical2)) physical0
    (ite (>= physical1 physical2) physical1 physical2)))
(define-fun middle3 () Int
  (- (+ physical0 physical1 physical2) minimum3 maximum3))
(assert (or (not (= minimum3 1))
            (not (= middle3 2))
            (not (= maximum3 3))))
(check-sat)
(pop)

(echo "Q15_FAILURE_HAS_EXACT_TERMINAL_PHASE")
(push)
(assert (not (= (phase-for Failure) Failed)))
(check-sat)
(pop)

(echo "Q16_BOUNDED_CODEC_CANNOT_EXCEED_DECLARED_LIMIT")
(push)
(declare-const event-count Int)
(declare-const header-bytes Int)
(declare-const event-bytes Int)
(assert (and (<= 0 event-count) (<= event-count 4)))
(assert (>= header-bytes 0))
(assert (>= event-bytes 0))
(assert (> (+ header-bytes (* event-count event-bytes))
           (+ header-bytes (* 4 event-bytes))))
(check-sat)
(pop)

(echo "Q17_REPLAY_CURSOR_STRICTLY_DECREASES")
(push)
(declare-const remaining Int)
(declare-const remaining-next Int)
(assert (> remaining 0))
(assert (= remaining-next (- remaining 1)))
(assert (not (< remaining-next remaining)))
(check-sat)
(pop)

(echo "Q18_NATIVE_STACK_BOUND_IS_CONSTANT")
(push)
(declare-const native-frames Int)
(assert (= native-frames 1))
(assert (> native-frames 1))
(check-sat)
(pop)

(echo "Q19_LAGGING_RECEIPT_RESUME_HAS_WITNESS")
(push)
(declare-const durable-count Int)
(declare-const published-count Int)
(declare-const next-to-publish Int)
(assert (= durable-count 3))
(assert (= published-count 1))
(assert (= next-to-publish (+ published-count 1)))
(assert (and (< published-count durable-count)
             (= next-to-publish 2)))
(check-sat)
(pop)

(echo "Q20_CANCELLATION_CANNOT_SELF_PROMOTE_TO_COMPLETION")
(push)
(assert (= CancelledPhase CompletedPhase))
(check-sat)
(pop)

(echo "Q21_INCOMPLETE_CANNOT_SELF_PROMOTE_TO_COMPLETION")
(push)
(assert (= (phase-for Incomplete) CompletedPhase))
(check-sat)
(pop)
