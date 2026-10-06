//! One module per subcommand. Each takes its clap arguments and returns a
//! typed outcome (rendered by [`render`](self) functions as counts and ids)
//! or a typed error.

pub mod diff;
pub mod export;
pub mod input_view;
pub mod run;
pub mod score;
pub mod validate;
