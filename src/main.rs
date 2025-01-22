use serde::Deserialize;
use std::fmt::Display;
use std::io::{BufReader, Write};
use std::fs::File;
use std::collections::{HashMap, HashSet};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};
use petgraph::graph::{Graph, NodeIndex};
use petgraph::{EdgeType, Undirected};
use petgraph::algo;
use petgraph::visit::{Dfs, IntoNodeReferences, Walker};
use petgraph::csr::IndexType;
use petgraph::dot::{Dot, Config};
use clap::Parser;
use rayon::prelude::*;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub struct Cli {
    #[arg(long)]
    pub system: String,

    #[arg(long)]
    pub map: String,
}

#[allow(non_snake_case)]
#[derive(Debug, Clone, Deserialize)]
struct SolarSystem {
    solarSystemID: u32,
    solarSystemName: String,
    neighbours: Vec<u32>,
}

#[derive(Clone, Debug)]
pub struct System {
    pub id: u32,
    pub name: String,
}

#[derive(Clone)]
pub struct SystemWithJump {
    pub id: u32,
    pub name: String,
    pub jumps: Vec<usize>,
}

impl Display for SystemWithJump {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {:?}", self.name, self.jumps)
    }
}

pub fn debug_graph(graph: &Graph<System, f32, Undirected>, path: &Vec<u32>, name: String) {
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis();

    let dot_file = format!("graph_{}_{}.dot", name, timestamp);

    let mut jump_positions = HashMap::<u32,Vec<usize>>::new();
    for (position, id) in path.iter().enumerate() {
        jump_positions.entry(*id).or_default().push(position);
    }

    let graph_with_jumps = graph.filter_map(|_, s| {
        Some(SystemWithJump {
            id: s.id,
            name: s.name.clone(),
            jumps: jump_positions.get(&s.id).cloned().unwrap_or_default(),
        })
    }, |_, d| Some(d.clone()));

    let mut f = File::create(&dot_file).unwrap();
    let output = format!("{}", Dot::with_config(&graph_with_jumps, &[Config::EdgeNoLabel]));
    f.write_all(&output.as_bytes()).unwrap();

    let _ = Command::new("dot")
        .args(["-Tpng".to_owned(), format!("-ograph_{}_{}.png", name, timestamp), dot_file])
        .spawn();
}

pub fn filter_nodes<'a, N, E, Ty, Ix, F>(
    graph: &'a Graph<N, E, Ty, Ix>,
    mut node_map: F,
) -> Graph<N, E, Ty, Ix>
where
    F: FnMut(NodeIndex<Ix>, &'a N) -> bool,
    Ty: EdgeType,
    Ix: IndexType,
    N: Clone,
    E: Clone,
{
    graph.filter_map(|i, n| {
        if node_map(i, n) {
            Some(n.clone())
        } else {
            None
        }
    }, |_, e| Some(e.clone()))
}

pub fn find_longest_paths(original_graph: Graph<System, f32, Undirected>, start_id: u32) -> Vec<u32> {
    let mut graph = original_graph.clone();
    let mut result = Vec::<u32>::new();

    loop {
        let start_index = graph.node_references().find(|(_, system)| {
            system.id == start_id
        }).expect("Start node disappeared").0;

        if let Some(longest_path) = graph.node_indices().par_bridge().filter_map(|n| {
            algo::all_simple_paths(&graph, start_index, n, 0, None).max_by_key(|v: &Vec<NodeIndex>| v.len())
        }).max_by_key(|v| v.len()) {
            let longest_path = longest_path.into_iter().skip(1).collect::<Vec<_>>();

            let return_path = algo::astar(
                &graph,
                *longest_path.last().expect("Got an empty path"),
                |n| n == start_index,
                |_| 1,
                |_| 0,
            ).expect("Cannot return to start").1.into_iter().skip(1).collect::<Vec<_>>();
            
            let full_path = longest_path.into_iter().chain(return_path.into_iter()).collect::<Vec<_>>();
            let full_path_id = full_path.iter().map(|n| graph[*n].id).collect::<Vec<_>>();

            result.extend(full_path_id);

            graph = filter_nodes(&graph, |i, _| !full_path.contains(&i) || i == start_index);
        } else {
            break;
        }
    }

    let mut final_result = Vec::<u32>::new();
    let mut visited = HashSet::<u32>::new();
    visited.insert(start_id);

    for id in &result {
        final_result.push(*id);

        if !visited.contains(id) {
            let sub_graph = filter_nodes(&original_graph, |_, n| (!result.contains(&n.id) && !final_result.contains(&n.id)) || n.id == *id);
            let path = find_longest_paths(sub_graph.clone(), *id);
            
            final_result.extend(path);

            visited.insert(*id);
        }
    }

    final_result
}

pub fn sort_tuple<T>(v: (T, T)) -> (T, T)
where  T: PartialOrd {
    if v.0 < v.1 {
        (v.0, v.1)
    } else {
        (v.1, v.0)
    }
}

fn main() {
    let args = Cli::parse();

    let file = File::open(args.map).expect("data.json not found");
    let reader = BufReader::new(file);
    let data : HashMap<u32, SolarSystem> = serde_json::from_reader(reader).expect("Deserialization failed");

    let mut graph = Graph::<System, f32, Undirected>::new_undirected();
    let mut node_index = HashMap::<u32, NodeIndex>::new();

    for (_, ss) in data.iter() {
        node_index.insert(ss.solarSystemID, graph.add_node(System {
            id: ss.solarSystemID,
            name: ss.solarSystemName.clone(),
        }));
    }

    let mut added = HashSet::<(u32,u32)>::new();
    for (_, ss) in data.iter() {
        let index1 = *node_index.get(&ss.solarSystemID).unwrap();
        for n in &ss.neighbours {
            let index2 = *node_index.get(n).unwrap();
            let system_pair = sort_tuple((ss.solarSystemID, *n));
            if !added.contains(&system_pair) {
                graph.add_edge(index1, index2, 1.0);
                added.insert(system_pair);
            }
        }
    }

    println!("Entire game cyclic: {}", algo::is_cyclic_undirected(&graph));

    let (start_node, start_system) = graph.node_references().find(|(_, system)| {
        system.name == args.system
    }).expect("Starting system not found");

    let nodes = Dfs::new(&graph, start_node).iter(&graph).collect::<HashSet<_>>();
    let graph = filter_nodes(&graph, |i, _| nodes.contains(&i));

    println!("Region cyclic: {}", algo::is_cyclic_undirected(&graph));

    let result = find_longest_paths(graph.clone(), start_system.id);

    let mut name_lookup = HashMap::<u32,String>::new();
    for (_, n) in graph.node_references() {
        name_lookup.insert(n.id, n.name.clone());
    }

    let result_names = result.iter().map(|id| name_lookup.get(id).cloned().unwrap_or_default()).collect::<Vec<_>>();
    println!("Jumps: {}", result_names.len());
    println!("Path: {:?}", result_names);

    debug_graph(&graph, &result, args.system);
}
