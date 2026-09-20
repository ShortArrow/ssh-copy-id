# Documentation

## Document Roles

| Document | Purpose |
| --- | --- |
| Design: [English](design.md), [Japanese](design.jp.md) | Authoritative details of scope, accepted decisions, rationale, implementation plans, open questions, and implementation or verification status. |
| Differences from upstream: [English](compatibility.md), [Japanese](compatibility.jp.md) | A concise comparison table with stable IDs, upstream behavior, project behavior, and links to detailed design decisions. |
| [Adversarial design review](design-review.md) | Review findings, proposed decisions, and links to subsequently accepted decisions. Proposals are not requirements unless explicitly accepted. |
| [Validation plan and results](validation.md) | Scenario IDs, reproducible checks, observed results, and pending coverage. Fixture success is distinct from product conformance. |

## Writing and Updating Documents

Reproducible environment instructions: [Linux identity experiment](../tests/prototypes/identity-isolation/README.md)
and [Windows dockur fixture](../tests/environments/windows/README.md).

- Write primary documentation in English. Keep existing Japanese translations in sync when changing the corresponding English documents.
- Keep detailed decisions in the design document. The differences list contains summaries and links, not duplicated explanations of rationale, scope, implementation, or open questions.
- When a difference is accepted, update the design document and add or update its comparison row in both language versions of the differences list. Preserve its ID and link to the detailed decision.
- If a decision originated in the review, record its disposition there and link to the design. Keep unaccepted proposals distinct from accepted requirements.
- Describe upstream differences neutrally; do not label them as defects or bug fixes without an explicit decision to do so.
- Distinguish accepted designs from implemented and verified behavior. Update detailed implementation and verification status in the design document.
- Check relative links and section anchors when moving or renaming documents or headings.
