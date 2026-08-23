@process(example)
@process-version(1)
@cg-language(1)
Feature: Unknown semantic step
  Rule: Process
    Given state START is initial
    Given state DONE is terminal
    Given event finish
    Scenario: invalid
      Given the agent feels ready
      When event finish occurs
      Then transition to state DONE
