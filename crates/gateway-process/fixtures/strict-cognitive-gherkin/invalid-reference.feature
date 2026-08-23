@process(example)
@process-version(1)
@cg-language(1)
Feature: Unknown reference
  Rule: Process
    Given state START is initial
    Given state DONE is terminal
    Given event finish
    Scenario: invalid
      Given process state START
      When event missing occurs
      Then transition to state DONE
