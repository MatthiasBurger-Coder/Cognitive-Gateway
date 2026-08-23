@process(implementation-lifecycle)
@process-version(1)
@cg-language(1)
Feature: Generic implementation lifecycle
  Rule: Process
    Given state ANALYZE is initial
    Given state THREE_AMIGOS
    Given state IMPLEMENT
    Given state VERIFY
    Given state ARCHITECTURE_REVIEW
    Given state E2E
    Given state EVIDENCE
    Given state REPAIR
    Given state COMPLETE is terminal
    Given event requirements.approved
    Given event readiness.passed
    Given event implementation.completed
    Given event verification.passed
    Given event verification.failed
    Given event repair.completed
    Given event architecture.approved
    Given event architecture.failed
    Given event e2e.passed
    Given event e2e.failed
    Given event evidence.accepted
    Given gate THREE_AMIGOS
    Given gate ARCHITECTURE_REVIEW
    Given gate E2E
    Given evidence verification.report
    Given evidence architecture.report
    Given evidence e2e.report
    Given evidence completion.record
    Given activity implement-change requires capability architecture.dependency-analysis
    Given activity verify-change requires capability quality.test-strategy-analysis
    Given activity verify-change produces evidence verification.report
    Given activity review-architecture requires capability architecture.boundary-validation
    Given activity review-architecture produces evidence architecture.report
    Given activity run-e2e requires capability quality.test-strategy-analysis
    Given activity run-e2e produces evidence e2e.report
    Given activity repair-change requires capability quality.test-strategy-analysis
    Given activity record-completion requires capability documentation.traceability-analysis
    Given activity record-completion produces evidence completion.record
    Given retry verification.failed max 2 repair REPAIR
    Given blocker verification-failed reason verification requires repair resolvable
    Given blocker architecture-failed reason architecture requires repair resolvable
    Given blocker e2e-failed reason end-to-end verification requires repair resolvable

    Scenario: approve requirements
      Given process state ANALYZE
      Given authorization requirement-review is allowed
      When event requirements.approved occurs
      Then transition to state THREE_AMIGOS

    Scenario: pass readiness
      Given process state THREE_AMIGOS
      Given gate THREE_AMIGOS is passed
      Given policy decision implementation-policy is allow
      When event readiness.passed occurs
      Then transition to state IMPLEMENT
      Then authorize activity implement-change

    Scenario: start verification
      Given process state IMPLEMENT
      When event implementation.completed occurs
      Then transition to state VERIFY
      Then authorize activity verify-change

    Scenario: pass verification
      Given process state VERIFY
      Given evidence verification.report is present
      When event verification.passed occurs
      Then require evidence verification.report
      Then transition to state ARCHITECTURE_REVIEW
      Then authorize activity review-architecture

    Scenario: repair failed verification
      Given process state VERIFY
      When event verification.failed occurs
      Then transition to state REPAIR
      Then block process with verification-failed
      Then retry activity max 2
      Then authorize activity repair-change

    Scenario: return from verification repair
      Given process state REPAIR
      When event repair.completed occurs
      Then transition to state VERIFY
      Then authorize activity verify-change

    Scenario: pass architecture review
      Given process state ARCHITECTURE_REVIEW
      Given gate ARCHITECTURE_REVIEW is passed
      Given evidence architecture.report is present
      When event architecture.approved occurs
      Then require gate ARCHITECTURE_REVIEW
      Then require evidence architecture.report
      Then transition to state E2E
      Then authorize activity run-e2e

    Scenario: repair failed architecture review
      Given process state ARCHITECTURE_REVIEW
      When event architecture.failed occurs
      Then transition to state REPAIR
      Then block process with architecture-failed
      Then authorize activity repair-change

    Scenario: pass end-to-end verification
      Given process state E2E
      Given gate E2E is passed
      Given evidence e2e.report is present
      When event e2e.passed occurs
      Then require gate E2E
      Then require evidence e2e.report
      Then transition to state EVIDENCE
      Then authorize activity record-completion

    Scenario: repair failed end-to-end verification
      Given process state E2E
      When event e2e.failed occurs
      Then transition to state REPAIR
      Then block process with e2e-failed
      Then authorize activity repair-change

    Scenario: record completion evidence
      Given process state EVIDENCE
      Given evidence completion.record is present
      When event evidence.accepted occurs
      Then require evidence completion.record
      Then transition to state COMPLETE
      Then complete process
