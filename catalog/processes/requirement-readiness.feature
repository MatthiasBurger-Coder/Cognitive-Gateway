@process(requirement-readiness)
@process-version(1)
@cg-language(1)
Feature: Generic requirement readiness
  Rule: Process
    Given state INTAKE is initial
    Given state REVIEW
    Given state APPROVED is terminal
    Given state BLOCKED
    Given event request.submitted
    Given event readiness.approved
    Given event readiness.rejected
    Given event blocker.resolved
    Given gate requirement-review
    Given evidence requirement.record
    Given activity prepare-requirements requires capability documentation.traceability-analysis
    Given activity prepare-requirements produces evidence requirement.record
    Given activity review-requirements requires capability quality.test-strategy-analysis
    Given blocker readiness-rejected reason readiness criteria rejected resolvable

    Scenario: begin readiness review
      Given process state INTAKE
      When event request.submitted occurs
      Then transition to state REVIEW
      Then authorize activity prepare-requirements

    Scenario: approve a ready requirement
      Given process state REVIEW
      Given gate requirement-review is passed
      Given evidence requirement.record is present
      Given authorization requirement-owner is allowed
      Given policy decision requirement-policy is allow
      When event readiness.approved occurs
      Then require gate requirement-review
      Then require evidence requirement.record
      Then transition to state APPROVED
      Then complete process

    Scenario: block a rejected requirement
      Given process state REVIEW
      Given policy decision requirement-policy is deny
      When event readiness.rejected occurs
      Then transition to state BLOCKED
      Then block process with readiness-rejected
      Then authorize activity review-requirements

    Scenario: resume a blocked requirement
      Given process state BLOCKED
      Given authorization requirement-owner is allowed
      When event blocker.resolved occurs
      Then transition to state REVIEW
      Then authorize activity review-requirements
