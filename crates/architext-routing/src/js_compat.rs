//! Exact reproductions of the JS numeric/string/sort semantics the router relies
//! on. Each primitive is verified against Node golden values; getting one wrong
//! produces a different diagram, not a crash.
