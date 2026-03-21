use std::collections::BTreeMap;

pub enum Scc<'a> {
    Acyclic(&'a str),
    Cyclic(Vec<&'a str>),
}

/// Iterative Tarjan's SCC. Mirrors Haskell's `Data.Graph.stronglyConnComp`.
pub fn strongly_connected_components<'a>(
    nodes: &[&'a str],
    edges: &BTreeMap<&'a str, Vec<&'a str>>,
) -> Vec<Scc<'a>> {
    let n = nodes.len();
    let mut index_of: BTreeMap<&str, usize> = BTreeMap::new();
    for (i, &node) in nodes.iter().enumerate() {
        index_of.insert(node, i);
    }

    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut has_self_edge = vec![false; n];
    for (i, &node) in nodes.iter().enumerate() {
        if let Some(deps) = edges.get(node) {
            for &dep in deps {
                if let Some(&j) = index_of.get(dep) {
                    if i == j {
                        has_self_edge[i] = true;
                    } else {
                        adj[i].push(j);
                    }
                }
            }
        }
    }

    let mut order: Vec<usize> = vec![0; n];
    let mut lowlink: Vec<usize> = vec![0; n];
    let mut on_stack: Vec<bool> = vec![false; n];
    let mut visited: Vec<bool> = vec![false; n];
    let mut stack: Vec<usize> = Vec::new();
    let mut counter: usize = 0;
    let mut result_sccs: Vec<Vec<usize>> = Vec::new();

    let mut work: Vec<(usize, usize, bool)> = Vec::new();

    for start in 0..n {
        if visited[start] {
            continue;
        }

        work.push((start, 0, true));

        while let Some((v, ni, is_init)) = work.last_mut() {
            let v = *v;

            if *is_init {
                order[v] = counter;
                lowlink[v] = counter;
                counter += 1;
                visited[v] = true;
                on_stack[v] = true;
                stack.push(v);
                *is_init = false;
            }

            if *ni < adj[v].len() {
                let w = adj[v][*ni];
                *ni += 1;
                if !visited[w] {
                    work.push((w, 0, true));
                } else if on_stack[w] {
                    lowlink[v] = lowlink[v].min(order[w]);
                }
            } else {
                if lowlink[v] == order[v] {
                    let mut component = Vec::new();
                    loop {
                        let w = stack.pop().unwrap();
                        on_stack[w] = false;
                        component.push(w);
                        if w == v {
                            break;
                        }
                    }
                    result_sccs.push(component);
                }

                let finished_v = v;
                let finished_lowlink = lowlink[v];
                work.pop();
                if let Some((parent, _, _)) = work.last() {
                    lowlink[*parent] = lowlink[*parent].min(finished_lowlink);
                    let _ = finished_v;
                }
            }
        }
    }

    // Tarjan emits SCCs with leaves (no dependencies) first,
    // which is exactly the processing order we want (deps before dependents).
    result_sccs
        .into_iter()
        .map(|component| {
            if component.len() == 1 && !has_self_edge[component[0]] {
                Scc::Acyclic(nodes[component[0]])
            } else {
                Scc::Cyclic(component.into_iter().map(|i| nodes[i]).collect())
            }
        })
        .collect()
}
