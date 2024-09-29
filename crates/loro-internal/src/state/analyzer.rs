use crate::{change::Timestamp, LoroDoc};
use fxhash::FxHashMap;
use loro_common::ContainerID;
use rle::HasLength;

#[derive(Debug, Clone)]
pub struct DocAnalysis {
    pub containers: FxHashMap<ContainerID, ContainerAnalysisInfo>,
}

#[derive(Debug, Clone)]
pub struct ContainerAnalysisInfo {
    pub size: u32,
    pub dropped: bool,
    pub depth: u32,
    pub ops_num: u32,
    pub last_edit_time: Timestamp,
}

impl DocAnalysis {
    pub fn analyze(doc: &LoroDoc) -> Self {
        let mut ops_nums = FxHashMap::default();
        let mut last_edit_time = FxHashMap::default();
        {
            let oplog = doc.oplog().try_lock().unwrap();
            oplog.change_store().visit_all_changes(&mut |c| {
                for op in c.ops().iter() {
                    let idx = op.container;
                    let info = ops_nums.entry(idx).or_insert(0);
                    *info += op.atom_len();

                    let time = last_edit_time.entry(idx).or_insert(c.timestamp());
                    if *time < c.timestamp() {
                        *time = c.timestamp();
                    }
                }
            });
        }

        let mut containers = FxHashMap::default();
        let mut state = doc.app_state().try_lock().unwrap();
        let alive_containers = state.get_all_alive_containers();
        for (&idx, c) in state.iter_all_containers_mut() {
            let ops_num = ops_nums.get(&idx).unwrap_or(&0);
            let id = doc.arena().get_container_id(idx).unwrap();
            let dropped = !alive_containers.contains(&id);
            containers.insert(
                id,
                ContainerAnalysisInfo {
                    depth: c.depth() as u32,
                    dropped,
                    size: c.encode().len() as u32,
                    ops_num: *ops_num as u32,
                    last_edit_time: *last_edit_time.get(&idx).unwrap_or(&0),
                },
            );
        }

        Self { containers }
    }

    pub fn len(&self) -> usize {
        self.containers.len()
    }

    pub fn dropped_len(&self) -> usize {
        self.containers
            .iter()
            .filter(|(_, info)| info.dropped)
            .count()
    }

    pub fn tiny_container_len(&self) -> usize {
        self.containers
            .iter()
            .filter(|(_, info)| info.size < 128)
            .count()
    }

    pub fn large_container_len(&self) -> usize {
        self.containers
            .iter()
            .filter(|(_, info)| info.size >= 1024)
            .count()
    }
}
