# Interpretation Context 1.0 fixtures

These complete snapshot inputs exercise the real application resolver with a
trusted fixture host, an explicit evaluation time of 100 and no model/network.

| Fixture | Query | Expected result |
| --- | --- | --- |
| `alias.json` | Exact service alias `payments` | AMBIGUOUS: A/B; focus A cannot break the tie |
| `pronoun.json` | Typed `target` focus query | RESOLVED: A from conversation focus |
| `temporal.json` | Yesterday at named anchor `now` | RESOLVED: recorded Gregorian UTC civil interval |
| `scope-conflict.json` | Any query | Scope admission rejected before candidate disclosure |

Run `cargo test -p gateway-application --test interpretation --locked`.
The broader test corpus constructs stale, conflicting, corrected, malformed,
wrong-kind, wrong-session, bounded-lookup and changed-basis cases. Fixtures can
be regenerated intentionally by setting `CG_UPDATE_INTERPRETATION_FIXTURES=1`
for the `frozen_alias_pronoun_temporal_and_scope_fixtures` test; review their diff.
