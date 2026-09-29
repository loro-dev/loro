---
"loro.js": patch
---

Fix a `loro.js` checkout event for a child container in a List when the checkout falls back to replaying history, for example after updates were imported while the document was detached. The List diff could delete and re-insert an unchanged child container without sending its content, so a listener that starts an inserted child from empty lost that child's content. The child's whole state is now sent, as for any other re-attached child.
