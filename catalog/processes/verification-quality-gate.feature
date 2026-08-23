@process(verification-quality-gate)
@process-version(1)
@cg-language(1)
Feature: Generic verification quality gate
  Rule: Process
    Given state VERIFY is initial
    Given state REPAIR
    Given state PASSED is terminal
    Given event verification.passed
    Given event verification.failed
    Given event repair.completed
    Given gate quality-review
    Given evidence test.report
    Given activity run-tests requires capability quality.test-strategy-analysis
    Given activity run-tests produces evidence test.report
    Given activity repair-tests requires capability quality.test-strategy-analysis
    Given blocker verification-failed reason verification requires repair resolvable
    Given retry verification.failed max 2 repair REPAIR

    Scenario: pass the quality gate
      Given process state VERIFY
      Given gate quality-review is passed
      Given evidence test.report is present
      Given policy decision quality-policy is allow
      When event verification.passed occurs
      Then require gate quality-review
      Then require evidence test.report
      Then transition to state PASSED
      Then complete process

    Scenario: repair a failed quality gate
      Given process state VERIFY
      When event verification.failed occurs
      Then transition to state REPAIR
      Then block process with verification-failed
      Then retry activity max 2
      Then authorize activity repair-tests

    Scenario: retry verification after repair
      Given process state REPAIR
      When event repair.completed occurs
      Then transition to state VERIFY
      Then authorize activity run-tests
