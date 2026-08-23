@process(example)
@process-version(1)
@cg-language(1)
Feature: Executable content is forbidden
  Rule: Process
    Given state START is initial
    Given state DONE is terminal
    Given event finish
    Scenario: invalid
      Given execute rust { std::process::Command::new("sh") }
