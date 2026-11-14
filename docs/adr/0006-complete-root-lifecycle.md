# Complete content transfers before retiring their source roots

The graph operation owns the full content transfer: check references and policy,
detach the payload, insert it at the destination, then finish the source root.
A temporarily empty source is private intermediate state. Same-root moves must
finish insertion before deciding whether the source has become empty.

Root lifecycle operations distinguish detaching ownership from destruction.
Detaching preserves the tree for a transfer. Retirement removes the root, its
presentation, every remaining subtree node and their placements together. An
empty main central dockspace survives removal of its last content; an emptied
contained floating root retires. Explicitly constructed empty central regions
remain valid layout inputs.

The operation returns actual topology effects to its caller. The single-item
fast path consumes those effects to prune the affected surface; it does not
independently predict which root will retire. Batch transactions retain private
intermediate surfaces until their final cleanup. Complete-root drag queries
share the same singleton retention rule in indexed and live paths.

This keeps lifecycle policy in one internal module without adding persistent
bookkeeping or full-workspace checks to ordinary pointer moves. Regression
tests compare local completion with general normalization, validate successful
results, conserve item ownership and require failed transactions to leave their
input unchanged. These checks cover graph transitions; platform capture and
actual window presentation still require adapter and real-window verification.
