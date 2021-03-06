#[macro_use]
extern crate integration;

#[cfg(debug_assertions)]
const BINARY: &str = "../target/debug/rlox";

#[cfg(not(debug_assertions))]
const BINARY: &str = "../target/release/rlox";

#[cfg(test)]
define_integration_tests!();
