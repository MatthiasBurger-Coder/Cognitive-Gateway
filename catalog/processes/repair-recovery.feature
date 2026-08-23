@process(repair-recovery)
@process-version(1)
@cg-language(1)
Feature: Generic bounded repair and recovery
  Rule: Process
    Given state RECOVERY is initial
    Given state RETRY
    Given state RECOVERED is terminal
    Given state FAILED is terminal
    Given event recovery.started
    Given event recovery.succeeded
    Given event recovery.exhausted
    Given activity repair-work requires capability quality.test-strategy-analysis
    Given blocker recovery-exhausted reason recovery budget exhausted resolvable
    Given retry recovery.started max 2 repair RETRY

    Scenario: enter bounded recovery
      Given process state RECOVERY
      Given authorization recovery-owner is allowed
      When event recovery.started occurs
      Then transition to state RETRY
      Then retry activity max 2
      Then authorize activity repair-work

    Scenario: complete recovery
      Given process state RETRY
      When event recovery.succeeded occurs
      Then transition to state RECOVERED
      Then complete process

    Scenario: stop after recovery exhaustion
      Given process state RETRY
      When event recovery.exhausted occurs
      Then transition to state FAILED
      Then block process with recovery-exhausted
      Then complete process
