# Product cost attribution evidence

Each condition retains raw process stdout/stderr, pooled CSV and exact source,
compiler and executable provenance. The fresh baseline uses the previous
qualification's frozen, SHA-verified executables; its original build provenance
is retained rather than attributing the binary to newly edited source.

`candidate-*-source/sources.json` uses original workspace paths. Each archived
source has `.txt` appended to that path so that frozen code is not mistaken for
current Rust modules. Contents byte-match the recorded source digest.

CPU samples are separate from formal timings. They include startup/readiness,
warmup, untimed diagnostics and output disposal. Category roots are disjoint
within each category; different categories can overlap. The reproduction helpers
under `tools/` are frozen Python text artifacts for this macOS `sample` run;
the portable product runner remains `scripts/compare_products.py`.

See the adjacent dated result document for interpretation and qualification.
