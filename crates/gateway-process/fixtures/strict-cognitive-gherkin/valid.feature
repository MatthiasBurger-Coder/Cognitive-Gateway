@process(canonical-issue-lifecycle)
@process-version(1)
@cg-language(1)
Feature: Canonical issue lifecycle
  Rule: Process
    Given state ANALYZE is initial
    Given state COMPLETE is terminal
    Given state IMPLEMENT
    Given event implementation.accepted
    Given event implementation.completed
    Given gate THREE_AMIGOS
    Given evidence verification.report
    Given activity implement-change requires capability repository.write

    Scenario: accept implementation
      Given process state ANALYZE
      Given gate THREE_AMIGOS is passed
      When event implementation.accepted occurs
      Then transition to state IMPLEMENT
      Then authorize activity implement-change

    Scenario: complete implementation
      Given process state IMPLEMENT
      When event implementation.completed occurs
      Then transition to state COMPLETE
      Then complete process
