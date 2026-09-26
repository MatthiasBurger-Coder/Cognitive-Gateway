@process(inspect)
@process-version(1)
@cg-language(1)
Feature: Inspect external project
Rule: Process
Given state START is initial
Given state END is terminal
Given event finish
Given activity inspect requires capability architecture.dependency-analysis
Scenario: finish
Given process state START
When event finish occurs
Then transition to state END
Then authorize activity inspect
Then complete process
