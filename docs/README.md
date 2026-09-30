# Documentation

## Document Roles

| Document | Purpose |
| --- | --- |
| Design: [English](design.md), [Japanese](design.jp.md) | Authoritative details of scope, accepted decisions, rationale, implementation plans, open questions, and a decision log. One line per area points to verification status. |
| Differences from upstream: [English](compatibility.md), [Japanese](compatibility.jp.md) | Concise comparison tables with stable IDs: `D-xx` for upstream behavior versus project behavior, `O-xx` for Unix-like versus Windows destinations, each linked to the detailed design decision. |
| [Adversarial design review](design-review.md) | Review findings, proposed decisions, and links to subsequently accepted decisions. Proposals are not requirements unless explicitly accepted. |
| [Validation plan and results](validation.md) | Scenario IDs, reproducible checks, observed results, verification narratives, and pending coverage. Fixture success is distinct from product conformance. |

## Writing and Updating Documents

Reproducible environment instructions: [Linux identity experiment](../tests/prototypes/identity-isolation/README.md)
and [Windows dockur fixture](../tests/environments/windows/README.md).

- Write primary documentation in English. Keep existing Japanese translations in sync when changing the corresponding English documents.
- Keep detailed decisions in the design document. The differences list contains summaries and links, not duplicated explanations of rationale, scope, implementation, or open questions.
- When a difference is accepted, update the design document and add or update its comparison row in both language versions of the differences list. Preserve its ID and link to the detailed decision.
- If a decision originated in the review, record its disposition there and link to the design. Keep unaccepted proposals distinct from accepted requirements.
- Describe upstream differences neutrally; do not label them as defects or bug fixes without an explicit decision to do so. Each recorded difference states the apparent upstream intent and the reason to differ, taken from the design stance list (DL-24).
- Distinguish accepted designs from implemented and verified behavior. Record verification narratives in the validation document; the design document keeps one line and a link per area.
- Record each decision once, in the section it affects, and add a row to the design document's decision log with the next `DL-xx` ID. Name that ID in the commit message.
- Check relative links and section anchors when moving or renaming documents or headings.
