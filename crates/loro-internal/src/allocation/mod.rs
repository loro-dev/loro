pub mod allocation;
mod test_dfs;
mod types;
pub(crate) use allocation::calc_critical_version;
pub(crate) use test_dfs::calc_critical_version_dfs;
