use std::cmp::Ordering;

use super::*;
impl HasIndex for AppDagNode {
    type Int = Counter;
    fn get_start_index(&self) -> Self::Int {
        self.cnt
    }

    fn get_end_index(&self) -> Self::Int {
        self.cnt + self.len as Counter
    }
}

impl Sliceable for AppDagNode {
    fn slice(&self, from: usize, to: usize) -> Self {
        AppDagNode {
            client: self.client,
            cnt: self.cnt + from as Counter,
            lamport: self.lamport + from as Lamport,
            parents: Default::default(),
            vv: Default::default(),
            len: to - from,
        }
    }
}

impl HasId for AppDagNode {
    fn id_start(&self) -> ID {
        ID {
            peer: self.client,
            counter: self.cnt,
        }
    }
}

impl HasLength for AppDagNode {
    fn atom_len(&self) -> usize {
        self.len
    }

    fn content_len(&self) -> usize {
        self.len
    }
}

impl Mergable for AppDagNode {}

impl HasLamport for AppDagNode {
    fn lamport(&self) -> Lamport {
        self.lamport
    }
}

impl DagNode for AppDagNode {
    fn deps(&self) -> &[ID] {
        &self.parents
    }
}

impl Dag for AppDag {
    type Node = AppDagNode;

    fn frontier(&self) -> &[ID] {
        &self.frontiers
    }

    fn get(&self, id: ID) -> Option<&Self::Node> {
        let ID {
            peer: client_id,
            counter,
        } = id;
        self.map
            .get(&client_id)
            .and_then(|rle| rle.get(counter).map(|x| x.element))
    }

    fn vv(&self) -> VersionVector {
        self.vv.clone()
    }
}

impl AppDag {
    /// get the version vector for a certain op.
    /// It's the version when the op is applied
    pub fn get_vv(&self, id: ID) -> Option<ImVersionVector> {
        self.map.get(&id.peer).and_then(|rle| {
            rle.get(id.counter).map(|x| {
                let mut vv = x.element.vv.clone();
                vv.insert(id.peer, id.counter);
                vv
            })
        })
    }

    /// Compare the causal order of two versions.
    /// If None, two versions are concurrent to each other
    pub fn cmp_version(&self, a: ID, b: ID) -> Option<Ordering> {
        if a.peer == b.peer {
            return Some(a.counter.cmp(&b.counter));
        }

        let a = self.get_vv(a).unwrap();
        let b = self.get_vv(b).unwrap();
        a.partial_cmp(&b)
    }

    pub fn get_lamport(&self, id: &ID) -> Option<Lamport> {
        self.map.get(&id.peer).and_then(|rle| {
            rle.get(id.counter)
                .map(|x| x.element.lamport + (id.counter - x.element.cnt) as Lamport)
        })
    }
}
