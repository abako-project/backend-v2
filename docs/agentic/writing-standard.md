# Writing Standard

Good engineering prose saves time. Use four rules for every document, specification, prompt, comment, and handoff.

## Simplicity

Use the common word when it is accurate. Prefer one idea per sentence. Introduce a term only when the term carries useful precision.

## Brevity

Say each rule once. Remove throat-clearing, repeated conclusions, decorative adjectives, and examples that do not change a decision. Short does not mean incomplete. Keep the facts a reader needs to act safely.

## Clarity

Name the actor, action, condition, and expected result. State assumptions as assumptions. State unknowns as unknowns. Use concrete paths, commands, types, units, and boundaries.

For acceptance behavior, use Gherkin when a scenario is useful:

```gherkin
Given a registered user
When the user submits valid credentials
Then the service returns an authenticated session
And the audit event contains no secret value
```

## Humanity

Write for another engineer, not for a rubric. Be calm, useful, and candid. Explain the reason behind a surprising rule. Do not hide uncertainty behind formal language.

## Code Comments

Comments explain invariants, non-obvious trade-offs, safety conditions, compatibility constraints, or why the obvious implementation is wrong. Do not translate code into English.

## Review Checklist

- Can a sentence be shorter without losing meaning?
- Is any rule repeated?
- Are vague references easy to resolve?
- Are assumptions and unknowns labeled?
- Can the reader tell what to do next?
- Does the text sound like a competent human wrote it?
