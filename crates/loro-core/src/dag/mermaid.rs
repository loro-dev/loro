use rle::RleVec;

use super::*;
struct BreakPoints {
    vv: VersionVector,
    break_points: FxHashMap<ClientID, FxHashSet<Counter>>,
    /// start ID to ID. The target ID may be in the middle of an op.
    ///
    /// only includes links across different clients
    links: FxHashMap<ID, ID>,
}

struct Output {
    clients: FxHashMap<ClientID, Vec<IdSpan>>,
    /// start ID to start ID.
    ///
    /// only includes links across different clients
    links: FxHashMap<ID, ID>,
}

fn to_str(output: Output) -> String {
    let mut s = String::new();
    let mut indent_level = 0;
    macro_rules! new_line {
        () => {
            s += "\n";
            for _ in 0..indent_level {
                s += "    ";
            }
        };
    }
    s += "flowchart RL";
    indent_level += 1;
    new_line!();
    for (client_id, spans) in output.clients.iter() {
        s += format!("subgraph client{}", client_id).as_str();
        new_line!();
        let mut is_first = true;
        for id_span in spans.iter().rev() {
            if !is_first {
                s += " --> "
            }

            is_first = false;
            s += format!(
                "{}-{}(ctr: {}..{})",
                id_span.client_id, id_span.counter.from, id_span.counter.from, id_span.counter.to
            )
            .as_str();
        }

        new_line!();
        s += "end";
        new_line!();
        new_line!();
    }

    for (id_from, id_to) in output.links.iter() {
        s += format!(
            "{}-{} --> {}-{}",
            id_from.client_id, id_from.counter, id_to.client_id, id_to.counter
        )
        .as_str();
        new_line!();
    }

    s
}

fn break_points_to_output(input: BreakPoints) -> Output {
    let mut output = Output {
        clients: FxHashMap::default(),
        links: FxHashMap::default(),
    };
    let breaks: FxHashMap<ClientID, Vec<Counter>> = input
        .break_points
        .into_iter()
        .map(|(client_id, set)| {
            let mut vec: Vec<Counter> = set.iter().cloned().collect();
            vec.sort();
            (client_id, vec)
        })
        .collect();
    for (client_id, break_points) in breaks.iter() {
        let mut spans = vec![];
        for (from, to) in break_points.iter().zip(break_points.iter().skip(1)) {
            spans.push(IdSpan::new(*client_id, *from, *to));
        }
        output.clients.insert(*client_id, spans);
    }

    for (id_from, id_to) in input.links.iter() {
        let client_breaks = breaks.get(&id_to.client_id).unwrap();
        match client_breaks.binary_search(&id_to.counter) {
            Ok(_) => {
                output.links.insert(*id_from, *id_to);
            }
            Err(index) => {
                output
                    .links
                    .insert(*id_from, ID::new(id_to.client_id, client_breaks[index - 1]));
            }
        }
    }
    output
}

fn get_dag_break_points<T: DagNode>(dag: &impl Dag<Node = T>) -> BreakPoints {
    let mut break_points = BreakPoints {
        vv: dag.vv(),
        break_points: FxHashMap::default(),
        links: FxHashMap::default(),
    };

    for node in dag.iter() {
        let id = node.id_start();
        let set = break_points.break_points.entry(id.client_id).or_default();
        set.insert(id.counter);
        set.insert(id.counter + node.len() as Counter);
        for dep in node.deps() {
            if dep.client_id == id.client_id {
                continue;
            }

            break_points
                .break_points
                .entry(dep.client_id)
                .or_default()
                .insert(dep.counter);
            break_points.links.insert(id, *dep);
        }
    }

    break_points
}

pub(crate) fn dag_to_mermaid<T: DagNode>(dag: &impl Dag<Node = T>) -> String {
    to_str(break_points_to_output(get_dag_break_points(dag)))
}
