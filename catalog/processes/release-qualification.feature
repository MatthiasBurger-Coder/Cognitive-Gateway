@process(release-qualification)
@process-version(1)
@cg-language(1)
Feature: Generic release qualification
  Rule: Process
    Given state QUALIFY is initial
    Given state REWORK
    Given state QUALIFIED is terminal
    Given event release.approved
    Given event release.rejected
    Given event rework.completed
    Given gate release-approval
    Given evidence release.report
    Given activity qualify-release requires capability quality.test-strategy-analysis
    Given activity qualify-release produces evidence release.report
    Given activity rework-release requires capability quality.test-strategy-analysis
    Given blocker release-rejected reason release qualification failed resolvable

    Scenario: qualify a release
      Given process state QUALIFY
      Given gate release-approval is passed
      Given evidence release.report is present
      Given authorization release-owner is allowed
      Given policy decision release-policy is allow
      When event release.approved occurs
      Then require gate release-approval
      Then require evidence release.report
      Then transition to state QUALIFIED
      Then complete process

    Scenario: repair a rejected release
      Given process state QUALIFY
      When event release.rejected occurs
      Then transition to state REWORK
      Then block process with release-rejected
      Then authorize activity rework-release

    Scenario: return from release rework
      Given process state REWORK
      When event rework.completed occurs
      Then transition to state QUALIFY
      Then authorize activity qualify-release
