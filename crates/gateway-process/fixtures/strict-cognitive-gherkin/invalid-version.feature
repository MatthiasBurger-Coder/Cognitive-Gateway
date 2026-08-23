@process(example)
@process-version(1)
@cg-language(99)
Feature: Unsupported language version
  Rule: Process
    Given state START is initial
    Given state DONE is terminal
    Given event finish
