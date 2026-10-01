# Documentation

## Document Roles

| Document | Purpose |
| --- | --- |
| Design: [English](design.md), [Japanese](design.jp.md) | The current design in the present tense: scope, decisions with the reasons that still hold, and the delivery plan. Edited in place when the design changes. |
| [Open questions](open-questions.md) | What is not decided yet, and what will decide each item. |
| Differences from upstream: [English](compatibility.md), [Japanese](compatibility.jp.md) | Concise comparison tables with stable IDs: `D-xx` for upstream behavior versus project behavior, `O-xx` for Unix-like versus Windows destinations, each linked to the detailed design decision. |
| [Adversarial design review](design-review.md) | A record of review findings and their dispositions, frozen when written. Proposals are not requirements unless the design states them. |
| [Validation plan and results](validation.md) | Scenario IDs, reproducible checks, observed results, verification narratives, and pending coverage. Fixture success is distinct from product conformance. |

## Writing and Updating Documents

Reproducible environment instructions: [Linux identity experiment](../tests/prototypes/identity-isolation/README.md)
and [Windows dockur fixture](../tests/environments/windows/README.md).

- Write primary documentation in English. Keep existing Japanese translations in sync when changing the corresponding English documents.
- Keep detailed decisions in the design document. The differences list contains summaries and links, not duplicated explanations of rationale, scope, implementation, or open questions.
- When a difference is accepted, update the design document and add or update its comparison row in both language versions of the differences list. Preserve its ID and link to the detailed decision.
- If a decision originated in the review, record its disposition there and link to the design. Keep unaccepted proposals distinct from accepted requirements.
- Describe upstream differences neutrally; do not label them as defects or bug fixes without an explicit decision to do so. Each difference states the apparent upstream intent and the reason to differ, taken from the design stance list.
- Distinguish accepted designs from implemented and verified behavior. Record verification narratives in the validation document; the design document names the validation matrix rows it depends on.
- The design and the differences list are living documents: when a decision changes, edit the section so it states the current design. Do not add decision dates, "superseded" notes, or former behavior; the history is in git.
- The reason for a change goes in its commit message, in one or two sentences, together with any difference ID (`D-xx`, `O-xx`) the change adds or changes.
- What is not decided goes in [open-questions.md](open-questions.md), not in the design. When an item is decided, remove it there and state the result in the design.
- Living documents do not link to dated records such as validation results or the review; records may link to living documents.
- Check relative links and section anchors when moving or renaming documents or headings.
