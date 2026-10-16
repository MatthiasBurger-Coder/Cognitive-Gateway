@process(semantic-review)
@process-version(1)
@cg-language(1)
Feature: Review an analysis without granting task semantics lifecycle authority
  Rule: Process
    Given state REVIEW is initial
    Given state COMPLETE is terminal
    Given event review.accepted
    Given gate analysis-review
    Given activity analyze requires capability architecture.dependency-analysis

    Scenario: accept reviewed analysis
      Given process state REVIEW
      Given gate analysis-review is passed
      When event review.accepted occurs
      Then require gate analysis-review
      Then transition to state COMPLETE
      Then complete process
