pub mod cli;
pub mod cmd;
mod download;
pub mod proxy;

pub use cli::Cli;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub const BANNER: &str = color_print::cstr!(
    r#"
      ___           ___           ___           ___
     /\  \         /\  \         /\  \         /\__\
    /::\  \       /::\  \       /::\  \       /:/  /
   /:/\:\  \     /:/\:\  \     /:/\ \  \     /:/__/
  /:/  \:\  \   /::\~\:\  \   _\:\~\ \  \   /::\  \ ___
 /:/__/ \:\__\ /:/\:\ \:\__\ /\ \:\ \ \__\ /:/\:\  /\__\
 \:\  \  \/__/ \/__\:\/:/  / \:\ \:\ \/__/ \/__\:\/:/  /
  \:\  \            \::/  /   \:\ \:\__\        \::/  /
   \:\  \           /:/  /     \:\/:/  /        /:/  /
    \:\__\         /:/  /       \::/  /        /:/  /
     \/__/         \/__/         \/__/         \/__/

 The <green><bold>Nash</bold></green> programming language.

 <magenta>repo:</magenta> <blue><italic><dim>https://github.com/nash-script/compiler</dim></italic></blue>
 <magenta>docs:</magenta> <blue><italic><dim>https://nash-script.dev</dim></italic></blue>
 <magenta>chat:</magenta> <blue><italic><dim>https://discord.gg/3qQGrKT3eE</dim></italic></blue>"#
);
