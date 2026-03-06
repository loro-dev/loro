#![allow(dead_code)]
#![allow(unused)]

mod bfs;
pub(crate) use bfs::calc_critical_version_bfs as calc_critical_version;

// Only for testing
mod dfs;
pub(crate) use dfs::calc_critical_version_dfs;
pub(crate) use dfs::get_end_list;
