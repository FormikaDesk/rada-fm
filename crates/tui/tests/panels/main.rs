//! The panels view: the screen as rounded boxes, drawn on the demo home folder at the sizes
//! that matter, and what each part does (the focus, the tree, the preview).
//!
//! Screens are committed as text (`tests/panels/snapshots/`), with a token map of the colours
//! for each theme: a change to the look shows up as a diff to review.

#[path = "../common/mod.rs"]
mod harness;

mod colours;
mod common;
mod focus;
mod git;
mod layout;
mod pills;
mod places;
mod ring;
mod screens;
mod tree;
mod two_columns;
mod way;
