use std::collections::BTreeMap;

pub struct Node<'a, T> {
    pub key: &'a str,
    pub value: T,
    pub deps: Vec<&'a str>,
}

pub enum Scc<T> {
    Acyclic(T),
    Cyclic(Vec<T>),
}

/// Iterative Tarjan's SCC. Mirrors Haskell's `Data.Graph.stronglyConnComp`.
///
/// Takes ownership of nodes, runs Tarjan on the key/deps graph,
/// then moves each node's `.value` into the SCC result.
pub fn strongly_connected_components<T>(nodes: Vec<Node<'_, T>>) -> Vec<Scc<T>> {
    let n = nodes.len();
    let mut index_of: BTreeMap<&str, usize> = BTreeMap::new();
    for (i, node) in nodes.iter().enumerate() {
        index_of.insert(node.key, i);
    }

    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut has_self_edge = vec![false; n];
    for (i, node) in nodes.iter().enumerate() {
        for dep in &node.deps {
            if let Some(&j) = index_of.get(dep) {
                if i == j {
                    has_self_edge[i] = true;
                } else {
                    adj[i].push(j);
                }
            }
        }
    }

    tarjan(n, &adj, &has_self_edge, nodes)
}

fn tarjan<T>(
    n: usize,
    adj: &[Vec<usize>],
    has_self_edge: &[bool],
    nodes: Vec<Node<'_, T>>,
) -> Vec<Scc<T>> {
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
                        let w = stack
                            .pop()
                            .expect("SCC stack contains root by Tarjan invariant");
                        on_stack[w] = false;
                        component.push(w);
                        if w == v {
                            break;
                        }
                    }
                    result_sccs.push(component);
                }

                let finished_lowlink = lowlink[v];
                work.pop();
                if let Some((parent, _, _)) = work.last() {
                    lowlink[*parent] = lowlink[*parent].min(finished_lowlink);
                }
            }
        }
    }

    // Move values out of nodes so we can return them in SCC order.
    let mut values: Vec<Option<T>> = nodes.into_iter().map(|n| Some(n.value)).collect();

    // Tarjan emits SCCs with leaves (no dependencies) first,
    // which is exactly the processing order we want (deps before dependents).
    result_sccs
        .into_iter()
        .map(|component| {
            if component.len() == 1 && !has_self_edge[component[0]] {
                Scc::Acyclic(
                    values[component[0]]
                        .take()
                        .expect("each node consumed exactly once"),
                )
            } else {
                Scc::Cyclic(
                    component
                        .into_iter()
                        .map(|i| values[i].take().expect("each node consumed exactly once"))
                        .collect(),
                )
            }
        })
        .collect()
}
