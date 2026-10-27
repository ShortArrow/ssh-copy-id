# Documentation

## Document Roles

| Document | Purpose |
| --- | --- |
| [Design](design.md) | The current design in the present tense: scope, decisions with the reasons that still hold, and the delivery plan. Edited in place when the design changes. |
| [Open questions](open-questions.md) | What is not decided yet, and what will decide each item. |
| [Differences from upstream](compatibility.md) | Concise comparison tables with stable IDs: `D-xx` for upstream behavior versus project behavior, `O-xx` for Unix-like versus Windows destinations, each linked to the detailed design decision. |
| [Behavior defined where the standards are silent](definitions.md) | `U-xx` rows: where the SSH and SFTP standards or the OpenSSH documentation leave a behavior undefined and this tool defines it, with the source and a link to the design. |
| [Adversarial design review](design-review.md) | A record of review findings and their dispositions, frozen when written. Proposals are not requirements unless the design states them. |
| [Prior art](prior-art.md) | A dated record of other tools that install keys from or onto Windows, compared on the points the design had to decide, frozen when written. |
| [Validation plan and results](validation.md) | Scenario IDs, reproducible checks, observed results, verification narratives, and pending coverage. Fixture success is distinct from product conformance. |

Reproducible environment instructions: [Linux identity experiment](../tests/prototypes/identity-isolation/README.md)
and [Windows dockur fixture](../tests/environments/windows/README.md).
