@process(example)
@process-version(1)
@cg-language(1)
Feature: Missing initial state
  Rule: Process
    Given state START
    Given state DONE is terminal
    Given event finish
