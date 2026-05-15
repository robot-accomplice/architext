# Third-Party Notices

Architext is MIT licensed. This file records third-party software and algorithm
references that influence the project.

This is not legal advice. It is a project-maintained attribution and license
posture document.

## Packaged Runtime And Build Dependencies

Architext's npm package and built viewer use open-source JavaScript packages.
The package manager remains the source of truth for exact transitive dependency
versions.

Direct runtime/build dependencies:

| Project | License | Use |
| --- | --- | --- |
| React | MIT | Viewer UI runtime |
| React DOM | MIT | Viewer UI runtime |
| Scheduler | MIT | React runtime dependency included through React DOM |
| AJV | MIT | JSON Schema validation |
| AJV Formats | MIT | JSON Schema format validation |
| Vite | MIT | Viewer build tooling |
| TypeScript | Apache-2.0 | Viewer type checking and build tooling |
| @vitejs/plugin-react | MIT | Viewer build tooling |

## Routing Algorithm References

Architext's router is custom project code. It does not copy source code from
the projects below. The project studies their public documentation and papers
for established routing concepts such as fixed-node routing, explicit ports,
orthogonal routing, monotonic path restrictions, edge crossing costs, bridge
rendering, and route/label planning.

| Project | License / Terms | Architext Use |
| --- | --- | --- |
| Eclipse Layout Kernel / elkjs | EPL-2.0 | Algorithm reference only |
| Adaptagrams / libavoid | LGPL, with commercial dual licensing available | Algorithm reference only |
| yFiles | Commercial SDK | Capability reference only |
| JointJS Community | MPL-2.0 | Router behavior reference only |
| JointJS+ | Commercial product | Capability reference only |
| Cytoscape.js | MIT | Visualization ergonomics reference only |
| React Flow | MIT | Edge-style/UI ergonomics reference only |
| Graphviz | EPL-2.0 for current versions | Export/layout reference only |
| Sprotty | Eclipse open-source project | Architecture reference only |
| D2 | MPL-2.0 | Diagram-language reference only |
| TALA | Proprietary/closed-source | Capability reference only |

Any future change that copies, ports, bundles, wraps, or links third-party
routing code must update this file and include the required license text,
copyright notices, and distribution obligations before the change ships.

## Product Inspiration

The original project idea for Architext was inspired by Dave J's x.com post
about interactive architecture and flow visualization:

https://x.com/davej/status/2053867258653339746?s=46&t=e_qP9a_xUWuOJ6eKxFpaAQ
