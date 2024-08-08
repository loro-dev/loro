pub mod allocation_tree;
mod allocation_tree_types;
mod dfs;
mod lamport_split;
pub(crate) use allocation_tree::calc_critical_version_allocation_tree;
pub(crate) use dfs::calc_critical_version_dfs;
pub(crate) use lamport_split::calc_critical_version_lamport_split;
